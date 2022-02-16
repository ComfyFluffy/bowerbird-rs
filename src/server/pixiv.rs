use std::{str::FromStr, sync::Mutex};

use mongodb::{options::FindOneOptions, Database};
use serde::Deserialize;

use actix_web::{
    get,
    http::{
        header::{self, CacheDirective, ContentType},
        StatusCode,
    },
    web::{self, Data},
    HttpRequest, HttpResponse,
};
use bson::{doc, Document};
use tokio::sync::Semaphore;

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
    find_by: Document,
    size: Option<u32>,
) -> Result<HttpResponse> {
    if let Some(r) = db
        .collection::<Document>("pixiv_image")
        .find_one(
            find_by,
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
        Ok(HttpResponse::TemporaryRedirect()
            .append_header((header::LOCATION, url))
            .finish())
    } else {
        Err(Error::with_msg(
            StatusCode::NOT_FOUND,
            "not found in database",
        ))
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
