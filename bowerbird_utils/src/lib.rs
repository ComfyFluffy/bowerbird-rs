use ::log::{debug, warn};
use image::GenericImageView;
use snafu::ResultExt;
use std::{
    net::TcpListener,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{process::Command, time::timeout};

mod waitgroup;

pub mod downloader;
pub mod error;

pub use waitgroup::WaitGroup;
pub(crate) type Result<T> = std::result::Result<T, error::Error>;

pub fn get_available_port<T>(ra: T) -> Option<u16>
where
    T: Iterator<Item = u16>,
{
    for p in ra {
        match TcpListener::bind(("127.0.0.1", p)) {
            Ok(_) => return Some(p),
            Err(e) => {
                eprintln!("{}", e)
            }
        }
    }
    None
}

pub type Hsv = [f32; 3];

#[derive(Debug, Clone, PartialEq)]
pub struct ImageMetadata {
    pub hsv_palette: Vec<Hsv>,
    pub width: u32,
    pub height: u32,
}

pub fn get_image_metadata(image_path: impl AsRef<Path>) -> Result<ImageMetadata> {
    let img = image::open(image_path).context(error::Image)?;
    let dim = img.dimensions();
    let thumbnail = img.thumbnail(512, 512).to_rgba8();
    drop(img);

    let hsv_v = color_thief::get_palette(thumbnail.as_raw(), color_thief::ColorFormat::Rgba, 5, 5)
        .context(error::ColorTheif)?
        .into_iter()
        .map(|c| rgb_to_hsv(c.r, c.g, c.b))
        .collect();
    Ok(ImageMetadata {
        hsv_palette: hsv_v,
        width: dim.0,
        height: dim.1,
    })
}

pub fn rgb_to_hsv(r: u8, g: u8, b: u8) -> Hsv {
    let r = r as f32 / 255.0;
    let g = g as f32 / 255.0;
    let b = b as f32 / 255.0;

    let max = r.max(g.max(b));
    let min = r.min(g.min(b));
    let d = max - min;
    let mut h = if max == min {
        0.0
    } else if max == r {
        60.0 * (((g - b) / d) % 6.0)
    } else if max == g {
        60.0 * (((b - r) / d) + 2.0)
    } else {
        60.0 * (((r - g) / d) + 4.0)
    };
    if h < 0.0 {
        h += 360.0;
    }
    let s = if max == 0.0 { 0.0 } else { d / max };
    let v = max;
    [h, s, v]
}

#[cfg(test)]
mod tests {
    use super::rgb_to_hsv;
    #[test]
    fn rgb2hsv() {
        assert_eq!(rgb_to_hsv(255, 0, 0), [0.0, 1.0, 1.0]);
        assert_eq!(rgb_to_hsv(0, 255, 0), [120.0, 1.0, 1.0]);
        assert_eq!(rgb_to_hsv(0, 0, 255), [240.0, 1.0, 1.0]);
        assert_eq!(rgb_to_hsv(255, 255, 255), [0.0, 0.0, 1.0]);
        assert_eq!(rgb_to_hsv(0, 0, 0), [0.0, 0.0, 0.0]);
        assert_eq!(
            rgb_to_hsv(108, 52, 62).map(|x| (x * 100.0).round()),
            [34929.0, 52.0, 42.0]
        );
    }
}

#[macro_export]
macro_rules! try_skip {
    ($res:expr) => {
        match $res {
            Ok(val) => val,
            Err(e) => {
                warn!("{}", e);
                continue;
            }
        }
    };
}

pub async fn check_ffmpeg(path: &str) -> Option<PathBuf> {
    let ffmpeg_path = if path.is_empty() {
        PathBuf::from("ffmpeg")
    } else {
        PathBuf::from(path)
    };

    debug!("run command to check ffmpeg: {:?}", ffmpeg_path);

    let mut ffmpeg = Command::new(&ffmpeg_path);
    ffmpeg.args(["-hide_banner", "-loglevel", "error"]);
    match ffmpeg.spawn() {
        Ok(mut child) => {
            if timeout(Duration::from_secs(1), child.wait()).await.is_err() {
                warn!("ffmpeg running for 1s, try to kill it");
                child.kill().await.ok();
            };
            Some(ffmpeg_path)
        }
        Err(err) => {
            warn!(
                "ffmpeg not found, some functions will not work: {}: {}",
                ffmpeg_path.to_string_lossy(),
                err
            );
            None
        }
    }
}
