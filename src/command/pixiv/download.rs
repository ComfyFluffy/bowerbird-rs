use aria2_ws::TaskOptions;
use futures::FutureExt;
use lazy_static::lazy_static;
use log::warn;
use mongodb::{
    bson::{doc, Document},
    Collection,
};

use regex::{Captures, Regex};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use tokio::task::spawn_blocking;

use super::{
    utils::{self, filename_from_url},
    TaskConfig,
};
use crate::{
    downloader::{Aria2Downloader, Task, TaskHooks},
    error::{self, BoxError},
    utils::try_skip,
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
    RE_ILLUST_URL.captures(&url).ok_or(
        error::PixivParse {
            message: format!("cannot match url with regex: {url}"),
        }
        .build(),
    )
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
    zip_url: String,
    zip_path: PathBuf,
    c_image: Collection<Document>,
    path_slash: String,
    ugoira_frame_delay: Vec<i32>,
    ffmpeg_path: Option<PathBuf>,
) -> Result<(), BoxError> {
    let with_mp4 = ffmpeg_path.is_some();
    if let Some(ffmpeg_path) = ffmpeg_path {
        let zip_path = zip_path.clone();
        spawn_blocking(move || utils::ugoira_to_mp4(&ffmpeg_path, &zip_path, ugoira_frame_delay))
            .await
            .unwrap()?;
    }
    let zip_size: i64 = tokio::fs::metadata(&zip_path).await?.len().try_into()?;

    super::database::save_image_ugoira(&c_image, zip_url, zip_path, path_slash, zip_size, with_mp4)
        .await?;

    Ok(())
}

async fn on_success_illust(
    url: String,
    image_path: PathBuf,
    c_image: Collection<Document>,
    path_slash: String,
) -> Result<(), BoxError> {
    let size: i64 = tokio::fs::metadata(&image_path).await?.len().try_into()?;
    let ((w, h), hsv_v) = {
        let image_path = image_path.clone();
        spawn_blocking(move || utils::get_palette(image_path))
    }
    .await
    .unwrap()?;
    super::database::save_image(&c_image, size, (w, h), hsv_v, url, path_slash, image_path).await?;

    Ok(())
}

pub async fn download_other_images(
    downloader: &Aria2Downloader,
    c_image: &Collection<Document>,
    url: &str,
    parent_dir: &str,
    task_config: &TaskConfig,
) -> crate::Result<()> {
    let filename = filename_from_url(&url)?;

    let path_slash = format!("{parent_dir}/{filename}");
    let path = task_config.parent_dir.join(&path_slash);

    if file_exists(&path) {
        return Ok(());
    }

    let task = Task {
        hooks: Some(TaskHooks {
            on_success: Some(
                on_success_illust(
                    url.to_string(),
                    path.clone(),
                    c_image.clone(),
                    path_slash.clone(),
                )
                .boxed(),
            ),
            ..Default::default()
        }),
        options: Some(TaskOptions {
            header: Some(vec!["Referer: https://app-api.pixiv.net/".to_string()]),
            all_proxy: task_config.proxy.clone(),
            out: Some(path_slash),
            dir: Some(task_config.parent_dir.to_string_lossy().to_string()),
            ..Default::default()
        }),
        url: url.to_string(),
    };
    downloader.add_task(task).await
}

async fn download_illust(
    downloader: &Aria2Downloader,
    c_image: &Collection<Document>,
    url: Option<String>,
    user_id: &str,
    illust_id: &str,
    is_multi_page: bool,
    ugoira_frame_delay: Option<Vec<i32>>,
    task_config: &TaskConfig,
) -> crate::Result<()> {
    let url = url.ok_or(
        error::PixivParse {
            message: format!("empty url for {}", illust_id),
        }
        .build(),
    )?;

    let captures = get_captures(&url)?;
    let date = captures.get(1).unwrap().as_str().replace("/", "");

    let path_slash = if is_multi_page {
        let filename = captures.get(2).unwrap().as_str();
        format!("{user_id}/{illust_id}_{date}/{filename}")
    } else {
        let id_page = captures.get(3).unwrap().as_str();
        let ext = captures.get(4).unwrap().as_str();
        format!("{user_id}/{id_page}_{date}.{ext}")
    };

    let path = task_config.parent_dir.join(&path_slash);

    if file_exists(&path) {
        return Ok(());
    }

    let on_success_hook = if let Some(ugoira_frame_delay) = ugoira_frame_delay {
        // The task is an ugoira zip.
        on_success_ugoira(
            url.clone(),
            path.clone(),
            c_image.clone(),
            path_slash.clone(),
            ugoira_frame_delay,
            task_config.ffmpeg_path.clone(),
        )
        .boxed()
    } else {
        on_success_illust(
            url.clone(),
            path.clone(),
            c_image.clone(),
            path_slash.clone(),
        )
        .boxed()
    };

    let task = Task {
        hooks: Some(TaskHooks {
            on_success: Some(on_success_hook),
            ..Default::default()
        }),
        options: Some(TaskOptions {
            header: Some(vec!["Referer: https://app-api.pixiv.net/".to_string()]),
            all_proxy: task_config.proxy.clone(),
            out: Some(path_slash),
            dir: Some(task_config.parent_dir.to_string_lossy().to_string()),
            ..Default::default()
        }),
        url,
    };
    downloader.add_task(task).await
}

pub async fn download_illusts(
    illusts: &Vec<pixivcrab::models::illust::Illust>,
    ugoira_map: &mut HashMap<String, (String, Vec<i32>)>,
    downloader: &Aria2Downloader,
    c_image: &Collection<Document>,
    items_sent: &mut u32,
    limit: Option<u32>,
    task_config: &TaskConfig,
) -> crate::Result<()> {
    for i in illusts {
        if super::limit_reached(limit, *items_sent) {
            break;
        }
        *items_sent += 1;

        if !i.visible {
            continue;
        }
        let illust_id = i.id.to_string();
        let is_ugoira = i.r#type == "ugoira";

        if is_ugoira {
            if let Some((zip_url, delay)) = ugoira_map.remove(&illust_id) {
                let zip_url = zip_url.replace("600x600", "1920x1080");
                if let Err(err) = download_illust(
                    downloader,
                    c_image,
                    // get higher resolution images
                    Some(zip_url.clone()),
                    &i.user.id.to_string(),
                    &illust_id,
                    true,
                    Some(delay),
                    task_config,
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
                    downloader,
                    c_image,
                    i.meta_single_page.original_image_url.clone(),
                    &i.user.id.to_string(),
                    &illust_id,
                    is_ugoira,
                    None,
                    task_config
                )
                .await
            );
        } else {
            for img in &i.meta_pages {
                try_skip!(
                    download_illust(
                        downloader,
                        c_image,
                        img.image_urls.original.clone(),
                        &i.user.id.to_string(),
                        &illust_id,
                        true,
                        None,
                        task_config
                    )
                    .await
                );
            }
        }
    }
    Ok(())
}
