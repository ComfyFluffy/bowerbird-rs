use std::{str::FromStr, sync::Mutex};

use futures::TryStreamExt;
use mongodb::{options::FindOneOptions, Database};
use serde::Deserialize;

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
use bson::{doc, from_document, oid::ObjectId, Document};
use tokio::sync::Semaphore;

use crate::model::{ImageMedia, LocalMedia};

use super::utils;

use super::{error::*, PixivConfig, Result};

#[derive(Debug, Clone, Deserialize)]
struct ThumbnailQuery {
    size: u32,
}
#[get("/thumbnail/{path:.*}")]
async fn thumbnail(
    req: HttpRequest,
    path: web::Path<(String,)>,
    query: web::Query<ThumbnailQuery>,
    config: Data<PixivConfig>,
    cache: Data<Mutex<utils::ThumbnailCache>>,
    semaphore: Data<Semaphore>,
) -> Result<HttpResponse> {
    if req.headers().get(header::RANGE).is_some() {
        return Ok(HttpResponse::NotImplemented().finish());
    }
    if query.size > 1024 {
        return Err(Error::with_msg(StatusCode::BAD_REQUEST, "size too large"));
    }
    let path = config
        .storage_dir
        .join(path.0.replace("../", "").replace("..\\", ""));

    let img =
        utils::cached_image_thumbnail(path, query.size, cache.as_ref(), semaphore.as_ref()).await?;

    Ok(HttpResponse::Ok()
        .content_type(ContentType::jpeg())
        .append_header(header::CacheControl(vec![CacheDirective::MaxAge(604800)]))
        .body(img))
}

async fn media_redirect(
    db: &Database,
    filter: Document,
    size: Option<u32>,
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
        let url = if let Some(size) = size {
            format!("thumbnail/{path}?size={size}")
        } else {
            format!("storage/{path}")
        };
        Ok(HttpResponse::Found()
            .append_header((header::LOCATION, url))
            .finish())
    } else {
        Err(Error::not_found())
    }
}

#[derive(Debug, Clone, Deserialize)]
struct MediaByIdQuery {
    id: String,
    size: Option<u32>,
}
#[get("/media-by-id")]
async fn media_by_id(
    query: web::Query<MediaByIdQuery>,
    db: Data<Database>,
) -> Result<HttpResponse> {
    media_redirect(
        db.as_ref(),
        doc! { "_id": bson::oid::ObjectId::from_str(&query.id).with_status(StatusCode::BAD_REQUEST)? },
        query.size,
    ).await
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
) -> Result<HttpResponse> {
    media_redirect(db.as_ref(), doc! { "url": &query.url }, query.size).await
}

#[derive(Debug, Clone, Deserialize)]
struct FindImageMediaForm {
    h_range: Option<(f32, f32)>,
    min_s: Option<f32>,
    min_v: Option<f32>,
    limit: Option<u32>,
}
#[post("/find/media/image")]
async fn find_image_media(
    db: Data<Database>,
    form: Json<FindImageMediaForm>,
) -> Result<Json<Vec<String>>> {
    #[derive(Debug, Clone, Deserialize)]
    struct AggreateResult {
        _ids: Vec<ObjectId>,
    }
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

    let mut cur = db
        .collection::<LocalMedia<ImageMedia>>("pixiv_image")
        .aggregate(
            vec![
                doc! { "$match": m },
                doc! { "$sort": {"_id": -1} },
                doc! { "$limit": form.limit.unwrap_or(300) },
                doc! { "$group": {"_id": null, "_ids": {"$push": "$_id"} } },
            ],
            None,
        )
        .await
        .with_interal()?;

    if let Some(r) = cur.try_next().await.with_interal()? {
        let ids: AggreateResult = from_document(r).with_interal()?;

        Ok(Json(ids._ids.into_iter().map(|id| id.to_hex()).collect()))
    } else {
        Ok(Json(vec![]))
    }
}
