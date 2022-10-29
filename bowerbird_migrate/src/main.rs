mod model;
use bson::doc;
use chrono::NaiveDate;
use futures::TryStreamExt;
use mongodb::Database;
use regex::Regex;
use sqlx::{query, Pool, Postgres};
use std::{collections::BTreeMap, env::var, time::Instant};

use crate::model::{
    pixiv::{PixivIllust, PixivNovel, PixivUser},
    ImageMedia, LocalMedia, Tag,
};

type QueryBuilder<'a> = sqlx::QueryBuilder<'a, Postgres>;

async fn get_db() -> anyhow::Result<(Database, Pool<Postgres>)> {
    let mongo = mongodb::Client::with_options(
        mongodb::options::ClientOptions::parse(var("MONGODB_URL")?).await?,
    )?
    .database("bowerbird_rust");
    let pg = sqlx::PgPool::connect(&var("TARGET_DATABASE_URL")?).await?;
    Ok((mongo, pg))
}

async fn images(mongo: &Database, pg: &Pool<Postgres>) -> anyhow::Result<()> {
    let collection = mongo.collection::<LocalMedia<ImageMedia>>("pixiv_image");
    let old: Vec<_> = collection.find(None, None).await?.try_collect().await?;

    let mut tr = pg.begin().await?;

    for img in old {
        let ext = img.extension.as_ref();

        if img.local_path.starts_with("avatar")
            || img.local_path.starts_with("background")
            || img.local_path.starts_with("workspace")
        {
            continue;
        }
        let id: i64 = query!(
                "INSERT INTO pixiv_media (url, size, mime, local_path, width, height, inserted_at) VALUES ($1, $2, $3, $4, $5, $6, $7) RETURNING id",
                img.url, img.size as i32, img.mime, img.local_path, ext.map(|x| x.width as i32), ext.map(|x| x.height as i32), img._id.map(|o| o.timestamp().to_chrono())
            ).fetch_one(&mut tr).await?.id;
        if let Some(ref ext) = img.extension {
            let mut q = QueryBuilder::new("INSERT INTO pixiv_media_color (media_id, h, s, v) ");
            q.push_values(&ext.palette_hsv, |mut b, c| {
                b.push_bind(id).push_bind(c.h).push_bind(c.s).push_bind(c.v);
            });
            q.build().execute(&mut tr).await?;
        }
    }

    tr.commit().await?;
    Ok(())
}
async fn users(mongo: &Database, pg: &Pool<Postgres>) -> anyhow::Result<()> {
    let c_user = mongo.collection::<PixivUser>("pixiv_user");
    let old: Vec<_> = c_user.find(None, None).await?.try_collect().await?;

    let mut tr = pg.begin().await?;
    for user in old {
        let ext = user.extension.as_ref();
        let id = query!(
            "INSERT INTO pixiv_user (source_id, source_inaccessible, updated_at, is_followed, total_following, total_illust_series, total_illusts, total_manga, total_novel_series, total_novels, total_public_bookmarks, inserted_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) RETURNING id",
            user.source_id.as_ref(),
            user.source_inaccessible,
            user.last_modified.map(|dt| dt.to_chrono()),
            ext.map(|v| v.is_followed),
            ext.map(|v| (v.total_following.unwrap_or_default() as i32)),
            ext.map(|v| (v.total_illust_series.unwrap_or_default() as i32)),
            ext.map(|v| (v.total_illusts.unwrap_or_default() as i32)),
            ext.map(|v| (v.total_manga.unwrap_or_default() as i32)),
            ext.map(|v| (v.total_novel_series.unwrap_or_default() as i32)),
            ext.map(|v| (v.total_novels.unwrap_or_default() as i32)),
            ext.map(|v| (v.total_public_bookmarks.unwrap_or_default() as i32)),
            user._id.map(|o| o.timestamp().to_chrono())
        ).fetch_one(&mut tr).await?.id;

        let mut query_builder = QueryBuilder::new(
            "insert into pixiv_user_history (item_id, workspace_image_id, background_id, avatar_id, inserted_at, birth, region,
                gender, comment, twitter_account, web_page, workspace, name, account, is_premium)",
        );
        query_builder.push_values(&user.history, |mut b, h| {
            let ext = h.extension.as_ref();
            let select_id = "(SELECT id FROM pixiv_media where url = ";
            b.push_bind(id)
                .push(select_id)
                .push_bind_unseparated(ext.map(|v| &v.workspace_image_url))
                .push_unseparated(")")
                .push(select_id)
                .push_bind_unseparated(ext.map(|v| &v.background_url))
                .push_unseparated(")")
                .push(select_id)
                .push_bind_unseparated(ext.map(|v| &v.avatar_url))
                .push_unseparated(")")
                .push_bind(h.last_modified.map(|dt| dt.to_chrono()))
                // 2001-04-03
                .push_bind(ext.map(|v| {
                    v.birth
                        .as_ref()
                        .and_then(|t| NaiveDate::parse_from_str(t, "%Y-%m-%d").ok())
                }))
                .push_bind(ext.map(|v| &v.region))
                .push_bind(ext.map(|v| &v.gender))
                .push_bind(ext.map(|v| &v.comment))
                .push_bind(ext.map(|v| &v.twitter_account))
                .push_bind(ext.map(|v| &v.web_page))
                .push_bind(ext.map(|v| {
                    serde_json::to_value(v.workspace.as_ref().unwrap_or(&BTreeMap::new())).unwrap()
                }))
                .push_bind(ext.map(|v| &v.name))
                .push_bind(ext.map(|v| &v.account))
                .push_bind(ext.map(|v| v.is_premium));
        });
        let query = query_builder.build();
        query.execute(&mut tr).await?;
    }
    tr.commit().await?;
    Ok(())
}

async fn tags(mongo: &Database, pg: &Pool<Postgres>) -> anyhow::Result<()> {
    let collection = mongo.collection::<Tag>("pixiv_tag");
    let old: Vec<_> = collection.find(None, None).await?.try_collect().await?;

    let mut tr = pg.begin().await?;

    let mut q = QueryBuilder::new("INSERT INTO pixiv_tag (alias) ");
    q.push_values(old, |mut b, tag| {
        b.push_bind(tag.alias);
    });
    q.build().execute(&mut tr).await?;

    tr.commit().await?;
    Ok(())
}

async fn illust(mongo: &Database, pg: &Pool<Postgres>) -> anyhow::Result<()> {
    let collection = mongo.collection::<PixivIllust>("pixiv_illust");
    let c_user = mongo.collection::<PixivUser>("pixiv_user");
    let c_tag = mongo.collection::<Tag>("pixiv_tag");
    let old: Vec<_> = collection.find(None, None).await?.try_collect().await?;

    let mut tr = pg.begin().await?;

    for illust in old {
        let parent = c_user
            .find_one(
                doc! {
                    "_id": illust.parent_id
                },
                None,
            )
            .await?;

        let alias = if !illust.tag_ids.is_empty() {
            let tags: Vec<_> = c_tag
                .find(
                    doc! {
                        "_id": {
                            "$in": &illust.tag_ids
                        }
                    },
                    None,
                )
                .await?
                .try_collect()
                .await?;
            tags.into_iter().flat_map(|tag| tag.alias).collect()
        } else {
            vec![]
        };

        let id = query!(
            "INSERT INTO pixiv_illust (
            parent_id,
            source_id,
            source_inaccessible,
            updated_at,
            total_bookmarks,
            total_view,
            is_bookmarked,
            tag_ids,
            inserted_at
        ) VALUES (
            (SELECT id FROM pixiv_user WHERE source_id = $1),
            $2,
            $3,
            $4,
            $5,
            $6,
            $7,
            (SELECT array_agg(id) FROM pixiv_tag WHERE alias && $8::varchar[]),
            $9
        ) RETURNING id",
            parent.as_ref().map(|v| v.source_id.as_ref().unwrap()),
            illust.source_id.as_ref(),
            illust.source_inaccessible,
            illust.last_modified.map(|dt| dt.to_chrono()),
            illust.extension.as_ref().map(|v| v.total_bookmarks as i32),
            illust.extension.as_ref().map(|v| v.total_view as i32),
            illust.extension.as_ref().map(|v| v.is_bookmarked),
            alias.as_slice(),
            illust._id.map(|o| o.timestamp().to_chrono()),
        )
        .fetch_one(&mut tr)
        .await?
        .id;

        for h in &illust.history {
            let ext = h.extension.as_ref();
            let history_id = query!(
                "
                    INSERT INTO pixiv_illust_history (
                        item_id, 
                        illust_type,
                        caption_html,
                        title,
                        date,
                        ugoira_frame_duration,
                        inserted_at
                    ) VALUES (
                        $1,
                        $2,
                        $3,
                        $4,
                        $5,
                        $6,
                        $7
                    ) RETURNING id
                    ",
                id,
                ext.map(|v| &v.illust_type),
                ext.map(|v| &v.caption_html),
                ext.map(|v| &v.title),
                ext.and_then(|v| v.date).map(|t| t.to_chrono()),
                ext.and_then(|v| v.ugoira_delay.as_ref())
                    .map(|v| v.as_slice()),
                h.last_modified.map(|dt| dt.to_chrono()),
            )
            .fetch_one(&mut tr)
            .await?
            .id;

            if let Some(ext) = ext {
                query!(
                    "
                    insert into pixiv_illust_history_media (history_id, media_id)
                    select $1, id
                    from pixiv_media
                        join unnest($2::varchar[]) url using (url)
                    ",
                    history_id,
                    &ext.image_urls
                )
                .execute(&mut tr)
                .await?;
            }
        }
    }

    tr.commit().await?;
    Ok(())
}

async fn novel(mongo: &Database, pg: &Pool<Postgres>) -> anyhow::Result<()> {
    let collection = mongo.collection::<PixivNovel>("pixiv_novel");
    let c_user = mongo.collection::<PixivUser>("pixiv_user");
    let c_tag = mongo.collection::<Tag>("pixiv_tag");
    let old: Vec<_> = collection.find(None, None).await?.try_collect().await?;

    let mut tr = pg.begin().await?;

    for novel in old {
        let parent = c_user
            .find_one(
                doc! {
                    "_id": novel.parent_id
                },
                None,
            )
            .await?;

        let alias = if !novel.tag_ids.is_empty() {
            let tags: Vec<_> = c_tag
                .find(
                    doc! {
                        "_id": {
                            "$in": &novel.tag_ids
                        }
                    },
                    None,
                )
                .await?
                .try_collect()
                .await?;
            tags.into_iter().flat_map(|tag| tag.alias).collect()
        } else {
            vec![]
        };

        let id = query!(
            "INSERT INTO pixiv_novel (
            parent_id,
            source_id,
            source_inaccessible,
            updated_at,
            total_bookmarks,
            total_view,
            is_bookmarked,
            tag_ids,
            inserted_at
        ) VALUES (
            (SELECT id FROM pixiv_user WHERE source_id = $1),
            $2,
            $3,
            $4,
            $5,
            $6,
            $7,
            (SELECT array_agg(id) FROM pixiv_tag WHERE alias && $8::varchar[]),
            $9
        ) RETURNING id",
            parent.as_ref().map(|v| v.source_id.as_ref().unwrap()),
            novel.source_id.as_ref(),
            novel.source_inaccessible,
            novel.last_modified.map(|dt| dt.to_chrono()),
            novel.extension.as_ref().map(|v| v.total_bookmarks as i32),
            novel.extension.as_ref().map(|v| v.total_view as i32),
            novel.extension.as_ref().map(|v| v.is_bookmarked),
            alias.as_slice(),
            novel._id.map(|o| o.timestamp().to_chrono()),
        )
        .fetch_one(&mut tr)
        .await?
        .id;

        if !novel.history.is_empty() {
            let mut q = QueryBuilder::new(
                "
                INSERT INTO pixiv_novel_history (
                    item_id,
                    caption_html,
                    title,
                    date,
                    text,
                    inserted_at
                ) ",
            );
            q.push_values(&novel.history, |mut b, h| {
                let ext = h.extension.as_ref();
                b.push_bind(id)
                    .push_bind(ext.map(|v| &v.caption_html))
                    .push_bind(ext.map(|v| &v.title))
                    .push_bind(ext.map(|v| v.date.as_ref().map(|t| t.to_chrono())))
                    .push_bind(ext.map(|v| &v.text))
                    .push_bind(h.last_modified.map(|dt| dt.to_chrono()));
            });
            // println!("{}", q.sql());
            q.build().execute(&mut tr).await?;
        }
    }
    tr.commit().await?;
    Ok(())
}

async fn test_image_order(pg: &Pool<Postgres>) -> anyhow::Result<()> {
    let re = Regex::new(r"p(\d+)").unwrap();
    let r = query!(
        "
        select id, image_paths from pixiv_illust_latest
        "
    )
    .fetch_all(pg)
    .await?;
    for row in r {
        // Map the paths to their order and check if they are in order
        println!("{:?}", row);
        if let Some(paths) = &row.image_paths {
            let pages: Vec<_> = paths
                .iter()
                .filter_map(|path| {
                    let cap = re.captures(path)?;
                    Some(cap.get(1).unwrap().as_str().parse::<i32>().unwrap())
                })
                .collect();
            let mut pages_sorted = pages.clone();
            pages_sorted.sort();

            assert!(pages == pages_sorted, "{:?}", row);
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let (mongo, pg) = get_db().await?;
    bowerbird_core::migrate(&pg).await?;
    let t = Instant::now();
    images(&mongo, &pg).await?;
    println!("images: {:?}", t.elapsed());
    users(&mongo, &pg).await?;
    println!("users: {:?}", t.elapsed());
    tags(&mongo, &pg).await?;
    println!("tags: {:?}", t.elapsed());
    illust(&mongo, &pg).await?;
    println!("illust: {:?}", t.elapsed());
    novel(&mongo, &pg).await?;
    println!("novel: {:?}", t.elapsed());
    test_image_order(&pg).await?;
    Ok(())
}
