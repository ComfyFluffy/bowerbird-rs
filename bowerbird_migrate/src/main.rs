mod model;
use bson::doc;
use futures::TryStreamExt;
use mongodb::Database;
use sqlx::{query, Pool, Postgres};
use std::{env::var, time::Instant};

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
    let pg = sqlx::PgPool::connect(&var("DATABASE_URL")?).await?;
    Ok((mongo, pg))
}

async fn images(mongo: &Database, pg: &Pool<Postgres>) -> anyhow::Result<()> {
    let collection = mongo.collection::<LocalMedia<ImageMedia>>("pixiv_image");
    let old: Vec<_> = collection.find(None, None).await?.try_collect().await?;

    let mut tr = pg.begin().await?;

    for img in old {
        let ext = img.extension.as_ref();
        let id: i32 = query!(
            "INSERT INTO pixiv_media (url, size, mime, local_path, width, height) VALUES ($1, $2, $3, $4, $5, $6) RETURNING id",
            img.url, img.size as i32, img.mime, img.local_path, ext.map(|x| x.width as i32), ext.map(|x| x.height as i32)
        ).fetch_one(&mut tr).await?.id;
        if let Some(ext) = img.extension {
            let mut q = QueryBuilder::new("INSERT INTO pixiv_media_color (media_id, h, s, v) ");
            q.push_values(ext.palette_hsv, |mut b, c| {
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
            "INSERT INTO pixiv_user (source_id, source_inaccessible, last_modified, is_followed, total_following, total_illust_series, total_illusts, total_manga, total_novel_series, total_novels, total_public_bookmarks) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11) RETURNING id",
            user.source_id.as_ref(),
            user.source_inaccessible,
            user.last_modified.map(|dt| dt.to_chrono().naive_utc()),
            ext.map(|v| v.is_followed),
            ext.map(|v| (v.total_following.unwrap_or_default() as i32)),
            ext.map(|v| (v.total_illust_series.unwrap_or_default() as i32)),
            ext.map(|v| (v.total_illusts.unwrap_or_default() as i32)),
            ext.map(|v| (v.total_manga.unwrap_or_default() as i32)),
            ext.map(|v| (v.total_novel_series.unwrap_or_default() as i32)),
            ext.map(|v| (v.total_novels.unwrap_or_default() as i32)),
            ext.map(|v| (v.total_public_bookmarks.unwrap_or_default()  as i32)),
        ).fetch_one(&mut tr).await?.id;

        let mut query_builder = QueryBuilder::new(
                "insert into pixiv_user_history (item_id, workspace_image_id, background_id, avatar_id, last_modified, birth, region,
                    gender, comment, twitter_account, web_page, workspace)",
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
                    v.birth.as_ref().map(|t| {
                        chrono::NaiveDate::from_ymd(
                            t[..4].parse().unwrap(),
                            t[5..7].parse().unwrap(),
                            t[8..10].parse().unwrap(),
                        )
                    })
                }))
                .push_bind(ext.map(|v| &v.region))
                .push_bind(ext.map(|v| &v.gender))
                .push_bind(ext.map(|v| &v.comment))
                .push_bind(ext.map(|v| &v.twitter_account))
                .push_bind(ext.map(|v| &v.web_page))
                .push_bind(ext.map(|v| serde_json::to_value(v.workspace.as_ref()).unwrap()));
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

        let id: i32 = query!(
            "INSERT INTO pixiv_illust (
            parent_id,
            source_id,
            source_inaccessible,
            last_modified,
            total_bookmarks,
            total_view,
            is_bookmarked,
            tag_ids
        ) VALUES (
            (SELECT id FROM pixiv_user WHERE source_id = $1),
            $2,
            $3,
            $4,
            $5,
            $6,
            $7,
            (SELECT array_agg(id) FROM pixiv_tag WHERE alias && $8::varchar[])
        ) RETURNING id",
            parent.as_ref().map(|v| v.source_id.as_ref().unwrap()),
            illust.source_id.as_ref(),
            illust.source_inaccessible,
            illust.last_modified.map(|dt| dt.to_chrono().naive_utc()),
            illust.extension.as_ref().map(|v| v.total_bookmarks as i32),
            illust.extension.as_ref().map(|v| v.total_view as i32),
            illust.extension.as_ref().map(|v| v.is_bookmarked),
            alias.as_slice()
        )
        .fetch_one(&mut tr)
        .await?
        .id;

        if !illust.history.is_empty() {
            let mut q = QueryBuilder::new(
                "INSERT INTO pixiv_illust_history (
                    item_id, 
                    illust_type,
                    caption_html,
                    title,
                    date,
                    media_ids
                ) ",
            );
            q.push_values(&illust.history, |mut b, h| {
                let ext = h.extension.as_ref();
                b.push_bind(id)
                    .push_bind(ext.map(|v| &v.illust_type))
                    .push_bind(ext.map(|v| &v.caption_html))
                    .push_bind(ext.map(|v| &v.title))
                    .push_bind(ext.map(|v| v.date.as_ref().map(|t| t.to_chrono().naive_utc())))
                    .push("(SELECT array_agg(id) FROM pixiv_media join unnest(")
                    .push_bind_unseparated(ext.map(|v| &v.image_urls))
                    .push_unseparated(") on unnest = pixiv_media.url)");
            });
            // println!("{}", q.sql());
            q.build().execute(&mut tr).await?;
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

        let id: i32 = query!(
            "INSERT INTO pixiv_novel (
            parent_id,
            source_id,
            source_inaccessible,
            last_modified,
            total_bookmarks,
            total_view,
            is_bookmarked,
            tag_ids
        ) VALUES (
            (SELECT id FROM pixiv_user WHERE source_id = $1),
            $2,
            $3,
            $4,
            $5,
            $6,
            $7,
            (SELECT array_agg(id) FROM pixiv_tag WHERE alias && $8::varchar[])
        ) RETURNING id",
            parent.as_ref().map(|v| v.source_id.as_ref().unwrap()),
            novel.source_id.as_ref(),
            novel.source_inaccessible,
            novel.last_modified.map(|dt| dt.to_chrono().naive_utc()),
            novel.extension.as_ref().map(|v| v.total_bookmarks as i32),
            novel.extension.as_ref().map(|v| v.total_view as i32),
            novel.extension.as_ref().map(|v| v.is_bookmarked),
            alias.as_slice()
        )
        .fetch_one(&mut tr)
        .await?
        .id;

        if !novel.history.is_empty() {
            let mut q = QueryBuilder::new(
                "INSERT INTO pixiv_novel_history (
                    item_id,
                    caption_html,
                    title,
                    date,
                    text
                ) ",
            );
            q.push_values(&novel.history, |mut b, h| {
                let ext = h.extension.as_ref();
                b.push_bind(id)
                    .push_bind(ext.map(|v| &v.caption_html))
                    .push_bind(ext.map(|v| &v.title))
                    .push_bind(ext.map(|v| v.date.as_ref().map(|t| t.to_chrono().naive_utc())))
                    .push_bind(ext.map(|v| &v.text));
            });
            // println!("{}", q.sql());
            q.build().execute(&mut tr).await?;
        }
    }
    tr.commit().await?;
    Ok(())
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    let (mongo, pg) = get_db().await?;
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
    Ok(())
}
