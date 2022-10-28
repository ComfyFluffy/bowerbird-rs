use anyhow::anyhow;
use chrono::NaiveDate;
use futures::TryStreamExt;
use lazy_static::lazy_static;
use log::warn;
use pixivcrab::Pager;
use regex::Regex;
use serde::de::DeserializeOwned;
use snafu::ResultExt;
use std::{
    fmt::Debug,
    fs::File,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};
use url::Url;

use crate::{error, Result};

lazy_static! {
    static ref RE_ILLUST_URL: Regex =
        Regex::new(r"/(\d{4}/\d{2}/\d{2}/\d{2}/\d{2}/\d{2})/((.*)\.(.*))$").unwrap();
}

/// Parse the pximg URL.
///
/// # Example
///
/// Parsing the URL
/// `https://i.pximg.net/img-original/img/2021/08/22/22/03/33/92187206_p0.jpg`
///
/// Results:
///
/// `date`: `2021/08/22/22/03/33`
///
/// `filename`: `92187206_p0.jpg`
///
/// `filename_without_ext`: `92187206_p0`
///
/// `ext`: `jpg`
pub struct IllustUrl<'a> {
    pub date: &'a str,
    pub filename: &'a str,
    pub filename_without_ext: &'a str,
    pub ext: &'a str,
}

impl<'a> IllustUrl<'a> {
    pub fn new(url: &'a str) -> Result<Self> {
        let captures = RE_ILLUST_URL.captures(url).ok_or_else(|| {
            error::UnknownData {
                message: format!("cannot parse illust url: {url}"),
            }
            .build()
        })?;

        Ok(Self {
            date: captures.get(1).unwrap().as_str(),
            filename: captures.get(2).unwrap().as_str(),
            filename_without_ext: captures.get(3).unwrap().as_str(),
            ext: captures.get(4).unwrap().as_str(),
        })
    }
}

pub fn ugoira_to_mp4(
    ffmpeg_path: impl AsRef<Path>,
    zip_path: impl AsRef<Path>,
    frame_delay: Vec<i32>,
) -> anyhow::Result<PathBuf> {
    let zip_path = zip_path.as_ref();
    let mut mp4_path = PathBuf::from(zip_path);

    let mut file = File::open(zip_path)?;
    let mut zip_file = zip::ZipArchive::new(&mut file)?;
    mp4_path.set_extension("mp4");

    let mut ffmpeg = Command::new(ffmpeg_path.as_ref())
        .args([
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "image2pipe",
            "-framerate",
            "60",
            "-i",
            "-",
            "-c:v",
            "libx264",
            "-preset",
            "slow",
            "-crf",
            "22",
            "-pix_fmt",
            "yuv420p",
            "-vf",
            "pad=ceil(iw/2)*2:ceil(ih/2)*2",
        ])
        .arg(mp4_path.as_os_str())
        .stdin(Stdio::piped())
        .spawn()?;
    {
        let mut stdin = ffmpeg.stdin.take().unwrap();

        let mut t: f32 = 0.0; // video length in milliseconds
        let mut frame = 0;
        for i in 0..zip_file.len() {
            t += *frame_delay
                .get(i)
                .ok_or_else(|| anyhow!("cannot get ugoira frame {i} from {frame_delay:?}"))?
                as f32; // add delay for each frame
            let next_frame = (t / (1000.0 / 60.0)).round() as i32; // get the next frame count at 60fps
            for _ in frame..next_frame {
                // repeatly push the same frame to stdin
                let mut file = zip_file.by_index(i)?;
                std::io::copy(&mut file, &mut stdin)?;
            }
            frame = next_frame;
        }
    } // close stdin to get status
    let status = ffmpeg.wait()?;
    if !status.success() {
        Err(anyhow!("ffmpeg exited with status {status}"))?
    }
    Ok(mp4_path)
}

pub async fn retry_pager<T>(pager: &mut Pager<T>, max_tries: i32) -> Result<Option<T>>
where
    T: DeserializeOwned + pixivcrab::NextUrl + Debug + Send,
{
    let mut tries = 0;
    loop {
        tries += 1;
        match pager.try_next().await.context(error::PixivApi) {
            Ok(r) => {
                return Ok(r);
            }
            Err(e) => {
                if let error::Error::PixivApi {
                    source: pixivcrab::error::Error::HTTP { .. },
                    ..
                } = &e
                {
                    if tries <= max_tries {
                        warn!("retrying on pixiv api error: {}", e);
                        tokio::time::sleep(Duration::from_secs(2)).await;
                        continue;
                    }
                }
                return Err(e);
            }
        }
    }
}

pub fn filename_from_url(url: &str) -> Result<String> {
    let filename = (|| -> Option<String> {
        Some(Url::parse(url).ok()?.path_segments()?.last()?.to_string())
    })();
    match filename {
        Some(filename) => Ok(filename),
        None => Err(error::UnknownData {
            message: format!("cannot parse filename from url: {}", url),
        }
        .build()),
    }
}

pub fn parse_birth(birth: &str) -> Option<NaiveDate> {
    if birth.is_empty() {
        None
    } else {
        NaiveDate::parse_from_str(birth, "%Y-%m-%d").ok()
    }
}
