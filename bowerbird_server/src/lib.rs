use actix_files::Files;
use actix_web::{
    web::{self, Data},
    App, HttpServer,
};
use bowerbird_core::config::Config;
use log::info;
use sqlx::PgPool;
use std::{path::PathBuf, sync::Mutex};
use tokio::sync::Semaphore;

use utils::ThumbnailCache;

mod error;
mod pixiv;
mod utils;

type Result<T> = std::result::Result<T, error::Error>;

#[derive(Debug, Clone)]
struct PixivConfig {
    storage_dir: PathBuf,
}

pub async fn run(db: PgPool, config: Config) -> std::io::Result<()> {
    let thumbnail_cache = Data::new(Mutex::new(ThumbnailCache::new()));
    let pixiv_config = Data::new(PixivConfig {
        storage_dir: config.sub_dir(&config.pixiv.storage_dir),
    });
    let db = Data::new(db);

    let cpu_workers_sem = Data::new(Semaphore::new(num_cpus::get()));

    info!("server listening on http://{}", config.server.listen_addr);
    HttpServer::new({
        let config = Data::new(config.clone());
        move || {
            let scope_pixiv = web::scope("/pixiv")
                .service(Files::new("/storage", pixiv_config.storage_dir.clone()))
                .service(pixiv::thumbnail)
                .service(pixiv::find_illust)
                .service(pixiv::find_tag_search)
                .service(pixiv::find_image_media);

            let scope_v2 = web::scope("/api/v2").service(scope_pixiv);

            App::new()
                .app_data(db.clone())
                .app_data(thumbnail_cache.clone())
                .app_data(pixiv_config.clone())
                .app_data(cpu_workers_sem.clone())
                .app_data(config.clone())
                .service(scope_v2)
        }
    })
    .bind(config.server.listen_addr)?
    .run()
    .await
}
