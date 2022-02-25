mod error;
mod pixiv;
mod utils;
type Result<T> = std::result::Result<T, error::Error>;

use std::{path::PathBuf, sync::Mutex};

use actix_files::Files;
use actix_web::{
    web::{self, Data},
    App, HttpServer,
};
use mongodb::Database;
use snafu::ResultExt;
use tokio::sync::Semaphore;

use crate::config::Config;

use utils::ThumbnailCache;

#[derive(Debug, Clone)]
struct PixivConfig {
    storage_dir: PathBuf,
}

pub async fn run(db: Database, config: Config) -> crate::Result<()> {
    let thumbnail_cache = Data::new(Mutex::new(ThumbnailCache::new()));
    let pixiv_config = Data::new(PixivConfig {
        storage_dir: config.sub_dir(&config.pixiv.storage_dir),
    });
    let db = Data::new(db);

    let cpu_workers_sem = Data::new(Semaphore::new(num_cpus::get()));

    HttpServer::new(move || {
        let scope_pixiv = web::scope("/pixiv")
            .service(Files::new("/storage", pixiv_config.storage_dir.clone()))
            .service(pixiv::thumbnail)
            .service(pixiv::find_illust)
            .service(pixiv::find_tag)
            .service(pixiv::media_by_url)
            .service(pixiv::find_image_media);

        let scope_v1 = web::scope("/api/v1").service(scope_pixiv);

        App::new()
            .app_data(db.clone())
            .app_data(thumbnail_cache.clone())
            .app_data(pixiv_config.clone())
            .app_data(cpu_workers_sem.clone())
            .service(scope_v1)
    })
    .bind(("127.0.0.1", 5000))
    .context(crate::error::ServerIo)?
    .run()
    .await
    .context(crate::error::ServerIo)
}
