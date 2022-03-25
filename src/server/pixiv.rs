use std::sync::Mutex;

use actix_web::{
    get,
    http::{
        header::{self, CacheDirective, ContentType},
        StatusCode,
    },
    post,
    web::{self, Data, Json},
    HttpRequest, HttpResponse,
};
use bson::{doc, oid::ObjectId, to_document, Document};
use chrono::{DateTime, Utc};
use futures::TryStreamExt;
use indexmap::IndexMap;
use log::debug;
use mongodb::{
    options::{FindOneOptions, FindOptions},
    Database,
};
use serde::Deserialize;
use tokio::sync::Semaphore;

use super::{
    error::*,
    utils::{build_search_regex, cached_image_thumbnail, ThumbnailCache},
    PixivConfig, Result,
};
use crate::{
    config::Config,
    model::{
        pixiv::{PixivIllust, PixivUser},
        ImageMedia, LocalMedia, Tag,
    },
};

type SortBy = IndexMap<String, i32>;

fn parse_sort_by(sort_by: Option<SortBy>) -> Document {
    sort_by.map_or(doc! {"_id": -1}, |s| to_document(&s).unwrap())
}

fn sort_by_guard(sort_by: &Option<SortBy>) -> Result<()> {
    if let Some(sort_by) = sort_by {
        for (_, v) in sort_by {
            match *v {
                1 | -1 => {}
                _ => {
                    return Err(Error::with_msg(
                        StatusCode::BAD_REQUEST,
                        "sort_by value must be 1 or -1",
                    ))
                }
            }
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Deserialize)]
struct ThumbnailQuery {
    size: u32,
    crop_to_center: bool,
}
#[get("/thumbnail/{path:.*}")]
async fn thumbnail(
    req: HttpRequest,
    path: web::Path<(String,)>,
    query: web::Query<ThumbnailQuery>,
    config: Data<Config>,
    pixiv_config: Data<PixivConfig>,
    cache: Data<Mutex<ThumbnailCache>>,
    semaphore: Data<Semaphore>,
) -> Result<HttpResponse> {
    if req.headers().get(header::RANGE).is_some() {
        return Ok(HttpResponse::NotImplemented().finish());
    }
    let path = pixiv_config
        .storage_dir
        .join(path.0.replace("../", "").replace("..\\", ""));

    let img = cached_image_thumbnail(
        path,
        query.size,
        cache.as_ref(),
        semaphore.as_ref(),
        config.server.thumbnail_jpeg_quality,
        if query.crop_to_center {
            Some(0.75)
        } else {
            None
        },
    )
    .await?;

    Ok(HttpResponse::Ok()
        .content_type(ContentType::jpeg())
        .append_header(header::CacheControl(vec![CacheDirective::MaxAge(604800)]))
        .body(img))
}

async fn media_redirect(
    db: &Database,
    filter: Document,
    to_thumbnail: bool,
    query: &str,
) -> Result<HttpResponse> {
    if let Some(r) = db
        .collection::<Document>("pixiv_image")
        .find_one(
            filter,
            FindOneOptions::builder()
                .projection(doc! {"local_path": true})
                .build(),
        )
        .await
        .with_interal()?
    {
        let path = r.get_str("local_path").with_interal()?;
        let url = if to_thumbnail {
            format!("thumbnail/{path}?{query}")
        } else {
            format!("storage/{path}")
        };
        Ok(HttpResponse::TemporaryRedirect()
            .append_header((header::LOCATION, url))
            .finish())
    } else {
        Err(Error::not_found())
    }
}

#[derive(Debug, Clone, Deserialize)]
struct MediaByUrlQuery {
    url: String,
    size: Option<u32>,
}
#[get("/media-by-url")]
async fn media_by_url(
    query: web::Query<MediaByUrlQuery>,
    db: Data<Database>,
    req: HttpRequest,
) -> Result<HttpResponse> {
    media_redirect(
        db.as_ref(),
        doc! { "url": &query.url },
        query.size.is_some(),
        req.query_string(),
    )
    .await
}

#[derive(Debug, Clone, Deserialize)]
struct FindImageMediaForm {
    h_range: Option<(f32, f32)>,
    min_s: Option<f32>,
    min_v: Option<f32>,
}
#[post("/find/media/image")]
async fn find_image_media(
    db: Data<Database>,
    form: Json<FindImageMediaForm>,
) -> Result<Json<Vec<LocalMedia<ImageMedia>>>> {
    let mut m = Document::new();

    if let Some(h_range) = form.h_range {
        let h = if h_range.0 > h_range.1 {
            doc! { "$or": [
                { "extension.palette_hsv.0.h": {"$gte": h_range.0, "$lte": 360.0} },
                { "extension.palette_hsv.0.h": {"$gte": 0.0, "$lte": h_range.1} },
            ]}
        } else {
            doc! { "extension.palette_hsv.0.h": {"$gte": h_range.0, "$lte": h_range.1} }
        };
        m.extend(h);
        m.extend(doc! {
            "extension.palette_hsv.0.s": {"$gte": form.min_s.unwrap_or(0.2)},
            "extension.palette_hsv.0.v": {"$gte": form.min_v.unwrap_or(0.2)},
        })
    }

    let cur = db
        .collection("pixiv_image")
        .find(m, FindOptions::builder().sort(doc! {"_id": -1}).build())
        .await
        .with_interal()?;

    let rv = cur.try_collect().await.with_interal()?;
    Ok(Json(rv))
}

#[derive(Debug, Clone, Deserialize)]
struct FindUserForm {
    search: Option<String>,
    ids: Option<Vec<ObjectId>>,
    source_ids: Option<Vec<String>>,
    source_inaccessible: Option<bool>,
    tag_ids: Option<Vec<ObjectId>>,
    // limit: u32,
    sort_by: Option<SortBy>,
}
#[post("/find/user")]
async fn find_user(db: Data<Database>, form: Json<FindUserForm>) -> Result<Json<Vec<PixivUser>>> {
    let form = form.into_inner();
    sort_by_guard(&form.sort_by)?;
    let mut filter = doc! {};
    if let Some(search) = form.search {
        let reg = build_search_regex(&search);
        filter.extend(doc! {
            "$or": [
                { "history.extension.name": &reg },
                { "history.extension.comment": &reg },
            ]
        });
    }
    if let Some(ids) = form.ids {
        filter.extend(doc! { "_id": {"$in": ids} });
    }
    if let Some(source_ids) = form.source_ids {
        filter.extend(doc! { "source_id": {"$in": source_ids} });
    }
    if let Some(source_inaccessible) = form.source_inaccessible {
        filter.extend(doc! { "source_inaccessible": source_inaccessible });
    }
    if let Some(tag_ids) = form.tag_ids {
        filter.extend(doc! { "tag_ids": {"$in": tag_ids} });
    }

    let rv = db
        .collection("pixiv_user")
        .find(
            filter,
            FindOptions::builder()
                .sort(parse_sort_by(form.sort_by))
                // .limit(form.limit as i64)
                .build(),
        )
        .await
        .with_interal()?
        .try_collect()
        .await
        .with_interal()?;
    Ok(Json(rv))
}

#[derive(Debug, Clone, Deserialize)]
struct FindTagForm {
    search: Option<String>,
    ids: Option<Vec<ObjectId>>,
    limit: u32,
}
#[post("/find/tag")]
async fn find_tag(db: Data<Database>, form: Json<FindTagForm>) -> Result<Json<Vec<Tag>>> {
    let form = form.into_inner();
    let mut filter = doc! {};
    if let Some(search) = form.search {
        if !search.is_empty() {
            filter.extend(doc! {"alias": build_search_regex(&search)});
        }
    }
    if let Some(ids) = form.ids {
        filter.extend(doc! {"_id": {"$in": ids}});
    }

    let cur = db
        .collection::<Tag>("pixiv_tag")
        .find(
            filter,
            FindOptions::builder().limit(form.limit as i64).build(),
        )
        .await
        .with_interal()?;

    let rv = cur.try_collect().await.with_interal()?;
    Ok(Json(rv))
}

#[derive(Debug, Clone, Deserialize, Default)]
struct FindIllustForm {
    tags: Option<Vec<ObjectId>>,
    search: Option<String>, // Search in title and caption
    date_range: Option<(Option<DateTime<Utc>>, Option<DateTime<Utc>>)>,
    bookmarks_range: Option<(u32, u32)>,
    sort_by: Option<SortBy>,
    source_inaccessible: Option<bool>,
    parent_ids: Option<Vec<ObjectId>>,
}
#[post("/find/illust")]
async fn find_illust(
    db: Data<Database>,
    form: Json<FindIllustForm>,
) -> Result<Json<Vec<PixivIllust>>> {
    let form = form.into_inner();

    sort_by_guard(&form.sort_by)?;

    let mut filter = doc! {};

    if let Some(tag_ids) = form.tags {
        if !tag_ids.is_empty() {
            filter.extend(doc! { "tag_ids": {"$all": tag_ids} });
        }
    }

    if let Some(search) = form.search {
        if !search.is_empty() {
            let reg = build_search_regex(&search);
            filter.extend(doc! { "$or": [
                { "history.extension.title": &reg },
                { "history.extension.caption_html": &reg },
            ]});
        }
    }

    if let Some((start, end)) = form.date_range {
        let mut filter_date = Document::new();
        if let Some(start) = start {
            filter_date.insert("$gte", start);
        }
        if let Some(end) = end {
            filter_date.insert("$lte", end);
        }
        if filter_date.len() > 0 {
            filter.extend(doc! { "history.extension.date": filter_date });
        }
    }

    if let Some((min_bookmarks, max_bookmarks)) = form.bookmarks_range {
        if min_bookmarks != 0 || max_bookmarks != 0 {
            if max_bookmarks != 0 {
                filter.extend(doc! { "history.extension.bookmarks": {"$gte": min_bookmarks, "$lte": max_bookmarks} });
            } else {
                filter.extend(doc! { "history.extension.bookmarks": {"$gte": min_bookmarks} });
            }
        }
    }

    if let Some(source_inaccessible) = form.source_inaccessible {
        filter.extend(doc! {"source_inaccessible": source_inaccessible});
    }

    if let Some(parent_ids) = form.parent_ids {
        if !parent_ids.is_empty() {
            filter.extend(doc! { "parent_id": {"$in": parent_ids} });
        }
    }

    debug!("find illust: {:?} sort: {:?}", filter, form.sort_by);

    let options = FindOptions::builder()
        .sort(
            form.sort_by
                .map_or(doc! {"_id": -1}, |s| to_document(&s).unwrap()),
        )
        .build();

    let rv = db
        .collection("pixiv_illust")
        .find(filter, options)
        .await
        .with_interal()?
        .try_collect()
        .await
        .with_interal()?;
    Ok(Json(rv))
}
