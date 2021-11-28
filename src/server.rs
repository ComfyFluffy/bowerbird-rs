use std::{
    collections::HashMap,
    path::PathBuf,
    str::FromStr,
    sync::{Arc, Mutex},
};

use bson::doc;
use error::ServerErrorExt;
use futures::TryStreamExt;
use image::ImageOutputFormat;
use mongodb::{
    options::{FindOneOptions, FindOptions},
    Database,
};
use reqwest::header::HeaderMap;
use rocket::{
    fs::FileServer,
    http::{ContentType, Status},
    response::Redirect,
    routes,
    serde::json::Json,
    State,
};
use serde_json::Value;
use snafu::ResultExt;
use tokio::task::spawn_blocking;

use crate::config::Config;

use self::error::ErrorResponse;

mod error;

type Result<T> = std::result::Result<T, ErrorResponse>;
// async fn media_by_url(db: &State<Database>, collection: &str) -> Redirect {
//     db.collection(collection).find_one(filter, options);
//     Redirect::temporary()
// }
struct PixivProxy(reqwest::Client);

#[derive(Debug)]
struct StrErr(&'static str);

impl std::fmt::Display for StrErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for StrErr {}

// #[rocket::get("/proxy?<url>")]
// async fn proxy_pixiv(url: &str, client: &State<PixivProxy>) -> Result<Vec<u8>> {
//     let url: Url = url.parse().with_status(Status::BadRequest)?;
//     if let Some(h) = url.host() {
//         let h = h.to_string();
//         if h == "i.pximg.net" || h == "s.pximg.net" {
//             let r = client
//                 .0
//                 .get(url)
//                 .send()
//                 .await
//                 .with_status(Status::BadGateway)?
//                 .bytes()
//                 .await
//                 .with_status(Status::BadGateway)?;
//             return Ok(r.to_vec());
//         }
//     }
//     Err(StrErr("unsupported host")).with_status(Status::BadRequest)
// }
//TODO: Proxy

#[derive(Debug, Hash, PartialEq, Eq, Clone)]
struct ThumbnailCacheKey {
    max_width: u32,
    max_height: u32,
    local_path: String,
}

#[rocket::post("/find?<collection>&<limit>", data = "<filter>")]
async fn find(
    filter: Json<serde_json::Map<String, Value>>,
    collection: &str,
    db: &State<Database>,
    limit: u32,
) -> Result<Json<Vec<bson::Document>>> {
    let doc: bson::Document = filter.0.try_into().with_status(Status::BadRequest)?;
    let mut cur = db
        .collection::<bson::Document>(collection)
        .find(
            doc,
            FindOptions::builder()
                .sort(doc! {"_id": -1})
                .limit(if limit > 500 { 500 } else { limit.into() })
                .build(),
        )
        .await
        .with_status(Status::BadRequest)?;
    let mut r = Vec::new();
    while let Some(i) = cur
        .try_next()
        .await
        .with_status(Status::InternalServerError)?
    {
        r.push(i);
    }

    Ok(Json(r))
}

#[rocket::get("/media-by-url?<url>&<size>")]
async fn find_pixiv_media_by_url(url: &str, db: &State<Database>, size: u32) -> Result<Redirect> {
    if let Some(r) = db
        .collection::<bson::Document>("pixiv_image")
        .find_one(
            doc! {"url": url},
            FindOneOptions::builder()
                .projection(doc! {"local_path": true})
                .build(),
        )
        .await
        .with_status(Status::InternalServerError)?
    {
        let p = r
            .get_str("local_path")
            .with_status(Status::InternalServerError)?;
        Ok(Redirect::temporary(format!("media/{}?size={}", p, size)))
    } else {
        Ok(Redirect::temporary(
            "proxy?url=".to_string() + &urlencoding::encode(url).clone(),
        ))
    }
}

#[rocket::get("/media-by-id?<id>")]
async fn find_pixiv_media_by_id(id: &str, db: &State<Database>) -> Result<Redirect> {
    if let Some(r) = db
        .collection::<bson::Document>("pixiv_image")
        .find_one(
            doc! { "_id": bson::oid::ObjectId::from_str(id).with_status(Status::BadRequest)? },
            FindOneOptions::builder()
                .projection(doc! {"local_path": true})
                .build(),
        )
        .await
        .with_status(Status::InternalServerError)?
    {
        let p = r
            .get_str("local_path")
            .with_status(Status::InternalServerError)?;
        Ok(Redirect::temporary("static/".to_string() + p))
    } else {
        Err(StrErr("not found in database")).with_status(Status::NotFound)
    }
}

type ThumbnailCache = HashMap<ThumbnailCacheKey, Vec<u8>>;

fn cached_image_thumbnail(
    local_path: String,
    max_width: u32,
    max_height: u32,
    cache: Arc<Mutex<ThumbnailCache>>,
) -> Result<Vec<u8>> {
    let mut cache_lock = cache.lock().unwrap();
    if cache_lock.len() > 200 {
        let k = cache_lock.keys().next().unwrap().clone();
        cache_lock.remove(&k);
    }
    if let Some(b) = cache_lock.get(&ThumbnailCacheKey {
        local_path: local_path.clone(),
        max_height,
        max_width,
    }) {
        Ok(b.clone())
    } else {
        drop(cache_lock);
        let img = image::io::Reader::open(&local_path)
            .with_status(Status::NotFound)?
            .decode()
            .with_status(Status::InternalServerError)?;
        let img = img.thumbnail(max_width, max_height);
        let mut b = Vec::with_capacity(1024 * 50);

        img.write_to(&mut b, ImageOutputFormat::Jpeg(85))
            .with_status(Status::InternalServerError)?;
        cache.lock().unwrap().insert(
            ThumbnailCacheKey {
                local_path,
                max_height,
                max_width,
            },
            b.clone(),
        );
        Ok(b)
    }
}

#[rocket::get("/media/<path..>?<size>")]
async fn pixiv_image(
    path: PathBuf,
    size: u32,
    config: &State<PixivConfig>,
    cache: &State<Arc<Mutex<ThumbnailCache>>>,
) -> Result<(ContentType, Vec<u8>)> {
    let path = config.storage_dir.join(path);
    let cache = cache.inner().clone();
    let b = spawn_blocking(move || {
        cached_image_thumbnail(path.to_string_lossy().to_string(), size, size, cache)
    })
    .await
    .unwrap()?;
    Ok((ContentType::JPEG, b))
}

#[derive(Debug, Clone)]
struct PixivConfig {
    storage_dir: PathBuf,
}

pub async fn run(db: Database, config: Config) -> crate::Result<()> {
    let pixiv_proxy = reqwest::ClientBuilder::new()
        .default_headers({
            let mut headers = HeaderMap::new();
            headers.insert("Referer", "https://app-api.pixiv.net/".parse().unwrap());
            headers
        })
        .build()
        .context(error::DownloadRequestBuild)?;
    let thumbnial_cache: Arc<Mutex<ThumbnailCache>> = Arc::new(Mutex::new(HashMap::new()));
    let pixiv_config = PixivConfig {
        storage_dir: config.sub_dir(&config.pixiv.storage_dir),
    };
    rocket::build()
        .mount("/api/bson", routes![find])
        .manage(db)
        .manage(PixivProxy(pixiv_proxy))
        .manage(thumbnial_cache)
        .mount(
            "/api/pixiv/static",
            FileServer::new(pixiv_config.storage_dir.clone(), rocket::fs::Options::None),
        )
        .manage(pixiv_config)
        .mount(
            "/api/pixiv",
            routes![find_pixiv_media_by_url, find_pixiv_media_by_id, pixiv_image],
        )
        .launch()
        .await
        .context(error::Rocket)
}
