use std::{
    fmt::Debug,
    fs::File,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use image::GenericImageView;
use pixivcrab::Pager;
use serde::de::DeserializeOwned;
use snafu::ResultExt;

use crate::{
    error::{self, BoxError},
    log::{debug, warning},
    model,
};

pub fn ugoira_to_mp4(
    ffmpeg_path: impl AsRef<Path>,
    zip_path: impl AsRef<Path>,
    frame_delay: Vec<i32>,
) -> Result<PathBuf, BoxError> {
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
    let mut stdin = ffmpeg.stdin.take().unwrap();

    let mut t: f32 = 0.0;
    let mut frame = 0;
    for i in 0..zip_file.len() {
        t += *frame_delay.get(i).ok_or(format!(
            "Cannot get ugoira frame {} from {:?}",
            i, frame_delay
        ))? as f32;
        let next_frame = (t / (1000.0 / 60.0)).round() as i32;
        for _ in frame..next_frame {
            let mut file = zip_file.by_index(i)?;
            std::io::copy(&mut file, &mut stdin)?;
        }
        frame = next_frame;
    }
    drop(stdin);
    let status = ffmpeg.wait()?;
    if !status.success() {
        Err(format!("FFmpeg exited with status {}", status))?
    }
    Ok(mp4_path)
}

pub fn get_palette(
    image_path: impl AsRef<Path>,
) -> Result<((i32, i32), Vec<model::Rgb>), BoxError> {
    let img = image::open(image_path)?;
    let (w, h) = img.dimensions();
    let thumbnail = img.thumbnail(512, 512).to_rgba8();
    drop(img);

    let rgb_v = color_thief::get_palette(thumbnail.as_raw(), color_thief::ColorFormat::Rgba, 5, 5)?
        .into_iter()
        .map(|c| model::Rgb(c.r.into(), c.g.into(), c.b.into()))
        .collect();
    // Convert to i32 here to save to bson.
    Ok(((w as i32, h as i32), rgb_v))
}

pub async fn retry_pager<'a, T>(
    pager: &mut Pager<'a, T>,
    max_tries: i32,
) -> crate::Result<Option<T>>
where
    T: DeserializeOwned + pixivcrab::NextUrl + Debug,
{
    let mut tries = 0;
    loop {
        tries += 1;
        debug!("Executing pager {:?}", pager);
        match pager.next().await.context(error::PixivAPI) {
            Ok(r) => {
                return Ok(r);
            }
            Err(e) => {
                if let error::Error::PixivAPI { source, .. } = &e {
                    if let pixivcrab::error::Error::HTTP { .. } = source {
                        if tries <= max_tries {
                            warning!("retrying on pixiv api error: {:?} :{}", pager, e);
                            tokio::time::sleep(Duration::from_secs(2)).await;
                            continue;
                        }
                    }
                }
                return Err(e);
            }
        }
    }
}
