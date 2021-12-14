use futures::FutureExt;
use lazy_static::lazy_static;
use regex::Regex;

use pixivcrab::AppAPI;

use std::{collections::HashMap, path::PathBuf};
use tokio::task::spawn_blocking;

use reqwest::{Method, Url};
use snafu::ResultExt;

use crate::{
    downloader::{ClosureFuture, Downloader, Task, TaskHooks, TaskOptions},
    error,
    log::warning,
};

use mongodb::{
    bson::{doc, Document},
    Collection,
};

use path_slash::PathBufExt;

use super::utils;
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

macro_rules! try_skip {
    ($res:expr) => {
        match $res {
            Ok(val) => val,
            Err(e) => {
                warning!("{}", e);
                continue;
            }
        }
    };
}

fn task_from_illust(
    api: &AppAPI,
    c_image: Collection<Document>,
    raw_url: Option<String>,
    parent_dir: &PathBuf,
    user_id: &str,
    illust_id: &str,
    is_multi_page: bool,
    ffmpeg_path: &Option<PathBuf>,
    ugoira_frame_delay: Option<Vec<i32>>,
) -> crate::Result<Task> {
    let url = match raw_url {
        Some(raw_url) => match raw_url.parse::<Url>() {
            Ok(url) => url,
            Err(e) => {
                return Err(e).context(error::PixivParseURL);
            }
        },
        None => {
            return error::PixivParse {
                message: "empty url".to_string(),
            }
            .fail()
        }
    };

    let captures = RE_ILLUST_URL.captures(url.path()).ok_or(
        error::PixivParse {
            message: format!("cannot match url with RE_ILLUST_URL: {}", url),
        }
        .build(),
    )?;
    let date = captures.get(1).unwrap().as_str().replace("/", "");

    let request_builder = {
        let url = url.clone();
        let hash_secret = api.hash_secret.clone();
        move |client: &reqwest::Client| {
            client
                .request(Method::GET, url.clone())
                .headers(pixivcrab::default_headers(&hash_secret))
                .header("Referer", "https://app-api.pixiv.net/")
                .build()
                .context(error::DownloadRequestBuild)
        }
    };
    let path_slash = if is_multi_page {
        format!(
            "{}/{}_{}/{}",
            user_id,
            illust_id,
            date,
            captures.get(2).unwrap().as_str()
        )
    } else {
        format!(
            "{}/{}_{}.{}",
            user_id,
            captures.get(3).unwrap().as_str(), // filename with page id
            date,
            captures.get(4).unwrap().as_str(), // extension
        )
    };

    let path_slash_cloned = path_slash.clone();

    let on_success_hook: ClosureFuture = if let Some(ffmpeg_path) = ffmpeg_path {
        // The task is an ugoira zip.
        let ffmpeg_path = ffmpeg_path.clone();
        let zip_url = url.clone();
        Box::new(move |t| {
            let zip_size = t.file_size.unwrap().try_into().unwrap_or_default();
            let zip_path = t.options.path.clone().unwrap();

            async move {
                let zip_path_cloned = zip_path.clone();
                spawn_blocking(move || {
                    utils::ugoira_to_mp4(
                        &ffmpeg_path,
                        &zip_path_cloned,
                        ugoira_frame_delay.unwrap(),
                    )
                })
                .await
                .unwrap()?;

                super::database::save_image_ugoira(
                    &c_image,
                    zip_url.to_string(),
                    zip_path,
                    path_slash,
                    zip_size,
                )
                .await?;

                Ok(())
            }
            .boxed()
        })
    } else {
        Box::new(move |t| {
            let image_path = t.options.path.clone().unwrap();
            let size = t.file_size.unwrap().try_into().unwrap_or_default();
            let url = t.url.clone();

            async move {
                let image_path_cloned = image_path.clone();
                let ((w, h), rgb_v) =
                    spawn_blocking(move || utils::get_palette(&image_path_cloned))
                        .await
                        .unwrap()?;
                super::database::save_image(
                    &c_image,
                    size,
                    (w, h),
                    rgb_v,
                    url.to_string(),
                    path_slash,
                    image_path,
                )
                .await?;

                Ok(())
            }
            .boxed()
        })
    };

    let path = parent_dir.join(PathBuf::from_slash(&path_slash_cloned));

    Ok(Task::new(
        Box::new(request_builder),
        url,
        TaskOptions {
            path: Some(path),
            ..Default::default()
        },
        Some(TaskHooks {
            on_error: None,
            on_success: Some(on_success_hook),
        }),
    ))
}

pub async fn download_illusts(
    illusts: &Vec<pixivcrab::models::illust::Illust>,
    ugoira_map: &mut HashMap<String, (String, Vec<i32>)>,
    api: &AppAPI,
    downloader: &Downloader,
    c_image: &Collection<Document>,
    items_sent: &mut u32,
    limit: Option<u32>,
    parent_dir: &PathBuf,
    ffmpeg_path: &Option<PathBuf>,
) {
    let mut tasks = Vec::new();
    for i in illusts {
        if super::limit_reached(limit, *items_sent) {
            return;
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
                match task_from_illust(
                    &api,
                    c_image.clone(),
                    // get higher resolution images
                    Some(zip_url.clone()),
                    parent_dir,
                    &i.user.id.to_string(),
                    &illust_id,
                    true,
                    ffmpeg_path,
                    Some(delay),
                ) {
                    Ok(task) => {
                        tasks.push(task);
                    }
                    Err(err) => {
                        warning!("Fail to build task from {}: {}", zip_url, err)
                    }
                }
            }
        }

        if i.page_count == 1 {
            let task = try_skip!(task_from_illust(
                &api,
                c_image.clone(),
                i.meta_single_page.original_image_url.clone(),
                parent_dir,
                &i.user.id.to_string(),
                &illust_id,
                is_ugoira,
                &None,
                None
            ));
            tasks.push(task);
        } else {
            for img in &i.meta_pages {
                let task = try_skip!(task_from_illust(
                    &api,
                    c_image.clone(),
                    img.image_urls.original.clone(),
                    parent_dir,
                    &i.user.id.to_string(),
                    &illust_id,
                    true,
                    &None,
                    None
                ));
                tasks.push(task);
            }
        }
    }
    downloader.send(tasks).await;
    if super::limit_reached(limit, *items_sent) {
        return;
    }
}
