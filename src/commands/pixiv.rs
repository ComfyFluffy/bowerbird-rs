use chrono::{Duration, Utc};
use futures::FutureExt;
use image::GenericImageView;
use pixivcrab::AppAPI;
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    path::PathBuf,
};

use lazy_static::lazy_static;
use reqwest::{Method, Url};
use snafu::ResultExt;

use crate::{
    downloader::{Task, TaskHooks, TaskOptions},
    error, info,
    models::{
        self,
        pixiv::{PixivIllustHistory, PixivUser, PixivUserHistory, PixivWorks},
        History, ImageMedia, Item, LocalMedia,
    },
    warn, Result,
};

use mongodb::{
    bson::{doc, oid::ObjectId, to_bson, DateTime, Document},
    options::{self, FindOneAndUpdateOptions, UpdateOptions},
    Collection, Database,
};

use path_slash::PathBufExt;

use regex::Regex;

lazy_static! {
    /// Match the pximg URL.
    ///
    /// # Example
    ///
    /// Matching the URL
    /// `https://i.pximg.net/img-original/img/2021/08/22/22/03/33/92187206_p0.jpg`
    ///
    /// Groups:
    ///
    /// __0__ `/2021/08/22/22/03/33/92187206_p0.jpg`
    ///
    /// __1__ `2021/08/22/22/03/33`
    ///
    /// __2__ `92187206_p0.jpg`
    ///
    /// __3__ `92187206_p0`
    ///
    /// __4__ `jpg`
    static ref RE_ILLUST_URL: Regex =
        Regex::new(r"/(\d{4}/\d{2}/\d{2}/\d{2}/\d{2}/\d{2})/((.*)\.(.*))$").unwrap();
}

macro_rules! try_skip {
    ($res:expr) => {
        match $res {
            Ok(val) => val,
            Err(e) => {
                warn!("{}", e);
                continue;
            }
        }
    };
}

type PUser = Item<PixivUser, PixivUserHistory>;
type PIllust = Item<PixivWorks, PixivIllustHistory>;
// type PNovel = Item<PixivWorks, PixivNovelHistory>;

fn task_from_illust(
    api: &AppAPI,
    c_image: Collection<Document>,
    raw_url: Option<String>,
    parent_dir: &PathBuf,
    user_id: &str,
    illust_id: &str,
    is_multi_page: bool,
) -> crate::Result<Task> {
    let url = match raw_url {
        Some(raw_url) => match raw_url.parse::<Url>() {
            Ok(url) => url,
            Err(e) => {
                return Err(e).context(error::PixivParseURL);
            }
        },
        None => return error::PixivParse.fail(),
    };

    let captures = RE_ILLUST_URL
        .captures(url.path())
        .ok_or(error::PixivParse.build())?;
    let date = captures.get(1).unwrap().as_str().replace("/", "");

    let request_builder = {
        let url_clone = url.clone();
        let hash_secret = api.hash_secret.clone();
        move |client: &reqwest::Client| {
            client
                .request(Method::GET, url_clone.clone())
                .headers(pixivcrab::default_headers(&hash_secret))
                .header("Referer", "https://app-api.pixiv.net/")
                .build()
                .context(error::DownloadRequestBuild)
        }
    };
    let path_slash = if is_multi_page {
        format!(
            "{}/{}_{}/{}",
            user_id,
            illust_id,
            date,
            captures.get(2).unwrap().as_str()
        )
    } else {
        format!(
            "{}/{}_{}.{}",
            user_id,
            captures.get(3).unwrap().as_str(), // filename with page id
            date,
            captures.get(4).unwrap().as_str(), // extension
        )
    };
    let path = parent_dir.join(PathBuf::from_slash(&path_slash));
    let on_success_hook = {
        let url_clone = url.clone();
        |t: &Task| {
            let path = t.options.path.clone().unwrap();
            let size = t.file_size.unwrap() as i64;

            async move {
                let buffer = tokio::fs::read(&path).await.context(error::DownloadIO)?;
                let img = image::load_from_memory(&buffer).context(error::Image)?;

                let (w, h) = img.dimensions();
                let p = img.thumbnail(512, 512).to_rgba8();
                let rgb_v =
                    color_thief::get_palette(p.as_raw(), color_thief::ColorFormat::Rgba, 5, 5)
                        .context(error::ImageColorThief)?
                        .into_iter()
                        .map(|c| models::RGB(c.r.into(), c.g.into(), c.b.into()))
                        .collect();
                c_image
                    .update_one(
                        doc! {"_id": url_clone.as_str()},
                        doc! {
                            "$set": to_bson(&LocalMedia {
                                _id: Some(url_clone.to_string()),
                                local_path: path_slash,
                                mime: mime_guess::from_path(&path).first().map(|x| x.to_string()),
                                size,
                                extension: Some(ImageMedia {
                                    height: h as i32,
                                    width: w as i32,
                                    palette_rgb: rgb_v,
                                })
                            }).context(error::MongoBsonConvert)?
                        },
                        UpdateOptions::builder().upsert(true).build(),
                    )
                    .await
                    .context(error::MongoDB)?;
                Ok(())
            }
            .boxed()
        }
    };

    Ok(Task::new(
        Box::new(request_builder),
        url.clone(),
        TaskOptions {
            path: Some(path),
            ..Default::default()
        },
        Some(TaskHooks {
            on_error: None,
            on_success: Some(Box::new(on_success_hook)),
        }),
    ))
}

async fn illusts<'a>(
    db: &Database,
    api: &AppAPI,
    downloader: &crate::downloader::Downloader,
    mut pager: pixivcrab::Pager<'a, pixivcrab::models::illust::Response>,
    parent_dir: &PathBuf,
    limit: Option<u32>,
) -> Result<()> {
    let c_illust = db.collection::<Document>("pixiv_illust");
    let c_user = db.collection::<Document>("pixiv_user");
    let c_tag = db.collection::<Document>("pixiv_tag");

    let mut users_need_update_set = BTreeSet::new();

    'wh: while let Some(r) = pager.next().await.context(error::PixivAPI)? {
        let mut ugoiras = Vec::new();
        let mut tags_set = HashSet::new();
        let mut users = BTreeMap::new();
        for i in &r.illusts {
            if i.user.id != 0 {
                let user_id = i.user.id.to_string();
                users.insert(user_id, &i.user);
            }

            for t in &i.tags {
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

        let mut tags_to_oid = HashMap::new();

        for alias in tags_set {
            let regs: Vec<bson::Regex> = alias
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
                .context(error::MongoDB)?
                .ok_or(error::MongoNotMatch.build())?;
            for t in alias {
                let oid = r.get_object_id("_id").context(error::MongoValueAccess)?;
                tags_to_oid.insert(t, oid);
            }
        }

        let mut users_to_oid = HashMap::new();
        for (user_id, user) in users {
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
                .context(error::MongoDB)?
                .ok_or(error::MongoNotMatch.build())?;
            let parent_id = r.get_object_id("_id").context(error::MongoValueAccess)?;
            users_to_oid.insert(user_id.clone(), parent_id);

            if let Ok(last_modified) = r.get_datetime("last_modified") {
                if last_modified.to_chrono() <= Utc::now() - Duration::weeks(1) {
                    users_need_update_set.insert(user_id);
                    continue;
                }
            } else {
                users_need_update_set.insert(user_id);
                continue;
            }
            if let Ok(histories) = r.get_array("history") {
                if let Some(h) = histories.last() {
                    let s = h
                        .as_document()
                        .ok_or(error::MongoNotMatch.build())?
                        .get_document("extension")
                        .context(error::MongoValueAccess)?
                        .get_str("avatar_url")
                        .context(error::MongoValueAccess)?;
                    if s != user.profile_image_urls.medium {
                        users_need_update_set.insert(user_id);
                    }
                } else {
                    users_need_update_set.insert(user_id);
                }
            } else {
                users_need_update_set.insert(user_id);
            }
        }

        for i in &r.illusts {
            let illust_id = i.id.to_string();
            if !i.visible {
                if i.id != 0 {
                    c_illust
                        .update_one(
                            doc! {
                                "source_id": &illust_id
                            },
                            doc! {
                                "$set": { "source_inaccessible": true }
                            },
                            UpdateOptions::builder().upsert(true).build(),
                        )
                        .await
                        .context(error::MongoDB)?;
                }
                continue;
            }
            if i.r#type == "ugoira" {
                ugoiras.push(illust_id.clone());
            }
            let mut tag_ids: Vec<ObjectId> = Vec::new();
            for t in &i.tags {
                if !t.name.is_empty() {
                    tag_ids.push(
                        tags_to_oid
                            .get(&t.name)
                            .ok_or(error::MongoNotMatch.build())?
                            .clone(),
                    );
                }
            }

            c_illust
                .update_one(
                    doc! {
                        "source_id": &illust_id,
                    },
                    doc! {
                        "$set": &to_bson(&PIllust {
                            parent_id: Some(
                                users_to_oid
                                    .get(&i.user.id.to_string())
                                    .ok_or(error::MongoNotMatch.build())?
                                    .to_owned(),
                            ),
                            tag_ids: Some(tag_ids),
                            source_inaccessible: false,
                            last_modified: Some(DateTime::now()),
                            extension: Some(PixivWorks {
                                is_bookmarked: i.is_bookmarked,
                                total_bookmarks: i.total_bookmarks,
                                total_view: i.total_view,
                            }),
                            ..Default::default()
                        }).context(error::MongoBsonConvert)?
                    },
                    UpdateOptions::builder().upsert(true).build(),
                )
                .await
                .context(error::MongoDB)?;

            let history = History {
                last_modified: Some(DateTime::now()),
                extension: Some(PixivIllustHistory {
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
            c_illust
                .update_one(
                    doc! {
                        "source_id": &illust_id,
                        "history.extension": {
                            "$ne": to_bson(history.extension.as_ref().unwrap()).context(error::MongoBsonConvert)?
                        }
                    },
                    doc! {"$push": {"history": to_bson(&history).context(error::MongoBsonConvert)?}},
                    None,
                )
                .await
                .context(error::MongoDB)?;
        }

        let mut tasks = Vec::new();

        let c_image = db.collection::<Document>("pixiv_image");

        let mut items_sent = 0;
        for i in r.illusts {
            let illust_id = i.id.to_string();

            if i.page_count == 1 {
                let task = try_skip!(task_from_illust(
                    &api,
                    c_image.clone(),
                    i.meta_single_page.original_image_url,
                    parent_dir,
                    &i.user.id.to_string(),
                    &illust_id,
                    false,
                ));
                tasks.push(task);
            } else {
                for img in i.meta_pages {
                    let task = try_skip!(task_from_illust(
                        &api,
                        c_image.clone(),
                        img.image_urls.original,
                        parent_dir,
                        &i.user.id.to_string(),
                        &illust_id,
                        true,
                    ));
                    tasks.push(task);
                }
            }

            items_sent += 1;

            if let Some(limit) = limit {
                if items_sent >= limit {
                    downloader.send(tasks).await;
                    break 'wh;
                }
            }
        }
        downloader.send(tasks).await;
    }

    for user_id in users_need_update_set {
        update_user_detail(api, &user_id, &c_user).await?;
    }

    Ok(())
}

pub async fn update_user_detail(
    api: &AppAPI,
    user_id: &str,
    c_user: &Collection<Document>,
) -> crate::Result<()> {
    info!("Pixiv database: Updating user {}", &user_id);
    let resp = api.user_detail(&user_id).await.context(error::PixivAPI)?;
    let user = PUser {
        last_modified: Some(DateTime::now()),
        extension: Some(PixivUser {
            is_followed: resp.user.is_followed,
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
            doc! { "$set": &to_bson(&user).context(error::MongoBsonConvert)? },
            UpdateOptions::builder().upsert(true).build(),
        )
        .await
        .context(error::MongoDB)?;

    fn none_if_empty(s: String) -> Option<String> {
        match s.as_str() {
            "" => None,
            _ => Some(s),
        }
    }

    let history = History {
        last_modified: Some(DateTime::now()),
        extension: Some(PixivUserHistory {
            account: resp.user.account,
            name: resp.user.name,
            avatar_url: Some(resp.user.profile_image_urls.medium),
            gender: none_if_empty(resp.profile.gender),
            background_url: resp.profile.background_image_url.filter(|s| !s.is_empty()),
            birth: none_if_empty(resp.profile.birth),
            comment: resp.user.comment.filter(|s| !s.is_empty()),
            is_premium: resp.profile.is_premium,
            region: resp.profile.region.filter(|s| !s.is_empty()),
            twitter_account: resp.profile.twitter_account.filter(|s| !s.is_empty()),
            web_page: resp.profile.webpage.filter(|s| !s.is_empty()),
            workspace_image_url: resp
                .workspace
                .get("workspace_image_url")
                .unwrap_or(&None)
                .clone(),
            workspace: {
                let mut workspace = BTreeMap::new();
                for (k, v) in resp.workspace {
                    if k.as_str() == "workspace_image_url" {
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
                    "$ne": to_bson(history.extension.as_ref().unwrap()).context(error::MongoBsonConvert)?
                }
            },
            doc! { "$push": { "history": to_bson(&history).context(error::MongoBsonConvert)? } },
            None,
        )
        .await
        .context(error::MongoDB)?;
    Ok(())
}

pub async fn illust_uploads(
    api: &pixivcrab::AppAPI,
    db: &mongodb::Database,
    downloader: &crate::downloader::Downloader,
    parent_dir: PathBuf,
    user_id: Option<i32>,
    limit: Option<u32>,
) -> Result<()> {
    let auth_result = api.auth().await.context(error::PixivAPI)?;
    let user_id = user_id.map_or(auth_result.user.id, |i| i.to_string());
    let pager = api.illust_uploads(&user_id);

    illusts(db, api, downloader, pager, &parent_dir, limit).await
}

pub async fn illust_bookmarks(
    api: &pixivcrab::AppAPI,
    db: &mongodb::Database,
    downloader: &crate::downloader::Downloader,
    parent_dir: PathBuf,
    user_id: Option<i32>,
    private: bool,
    limit: Option<u32>,
) -> Result<()> {
    let auth_result = api.auth().await.context(error::PixivAPI)?;
    let user_id = user_id.map_or(auth_result.user.id, |i| i.to_string());
    let pager = api.illust_bookmarks(&user_id, private);

    illusts(db, api, downloader, pager, &parent_dir, limit).await
}
