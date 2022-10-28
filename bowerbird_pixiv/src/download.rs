use aria2_ws::TaskOptions;
use bowerbird_utils::{
    downloader::{BoxFutureResult, Task, TaskHooks},
    get_image_metadata, try_skip,
};
use futures::{Future, FutureExt};
use log::warn;

use snafu::ResultExt;
use sqlx::PgPool;
use std::{
    collections::HashMap,
    convert::TryInto,
    path::{Path, PathBuf},
};
use tokio::{fs::metadata, task::spawn_blocking};

use crate::{database::save_image, error, queries::media, utils::IllustUrl, Result};

use super::{
    utils::{self, filename_from_url},
    PixivKit,
};

async fn file_exists(path: impl AsRef<Path>) -> bool {
    let path = path.as_ref();
    if metadata(path).await.is_ok() {
        let mut aria2_path = path.as_os_str().to_os_string();
        aria2_path.push(".aria2");
        let aria2_non_exists = metadata(PathBuf::from(aria2_path)).await.is_err();
        return aria2_non_exists;
    }
    false
}

async fn on_success_ugoira(
    db: PgPool,
    url: String,
    path: PathBuf,
    path_db: String,
    ugoira_frame_delay: Vec<i32>,
    ffmpeg_path: Option<PathBuf>,
) -> anyhow::Result<()> {
    let with_mp4 = ffmpeg_path.is_some();
    if let Some(ffmpeg_path) = ffmpeg_path {
        let zip_path = path.clone();
        spawn_blocking(move || utils::ugoira_to_mp4(&ffmpeg_path, &zip_path, ugoira_frame_delay))
            .await
            .unwrap()?;
    }
    let zip_size: i64 = tokio::fs::metadata(&path).await?.len().try_into()?;

    super::database::save_image_ugoira(
        &db,
        url,
        path,
        path_db,
        zip_size.try_into().unwrap_or_default(),
        with_mp4,
    )
    .await?;

    Ok(())
}

async fn on_success_image(
    db: PgPool,
    url: String,
    path: impl AsRef<Path>,
    path_db: String,
) -> anyhow::Result<()> {
    let size: i32 = tokio::fs::metadata(&path)
        .await?
        .len()
        .try_into()
        .unwrap_or_default();
    let img_metadata = match {
        let image_path = path.as_ref().to_owned();
        spawn_blocking(move || get_image_metadata(image_path))
    }
    .await
    .unwrap()
    {
        Ok(m) => Some(m),
        Err(e) => {
            warn!(
                "image error: failed to get metadata for {}: {}",
                &path.as_ref().to_string_lossy(),
                e
            );
            None
        }
    };

    save_image(&db, size, img_metadata, url, path, path_db).await?;

    Ok(())
}

pub async fn download_image(parent_dir: &str, url: &str, kit: &PixivKit) -> Result<()> {
    let filename = filename_from_url(url)?;

    let path_db = format!("{parent_dir}/{filename}");
    let path = kit.task_config.parent_dir.join(&path_db);

    let on_success_hook = on_success_image(
        kit.db.clone(),
        url.to_string(),
        path.clone(),
        path_db.clone(),
    );
    if file_exists(&path).await {
        on_path_exists(&path_db, kit, on_success_hook).await?;
        return Ok(());
    }
    let task = build_task(on_success_hook.boxed(), kit, path_db, url.to_string());
    kit.downloader.add_task(task).await.context(error::Utils)
}

async fn on_path_exists(
    path_db: &str,
    kit: &PixivKit,
    on_success_hook: impl Future<Output = anyhow::Result<()>> + Send + 'static,
) -> Result<()> {
    if !media::local_path_exists(path_db, &kit.db).await? {
        kit.spawn_limited(on_success_hook);
    };
    Ok(())
}

async fn download_illust(
    url: Option<String>,
    user_id: &str,
    illust_id: &str,
    is_multi_page: bool,
    ugoira_frame_delay: Option<Vec<i32>>,
    kit: &PixivKit,
) -> Result<()> {
    let url = url.ok_or_else(|| {
        error::UnknownData {
            message: format!("empty url for {}", illust_id),
        }
        .build()
    })?;

    let parsed_url = IllustUrl::new(&url)?;
    let date = parsed_url.date.replace('/', "");

    let path_db = if is_multi_page {
        let filename = parsed_url.filename;
        format!("{user_id}/{illust_id}_{date}/{filename}")
    } else {
        let id_page = parsed_url.filename_without_ext;
        let ext = parsed_url.ext;
        format!("{user_id}/{id_page}_{date}.{ext}")
    };

    let path = kit.task_config.parent_dir.join(&path_db);

    let on_success_hook = if let Some(ugoira_frame_delay) = ugoira_frame_delay {
        // The task is an ugoira zip.
        on_success_ugoira(
            kit.db.clone(),
            url.clone(),
            path.clone(),
            path_db.clone(),
            ugoira_frame_delay,
            kit.task_config.ffmpeg_path.clone(),
        )
        .boxed()
    } else {
        on_success_image(kit.db.clone(), url.clone(), path.clone(), path_db.clone()).boxed()
    };
    if file_exists(&path).await {
        on_path_exists(&path_db, kit, on_success_hook).await?;
        return Ok(());
    }

    let task = build_task(on_success_hook, kit, path_db, url);
    kit.downloader.add_task(task).await.context(error::Utils)
}

fn build_task(
    on_success_hook: BoxFutureResult,
    kit: &PixivKit,
    path_db: String,
    url: String,
) -> Task {
    let task = Task {
        hooks: Some(TaskHooks {
            on_success: Some(on_success_hook),
            ..Default::default()
        }),
        options: Some(TaskOptions {
            header: Some(vec!["Referer: https://app-api.pixiv.net/".to_string()]),
            all_proxy: kit.task_config.proxy.clone(),
            out: Some(path_db),
            dir: Some(kit.task_config.parent_dir.to_string_lossy().to_string()),
            ..Default::default()
        }),
        url,
    };
    task
}

pub async fn download_illusts(
    illusts: &Vec<pixivcrab::models::illust::Illust>,
    ugoira_map: &mut HashMap<String, (String, Vec<i32>)>,
    mut on_each_should_continue: impl FnMut() -> bool,
    kit: &PixivKit,
) -> Result<()> {
    for i in illusts {
        if !on_each_should_continue() {
            break;
        }

        if !i.visible {
            continue;
        }
        let illust_id = i.id.to_string();
        let is_ugoira = i.r#type == "ugoira";

        if is_ugoira {
            if let Some((zip_url, delay)) = ugoira_map.remove(&illust_id) {
                let zip_url = zip_url.replace("600x600", "1920x1080");
                if let Err(err) = download_illust(
                    // get higher resolution images
                    Some(zip_url.clone()),
                    &i.user.id.to_string(),
                    &illust_id,
                    true,
                    Some(delay),
                    kit,
                )
                .await
                {
                    warn!("fail to build task from {}: {}", zip_url, err);
                }
            }
        }

        if i.page_count == 1 {
            try_skip!(
                download_illust(
                    i.meta_single_page.original_image_url.clone(),
                    &i.user.id.to_string(),
                    &illust_id,
                    is_ugoira,
                    None,
                    kit,
                )
                .await
            );
        } else {
            for img in &i.meta_pages {
                try_skip!(
                    download_illust(
                        img.image_urls.original.clone(),
                        &i.user.id.to_string(),
                        &illust_id,
                        true,
                        None,
                        kit,
                    )
                    .await
                );
            }
        }
    }
    Ok(())
}
