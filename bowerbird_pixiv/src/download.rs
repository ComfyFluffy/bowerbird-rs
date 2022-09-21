use aria2_ws::TaskOptions;
use bowerbird_utils::{
    downloader::{Task, TaskHooks},
    get_dimensions_and_palette, try_skip,
};
use futures::FutureExt;
use lazy_static::lazy_static;
use log::warn;

use regex::{Captures, Regex};
use snafu::ResultExt;
use sqlx::PgPool;
use std::{
    collections::HashMap,
    convert::TryInto,
    path::{Path, PathBuf},
};
use tokio::task::spawn_blocking;

use crate::{database::save_image, error};

use super::{
    utils::{self, filename_from_url},
    PixivKit,
};

lazy_static! {
    /// Match the pximg URL.
    ///
    /// # Example
    ///
    /// Matching the URL
    /// `https://i.pximg.net/img-original/img/2021/08/22/22/03/33/92187206_p0.jpg`
    ///
    /// Groups:
    ///
    /// __0__ `/2021/08/22/22/03/33/92187206_p0.jpg`
    ///
    /// __1__ `2021/08/22/22/03/33`
    ///
    /// __2__ `92187206_p0.jpg`
    ///
    /// __3__ `92187206_p0`
    ///
    /// __4__ `jpg`
    static ref RE_ILLUST_URL: Regex =
        Regex::new(r"/(\d{4}/\d{2}/\d{2}/\d{2}/\d{2}/\d{2})/((.*)\.(.*))$").unwrap();
}

fn get_captures(url: &str) -> crate::Result<Captures> {
    RE_ILLUST_URL.captures(url).ok_or_else(|| {
        error::UnknownData {
            message: format!("cannot match url with regex: {url}"),
        }
        .build()
    })
}

fn file_exists(path: impl AsRef<Path>) -> bool {
    let path = path.as_ref();
    if path.exists() {
        let mut aria2_path = path.as_os_str().to_os_string();
        aria2_path.push(".aria2");
        return !PathBuf::from(aria2_path).exists();
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

async fn on_success_illust(
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
    let ((w, h), hsv_v) = {
        let image_path = path.as_ref().to_owned();
        spawn_blocking(move || get_dimensions_and_palette(image_path))
    }
    .await
    .unwrap()?;

    save_image(
        &db,
        size,
        (
            w.try_into().unwrap_or_default(),
            h.try_into().unwrap_or_default(),
        ),
        hsv_v,
        url,
        path,
        path_db,
    )
    .await?;

    Ok(())
}

pub async fn download_other_images(
    parent_dir: &str,
    url: &str,
    kit: &PixivKit,
) -> crate::Result<()> {
    let filename = filename_from_url(url)?;

    let path_db = format!("{parent_dir}/{filename}");
    let path = kit.task_config.parent_dir.join(&path_db);

    if file_exists(&path) {
        return Ok(());
    }

    let task = Task {
        hooks: Some(TaskHooks {
            on_success: Some(
                on_success_illust(
                    kit.db.clone(),
                    url.to_string(),
                    path.clone(),
                    path_db.clone(),
                )
                .boxed(),
            ),
            ..Default::default()
        }),
        options: Some(TaskOptions {
            header: Some(vec!["Referer: https://app-api.pixiv.net/".to_string()]),
            all_proxy: kit.task_config.proxy.clone(),
            out: Some(path_db),
            dir: Some(kit.task_config.parent_dir.to_string_lossy().to_string()),
            ..Default::default()
        }),
        url: url.to_string(),
    };
    kit.downloader.add_task(task).await.context(error::Utils)
}

async fn download_illust(
    url: Option<String>,
    user_id: &str,
    illust_id: &str,
    is_multi_page: bool,
    ugoira_frame_delay: Option<Vec<i32>>,
    kit: &PixivKit,
) -> crate::Result<()> {
    let url = url.ok_or_else(|| {
        error::UnknownData {
            message: format!("empty url for {}", illust_id),
        }
        .build()
    })?;

    let captures = get_captures(&url)?;
    let date = captures.get(1).unwrap().as_str().replace('/', "");

    let path_db = if is_multi_page {
        let filename = captures.get(2).unwrap().as_str();
        format!("{user_id}/{illust_id}_{date}/{filename}")
    } else {
        let id_page = captures.get(3).unwrap().as_str();
        let ext = captures.get(4).unwrap().as_str();
        format!("{user_id}/{id_page}_{date}.{ext}")
    };

    let path = kit.task_config.parent_dir.join(&path_db);

    if file_exists(&path) {
        return Ok(());
    }

    let on_success_hook = if let Some(ugoira_frame_delay) = ugoira_frame_delay {
        // The task is an ugoira zip.
        on_success_ugoira(
            kit.db.clone(),
            url.clone(),
            path,
            path_db.clone(),
            ugoira_frame_delay,
            kit.task_config.ffmpeg_path.clone(),
        )
        .boxed()
    } else {
        on_success_illust(kit.db.clone(), url.clone(), path, path_db.clone()).boxed()
    };

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
    kit.downloader.add_task(task).await.context(error::Utils)
}

pub async fn download_illusts(
    illusts: &Vec<pixivcrab::models::illust::Illust>,
    ugoira_map: &mut HashMap<String, (String, Vec<i32>)>,
    mut on_each_should_continue: impl FnMut() -> bool,
    kit: &PixivKit,
) -> crate::Result<()> {
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
                    warn!("Fail to build task from {}: {}", zip_url, err);
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
