use pixivcrab::AppAPI;
use std::{
    collections::{BTreeSet, HashMap},
    path::PathBuf,
};

use mongodb::{bson::Document, Database};

mod database;
mod download;
mod utils;

async fn illusts<'a>(
    db: &Database,
    api: &AppAPI,
    downloader: &crate::downloader::Downloader,
    mut pager: pixivcrab::Pager<'a, pixivcrab::models::illust::Response>,
    parent_dir: &PathBuf,
    limit: Option<u32>,
    ffmpeg_path: &Option<PathBuf>,
) -> crate::Result<()> {
    let c_illust = db.collection::<Document>("pixiv_illust");
    let c_user = db.collection::<Document>("pixiv_user");
    let c_tag = db.collection::<Document>("pixiv_tag");
    let c_image = db.collection::<Document>("pixiv_image");

    let mut users_need_update_set = BTreeSet::new();
    let mut ugoira_map = HashMap::new();

    let mut items_sent = 0;
    while let Some(r) = utils::retry_pager(&mut pager, 3).await? {
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
            api,
            downloader,
            &c_image,
            &mut items_sent,
            limit,
            parent_dir,
            ffmpeg_path,
        )
        .await;
        if let Some(limit) = limit {
            if items_sent >= limit {
                break;
            }
        }
    }

    database::update_user_id_set(api, &c_user, users_need_update_set).await?;

    Ok(())
}

pub async fn illust_uploads(
    api: &pixivcrab::AppAPI,
    db: &mongodb::Database,
    downloader: &crate::downloader::Downloader,
    parent_dir: PathBuf,
    user_id: &str,
    limit: Option<u32>,
    ffmpeg_path: &Option<PathBuf>,
) -> crate::Result<()> {
    let pager = api.illust_uploads(user_id);

    illusts(db, api, downloader, pager, &parent_dir, limit, ffmpeg_path).await
}

pub async fn illust_bookmarks(
    api: &pixivcrab::AppAPI,
    db: &mongodb::Database,
    downloader: &crate::downloader::Downloader,
    parent_dir: PathBuf,
    user_id: &str,
    private: bool,
    limit: Option<u32>,
    ffmpeg_path: &Option<PathBuf>,
) -> crate::Result<()> {
    let pager = api.illust_bookmarks(user_id, private);

    illusts(db, api, downloader, pager, &parent_dir, limit, ffmpeg_path).await
}

pub async fn novels<'a>(
    mut pager: pixivcrab::Pager<'a, pixivcrab::models::novel::Response>,
    db: &Database,
    api: &AppAPI,
    limit: Option<u32>,
    update_exists: bool,
) -> crate::Result<()> {
    let mut users_need_update_set = BTreeSet::new();
    let mut items_sent = 0;

    let c_user = db.collection::<Document>("pixiv_user");
    let c_tag = db.collection::<Document>("pixiv_tag");
    let c_novel = db.collection::<Document>("pixiv_novel");

    while let Some(r) = utils::retry_pager(&mut pager, 3).await? {
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
        if let Some(limit) = limit {
            if items_sent >= limit {
                break;
            }
        }
    }

    database::update_user_id_set(api, &c_user, users_need_update_set).await?;

    Ok(())
}

pub async fn novel_bookmarks(
    api: &pixivcrab::AppAPI,
    db: &mongodb::Database,
    update_exists: bool,
    user_id: &str,
    private: bool,
    limit: Option<u32>,
) -> crate::Result<()> {
    let pager = api.novel_bookmarks(user_id, private);

    novels(pager, db, api, limit, update_exists).await
}

pub async fn novel_uploads(
    api: &pixivcrab::AppAPI,
    db: &mongodb::Database,
    update_exists: bool,
    user_id: &str,
    limit: Option<u32>,
) -> crate::Result<()> {
    let pager = api.novel_uploads(user_id);

    novels(pager, db, api, limit, update_exists).await
}
