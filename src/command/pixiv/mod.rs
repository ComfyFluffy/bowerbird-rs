use log::info;
use mongodb::{bson::Document, Database};
use pixivcrab::AppApi;
use std::{
    collections::{BTreeSet, HashMap},
    path::PathBuf,
};

use crate::downloader::Aria2Downloader;

pub mod database;
mod download;
mod utils;

fn limit_reached<T>(limit: Option<T>, items_sent: T) -> bool
where
    T: std::cmp::PartialOrd,
{
    if let Some(limit) = limit {
        items_sent >= limit
    } else {
        false
    }
}

#[derive(Debug, Clone)]
pub struct TaskConfig {
    pub ffmpeg_path: Option<PathBuf>,
    pub proxy: Option<String>,
    pub parent_dir: PathBuf,
}

async fn illusts(
    db: &Database,
    api: &AppApi,
    downloader: &Aria2Downloader,
    mut pager: pixivcrab::Pager<pixivcrab::models::illust::Response>,
    limit: Option<u32>,
    task_config: &TaskConfig,
) -> crate::Result<()> {
    let c_illust = db.collection::<Document>("pixiv_illust");
    let c_user = db.collection::<Document>("pixiv_user");
    let c_tag = db.collection::<Document>("pixiv_tag");
    let c_image = db.collection::<Document>("pixiv_image");

    let mut users_need_update_set = BTreeSet::new();
    let mut ugoira_map = HashMap::new();

    let mut items_sent = 0;
    while let Some(r) = {
        info!("getting illusts with offset: {}", items_sent);
        utils::retry_pager(&mut pager, 3).await?
    } {
        database::save_illusts(
            &r.illusts,
            api,
            &c_tag,
            &c_user,
            &c_illust,
            &mut users_need_update_set,
            &mut ugoira_map,
        )
        .await?;
        download::download_illusts(
            &r.illusts,
            &mut ugoira_map,
            downloader,
            &c_image,
            &mut items_sent,
            limit,
            task_config,
        )
        .await?;
        if limit_reached(limit, items_sent) {
            break;
        }
    }
    info!("{} illusts processed", items_sent);

    database::update_user_id_set(
        api,
        downloader,
        &c_user,
        &c_image,
        users_need_update_set,
        task_config,
    )
    .await?;

    Ok(())
}

pub async fn illust_uploads(
    api: &pixivcrab::AppApi,
    db: &mongodb::Database,
    downloader: &Aria2Downloader,
    user_id: &str,
    limit: Option<u32>,
    task_config: &TaskConfig,
) -> crate::Result<()> {
    let pager = api.illust_uploads(user_id);

    illusts(db, api, downloader, pager, limit, task_config).await
}

pub async fn illust_bookmarks(
    api: &pixivcrab::AppApi,
    db: &mongodb::Database,
    downloader: &Aria2Downloader,
    user_id: &str,
    private: bool,
    limit: Option<u32>,
    task_config: &TaskConfig,
) -> crate::Result<()> {
    let pager = api.illust_bookmarks(user_id, private);

    illusts(db, api, downloader, pager, limit, task_config).await
}

async fn novels<'a>(
    db: &Database,
    api: &AppApi,
    downloader: &Aria2Downloader,
    mut pager: pixivcrab::Pager<pixivcrab::models::novel::Response>,
    limit: Option<u32>,
    update_exists: bool,
    task_config: &TaskConfig,
) -> crate::Result<()> {
    let c_user = db.collection::<Document>("pixiv_user");
    let c_tag = db.collection::<Document>("pixiv_tag");
    let c_novel = db.collection::<Document>("pixiv_novel");
    let c_image = db.collection::<Document>("pixiv_image");

    let mut users_need_update_set = BTreeSet::new();
    let mut items_sent = 0;

    while let Some(r) = {
        info!("getting novels with offset: {}", items_sent);
        utils::retry_pager(&mut pager, 3).await?
    } {
        database::save_novels(
            r.novels,
            api,
            &c_user,
            &c_tag,
            &c_novel,
            limit,
            &mut items_sent,
            update_exists,
            &mut users_need_update_set,
        )
        .await?;
        if limit_reached(limit, items_sent) {
            break;
        }
    }
    info!("{} novels processed", items_sent);

    database::update_user_id_set(
        api,
        downloader,
        &c_user,
        &c_image,
        users_need_update_set,
        task_config,
    )
    .await?;

    Ok(())
}

pub async fn novel_bookmarks(
    api: &pixivcrab::AppApi,
    db: &mongodb::Database,
    downloader: &Aria2Downloader,
    update_exists: bool,
    user_id: &str,
    private: bool,
    limit: Option<u32>,
    task_config: &TaskConfig,
) -> crate::Result<()> {
    let pager = api.novel_bookmarks(user_id, private);
    novels(
        db,
        api,
        downloader,
        pager,
        limit,
        update_exists,
        task_config,
    )
    .await
}

pub async fn novel_uploads(
    api: &pixivcrab::AppApi,
    db: &mongodb::Database,
    downloader: &Aria2Downloader,
    update_exists: bool,
    user_id: &str,
    limit: Option<u32>,
    task_config: &TaskConfig,
) -> crate::Result<()> {
    let pager = api.novel_uploads(user_id);
    novels(
        db,
        api,
        downloader,
        pager,
        limit,
        update_exists,
        task_config,
    )
    .await
}
