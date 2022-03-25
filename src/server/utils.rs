use actix_web::http::StatusCode;
use bson::Regex;
use bytes::Bytes;
use image::{imageops::FilterType::Lanczos3, GenericImageView, ImageOutputFormat};
use log::debug;
use std::{
    collections::HashMap,
    io::Cursor,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Instant,
};
use tokio::{sync::Semaphore, task::spawn_blocking};

use crate::server::error::ServerErrorExt;

#[derive(Debug, Hash, PartialEq, Eq, Clone)]
pub struct ThumbnailCacheKey {
    size: u32,
    local_path: PathBuf,
    target_ratio: Option<u32>,
}

pub type ThumbnailCache = HashMap<ThumbnailCacheKey, Bytes>;

/// Spawns cpu-bound task and await for result.
/// The spawned task is aborted when the handle is dropped.
///
/// # Panics
/// If the semaphore has been closed or `f` panics.
pub async fn spawn_semaphore<F, R>(semaphore: &Semaphore, f: F) -> R
where
    F: FnOnce() -> R + Send + 'static,
    R: Send + 'static,
{
    let _permit = semaphore.acquire().await.unwrap();
    spawn_blocking(f).await.unwrap()
}

pub async fn cached_image_thumbnail(
    local_path: impl AsRef<Path>,
    size: u32,
    cache: &Mutex<ThumbnailCache>,
    semaphore: &Semaphore,
    quality: u8,
    target_ratio: Option<f32>,
) -> super::Result<Bytes> {
    let mut cache_lock = cache.lock().unwrap();
    if cache_lock.len() > 500 {
        let k = cache_lock.keys().next().unwrap().clone();
        cache_lock.remove(&k);
    }

    let local_path = local_path.as_ref().to_path_buf();
    let target_ratio_u32 = target_ratio.map(|t| (t * 100.0) as u32);
    if let Some(b) = cache_lock.get(&ThumbnailCacheKey {
        local_path: local_path.clone(),
        size,
        target_ratio: target_ratio_u32,
    }) {
        Ok(b.clone())
    } else {
        drop(cache_lock);

        let b = spawn_semaphore(semaphore, {
            let local_path = local_path.clone();
            move || make_thumbnail(local_path, size, quality, target_ratio)
        })
        .await?;

        cache.lock().unwrap().insert(
            ThumbnailCacheKey {
                local_path,
                size,
                target_ratio: target_ratio_u32,
            },
            b.clone(),
        );
        Ok(b)
    }
}

fn make_thumbnail(
    local_path: impl AsRef<Path>,
    size: u32,
    quality: u8,
    target_ratio: Option<f32>,
) -> super::Result<Bytes> {
    let t = Instant::now();
    let mut img = image::io::Reader::open(&local_path)
        .with_status(StatusCode::NOT_FOUND)?
        .decode()
        .with_interal()?;
    let (w, h) = img.dimensions();
    if w > size || h > size {
        img = if let Some(target_ratio) = target_ratio {
            let wdh = (w as f32) / (h as f32);
            if wdh < target_ratio {
                img.resize_to_fill((size as f32 * target_ratio) as u32, size, Lanczos3)
            } else if wdh > (1.0 / target_ratio) {
                img.resize_to_fill(size, (size as f32 * target_ratio) as u32, Lanczos3)
            } else {
                img.resize(size, size, Lanczos3)
            }
        } else {
            img.resize(size, size, Lanczos3)
        }
    }
    let mut b = Cursor::new(Vec::with_capacity(1024 * 50));
    img.write_to(&mut b, ImageOutputFormat::Jpeg(quality))
        .with_interal()?;
    let mut b = b.into_inner();
    b.shrink_to_fit();
    debug!(
        "made thumbnail for {:?}: {:?}",
        local_path.as_ref(),
        t.elapsed()
    );
    Ok(Bytes::from(b))
}

pub fn build_search_regex(search: &str) -> Regex {
    Regex {
        pattern: regex::escape(search),
        options: "i".to_string(),
    }
}
