use chrono::{Duration, Utc};
use path_slash::PathBufExt;
use pixivcrab::AppAPI;

use log::{info, warn};
use snafu::ResultExt;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    path::{Path, PathBuf},
};

use crate::{
    error::{self, BoxError},
    model::{
        pixiv::{self, NovelHistory, PixivIllust, PixivNovel, PixivUser, UserHistory},
        History, Hsv, ImageMedia, LocalMedia,
    },
};

use mongodb::{
    bson::{doc, oid::ObjectId, to_bson, DateTime, Document},
    options::{self, FindOneAndUpdateOptions, UpdateOptions},
    Collection, Database, IndexModel,
};

async fn update_users(
    users_map: BTreeMap<String, &pixivcrab::models::user::User>,
    users_need_update_set: &mut BTreeSet<String>,
    c_user: &Collection<Document>,
) -> crate::Result<HashMap<String, ObjectId>> {
    let mut users_to_oid = HashMap::new();

    for (user_id, user) in users_map {
        let r = c_user
            .find_one_and_update(
                doc! {"source_id": &user_id},
                doc! {"$set": {
                    "source_inaccessible": false,
                    "extension.is_followed": user.is_followed,
                }},
                FindOneAndUpdateOptions::builder()
                    .upsert(true)
                    .return_document(options::ReturnDocument::After)
                    .projection(doc! {
                        "_id": true,
                        "last_modified": true,
                        "history.extension.avatar_url": true,
                    })
                    .build(),
            )
            .await
            .context(error::MongoDb)?
            .ok_or(error::MongoNotMatch.build())?;
        let parent_id = r.get_object_id("_id").context(error::MongoValueAccess)?;
        users_to_oid.insert(user_id.clone(), parent_id);

        // Do not insert to update list if updated in 7 days and the avatar is up-to-date.
        if let Ok(last_modified) = r.get_datetime("last_modified") {
            if last_modified.to_chrono() >= Utc::now() - Duration::weeks(1) {
                // The user was updated in last 7 days.
                if let Ok(histories) = r.get_array("history") {
                    if let Some(h) = histories.last() {
                        let s = h
                            .as_document()
                            .ok_or(error::MongoNotMatch.build())?
                            .get_document("extension")
                            .context(error::MongoValueAccess)?
                            .get_str("avatar_url")
                            .context(error::MongoValueAccess)?;
                        if s == user.profile_image_urls.medium {
                            // The avatar of the user is up-to-date.
                            continue;
                        }
                    }
                }
            }
        }
        users_need_update_set.insert(user_id);
    }
    Ok(users_to_oid)
}

async fn update_tags(
    tags_set: HashSet<Vec<String>>,
    c_tag: &Collection<Document>,
) -> crate::Result<HashMap<String, ObjectId>> {
    let mut tags_to_oid = HashMap::new();
    for alias in tags_set {
        let regs: Vec<_> = alias
            .iter()
            .map(|a| bson::Regex {
                pattern: format!("^{}$", regex::escape(a)),
                options: "i".to_string(),
            })
            .collect();
        let r = c_tag
            .find_one_and_update(
                doc! { "alias": { "$in": regs }, "protected": false },
                doc! { "$addToSet": {"alias": { "$each": &alias } } },
                FindOneAndUpdateOptions::builder()
                    .upsert(true)
                    .return_document(options::ReturnDocument::After)
                    .projection(doc! {"_id": true})
                    .build(),
            )
            .await
            .context(error::MongoDb)?
            .ok_or(error::MongoNotMatch.build())?;
        for t in alias {
            let oid = r.get_object_id("_id").context(error::MongoValueAccess)?;
            tags_to_oid.insert(t, oid);
        }
    }
    Ok(tags_to_oid)
}

fn insert_tags_to_alias(tags: &Vec<pixivcrab::models::Tag>, tags_set: &mut HashSet<Vec<String>>) {
    for t in tags {
        let mut alias: Vec<String> = Vec::new();
        if t.name != "" {
            alias.push(t.name.clone());
        }
        if let Some(ref tr) = t.translated_name {
            if tr != "" {
                alias.push(tr.clone());
            }
        }

        if !alias.is_empty() {
            tags_set.insert(alias);
        }
    }
}

async fn set_item_invisible(c_item: &Collection<Document>, source_id: &str) -> crate::Result<()> {
    warn!("pixiv: Works {} is invisible!", source_id);
    c_item
        .update_one(
            doc! {
                "source_id": source_id
            },
            doc! {
                "$set": { "source_inaccessible": true }
            },
            UpdateOptions::builder().upsert(true).build(),
        )
        .await
        .context(error::MongoDb)?;
    Ok(())
}

pub async fn update_user_id_set(
    api: &AppAPI,
    c_user: &Collection<Document>,
    users_need_update_set: BTreeSet<String>,
) -> crate::Result<()> {
    let need_sleep = users_need_update_set.len() > 500;
    // Sleep for 1s to avoid 403 error
    for user_id in users_need_update_set {
        update_user_detail(api, &user_id, c_user).await?;
        if need_sleep {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }
    Ok(())
}

async fn update_user_detail(
    api: &AppAPI,
    user_id: &str,
    c_user: &Collection<Document>,
) -> crate::Result<()> {
    info!("updating pixiv user {}", user_id);
    let resp = api.user_detail(&user_id).await.context(error::PixivApi)?;
    let user = PixivUser {
        last_modified: Some(DateTime::now()),
        extension: Some(pixiv::User {
            is_followed: resp.user.is_followed.unwrap_or_default(),
            total_following: Some(resp.profile.total_follow_users),
            total_illust_series: Some(resp.profile.total_illust_series),
            total_illusts: Some(resp.profile.total_illusts),
            total_manga: Some(resp.profile.total_manga),
            total_novel_series: Some(resp.profile.total_novel_series),
            total_novels: Some(resp.profile.total_novels),
            total_public_bookmarks: Some(resp.profile.total_illust_bookmarks_public),
        }),
        ..Default::default()
    };
    c_user
        .update_one(
            doc! { "source_id": user_id },
            doc! { "$set": &to_bson(&user).context(error::BsonSerialize)? },
            UpdateOptions::builder().upsert(true).build(),
        )
        .await
        .context(error::MongoDb)?;

    fn filter_empty(s: &String) -> bool {
        !s.is_empty()
    }

    let history = History {
        last_modified: Some(DateTime::now()),
        extension: Some(UserHistory {
            account: resp.user.account,
            name: resp.user.name,
            avatar_url: Some(resp.user.profile_image_urls.medium),
            gender: Some(resp.profile.gender).filter(filter_empty),
            background_url: resp.profile.background_image_url.filter(filter_empty),
            birth: Some(resp.profile.birth).filter(filter_empty),
            comment: resp.user.comment.filter(filter_empty),
            is_premium: resp.profile.is_premium,
            region: resp.profile.region.filter(filter_empty),
            twitter_account: resp.profile.twitter_account.filter(filter_empty),
            web_page: resp.profile.webpage.filter(filter_empty),
            workspace_image_url: resp
                .workspace
                .get("workspace_image_url")
                .unwrap_or(&None)
                .clone(),
            workspace: {
                let mut workspace = BTreeMap::new();
                for (k, v) in resp.workspace {
                    if k == "workspace_image_url" {
                        continue;
                    }
                    if let Some(v) = v {
                        if !v.is_empty() {
                            workspace.insert(k, v);
                        }
                    }
                }
                if workspace.is_empty() {
                    None
                } else {
                    Some(workspace)
                }
            },
        }),
    };

    c_user
        .update_one(
            doc! {
                "source_id": user_id,
                "history.extension": {
                    "$ne": to_bson(history.extension.as_ref().unwrap()).context(error::BsonSerialize)?
                }
            },
            doc! { "$push": { "history": to_bson(&history).context(error::BsonSerialize)? } },
            None,
        )
        .await
        .context(error::MongoDb)?;
    Ok(())
}

pub async fn save_image(
    c_image: &Collection<Document>,
    size: i64,
    (w, h): (i32, i32),
    palette_hsv: Vec<Hsv>,
    url: String,
    image_path_db: String,
    image_path: impl AsRef<Path>,
) -> crate::Result<()> {
    c_image
        .update_one(
            doc! {"url": &url},
            doc! {
                "$set": to_bson(&LocalMedia {
                    _id: None,
                    url: Some(url),
                    local_path: image_path_db,
                    mime: mime_guess::from_path(image_path).first().map(|x| x.to_string()),
                    size,
                    extension: Some(ImageMedia {
                        width: w,
                        height: h,
                        palette_hsv,
                    })
                }).context(error::BsonSerialize)?
            },
            UpdateOptions::builder().upsert(true).build(),
        )
        .await
        .context(error::MongoDb)?;
    Ok(())
}

pub async fn save_image_ugoira(
    c_image: &Collection<Document>,
    zip_url: String,
    mut zip_path: PathBuf,
    zip_path_db: String,
    zip_size: i64,
    with_mp4: bool,
) -> Result<(), BoxError> {
    c_image
        .update_one(
            doc! {"url": &zip_url},
            doc! {
                "$set": to_bson(&LocalMedia {
                    _id: None,
                    url: Some(zip_url),
                    local_path: zip_path_db.clone(),
                    mime: Some("application/zip".to_string()),
                    size: zip_size,
                    extension: None::<ImageMedia>
                }).context(error::BsonSerialize)?
            },
            UpdateOptions::builder().upsert(true).build(),
        )
        .await
        .context(error::MongoDb)?;

    if with_mp4 {
        let mut zip_path_db_slash = PathBuf::from_slash(zip_path_db);
        zip_path_db_slash.set_extension("mp4");
        let mp4_path_db = zip_path_db_slash.to_slash_lossy();

        zip_path.set_extension("mp4");
        let mp4_path = zip_path;

        c_image
        .update_one(
            doc! {"local_path": &mp4_path_db},
            doc! {
                "$set": to_bson(&LocalMedia {
                    _id: None,
                    url: None,
                    local_path: mp4_path_db,
                    mime: Some("video/mp4".to_string()),
                    size: tokio::fs::metadata(&mp4_path).await?.len().try_into().unwrap_or_default(),
                    extension: None::<ImageMedia>
                }).context(error::BsonSerialize)?
            },
            UpdateOptions::builder().upsert(true).build(),
        )
        .await
        .context(error::MongoDb)?;
    }
    Ok(())
}

pub async fn save_illusts(
    illusts: &Vec<pixivcrab::models::illust::Illust>,
    api: &AppAPI,
    c_tag: &Collection<Document>,
    c_user: &Collection<Document>,
    c_illust: &Collection<Document>,
    users_need_update_set: &mut BTreeSet<String>,
    ugoira_map: &mut HashMap<String, (String, Vec<i32>)>,
) -> crate::Result<()> {
    let mut tags_set = HashSet::new();
    let mut users_map = BTreeMap::new();
    for i in illusts {
        if i.user.id != 0 {
            users_map.insert(i.user.id.to_string(), &i.user);
        }
        insert_tags_to_alias(&i.tags, &mut tags_set)
    }
    let tags_to_oid = update_tags(tags_set, c_tag).await?;
    let users_to_oid = update_users(users_map, users_need_update_set, c_user).await?;

    for i in illusts {
        let illust_id = i.id.to_string();
        if !i.visible {
            if i.id != 0 {
                set_item_invisible(c_illust, &illust_id).await?;
            }
            continue;
        }
        let tag_ids = i
            .tags
            .iter()
            .filter_map(|t| tags_to_oid.get(&t.name).map(|x| x.clone()))
            .collect();
        let illust = PixivIllust {
            parent_id: Some(
                users_to_oid
                    .get(&i.user.id.to_string())
                    .ok_or(error::MongoNotMatch.build())?
                    .to_owned(),
            ),
            tag_ids: tag_ids,
            source_inaccessible: false,
            last_modified: Some(DateTime::now()),
            extension: Some(pixiv::Works {
                is_bookmarked: i.is_bookmarked,
                total_bookmarks: i.total_bookmarks,
                total_view: i.total_view,
            }),
            ..Default::default()
        };

        c_illust
            .update_one(
                doc! {
                    "source_id": &illust_id,
                },
                doc! {
                    "$set": &to_bson(&illust).context(error::BsonSerialize)?
                },
                UpdateOptions::builder().upsert(true).build(),
            )
            .await
            .context(error::MongoDb)?;

        let mut history = History {
            last_modified: Some(DateTime::now()),
            extension: Some(pixiv::IllustHistory {
                caption_html: i.caption.clone(),
                illust_type: i.r#type.clone(),
                title: i.title.clone(),
                image_urls: {
                    let mut urls = Vec::new();
                    if i.page_count == 1 {
                        if let Some(ref url) = i.meta_single_page.original_image_url {
                            urls.push(url.clone());
                        }
                    } else {
                        for some_url in &i.meta_pages {
                            if let Some(ref url) = some_url.image_urls.original {
                                urls.push(url.clone());
                            }
                        }
                    }
                    urls
                },
                date: Some(DateTime::from_chrono(i.create_date)),
                ugoira_delay: None, // TODO: fetch ugoira info after all items are sent.
            }),
        };
        if i.r#type == "ugoira" {
            let ugoira = api
                .ugoira_metadata(&illust_id)
                .await
                .context(error::PixivApi)?;
            let delay: Vec<_> = ugoira
                .ugoira_metadata
                .frames
                .iter()
                .map(|frame| frame.delay)
                .collect();
            history.extension.as_mut().unwrap().ugoira_delay = Some(delay.clone());
            ugoira_map.insert(
                illust_id.clone(),
                (ugoira.ugoira_metadata.zip_urls.medium, delay),
            );
        }

        c_illust
                .update_one(
                    doc! {
                        "source_id": &illust_id,
                        "history.extension": {
                            "$ne": to_bson(history.extension.as_ref().unwrap()).context(error::BsonSerialize)?
                        }
                    },
                    doc! {"$push": {"history": to_bson(&history).context(error::BsonSerialize)?}},
                    None,
                )
                .await
                .context(error::MongoDb)?;
    }
    Ok(())
}

pub async fn save_novels(
    novels: Vec<pixivcrab::models::novel::Novel>,
    api: &AppAPI,
    c_user: &Collection<Document>,
    c_tag: &Collection<Document>,
    c_novel: &Collection<Document>,
    limit: Option<u32>,
    items_sent: &mut u32,
    update_exists: bool,
    users_need_update_set: &mut BTreeSet<String>,
) -> crate::Result<()> {
    let mut tags_set = HashSet::new();
    let mut users_map = BTreeMap::new();
    for n in &novels {
        if n.user.id != 0 {
            users_map.insert(n.user.id.to_string(), &n.user);
        }
        insert_tags_to_alias(&n.tags, &mut tags_set);
    }
    let tags_to_oid = update_tags(tags_set, &c_tag).await?;
    let users_to_oid = update_users(users_map, users_need_update_set, &c_user).await?;

    for n in novels {
        if let Some(limit) = limit {
            if *items_sent >= limit {
                return Ok(());
            }
        }
        *items_sent += 1;
        if !n.visible {
            if n.id != 0 {
                set_item_invisible(&c_novel, &n.id.to_string()).await?;
            }
            continue;
        }

        let tag_ids = n
            .tags
            .iter()
            .filter_map(|t| tags_to_oid.get(&t.name).map(|x| x.clone()))
            .collect();

        let novel_id = n.id.to_string();
        let novel = PixivNovel {
            last_modified: Some(DateTime::now()),
            parent_id: Some(users_to_oid[&n.user.id.to_string()]),
            tag_ids,
            extension: Some(pixiv::Works {
                is_bookmarked: n.is_bookmarked,
                total_bookmarks: n.total_bookmarks,
                total_view: n.total_view,
            }),
            ..Default::default()
        };

        let matched_count = c_novel
            .update_one(
                doc! {
                    "source_id": &novel_id,
                },
                doc! {
                    "$set": &to_bson(&novel).context(error::BsonSerialize)?
                },
                UpdateOptions::builder().upsert(true).build(),
            )
            .await
            .context(error::MongoDb)?
            .matched_count;
        if matched_count != 0 && !update_exists {
            continue;
        }

        info!("pixiv: getting novel text of {}", novel_id);
        let r = api.novel_text(&novel_id).await.context(error::PixivApi)?;

        let history = History {
            extension: Some(NovelHistory {
                caption_html: n.caption,
                cover_image_url: n.image_urls.large.clone().or(n.image_urls.medium),
                date: Some(DateTime::from_chrono(n.create_date)),
                image_urls: n.image_urls.large.map_or_else(|| vec![], |url| vec![url]),
                title: n.title,
                text: r.novel_text,
            }),
            last_modified: Some(DateTime::now()),
        };

        c_novel
                .update_one(
                    doc! {
                        "source_id": &novel_id,
                        "history.extension": {
                            "$ne": to_bson(history.extension.as_ref().unwrap()).context(error::BsonSerialize)?
                        }
                    },
                    doc! {"$push": {"history": to_bson(&history).context(error::BsonSerialize)?}},
                    None,
                )
                .await
                .context(error::MongoDb)?;
    }

    Ok(())
}

pub async fn create_indexes(db: &Database) -> crate::Result<()> {
    let c_illust = db.collection::<Document>("pixiv_illust");
    let c_image = db.collection::<Document>("pixiv_image");
    let c_novel = db.collection::<Document>("pixiv_novel");
    let c_tag = db.collection::<Document>("pixiv_tag");
    let c_user = db.collection::<Document>("pixiv_user");

    let item_indexes: Vec<_> = ["source_id", "tag_ids", "parent_id"]
        .into_iter()
        .map(|k| IndexModel::builder().keys(doc! { k: 1 }).build())
        .collect();

    for c in [c_illust, c_novel] {
        c.create_indexes(item_indexes.clone(), None)
            .await
            .context(error::MongoDb)?;
    }

    c_user
        .create_indexes(item_indexes[..2].to_vec(), None)
        .await
        .context(error::MongoDb)?;

    c_image
        .create_index(IndexModel::builder().keys(doc! { "url": 1 }).build(), None)
        .await
        .context(error::MongoDb)?;

    c_tag
        .create_index(
            IndexModel::builder().keys(doc! { "alias": 1 }).build(),
            None,
        )
        .await
        .context(error::MongoDb)?;

    Ok(())
}
