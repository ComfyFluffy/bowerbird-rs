use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::Mutex,
    time::Instant,
};

use actix_web::http::StatusCode;
use bytes::Bytes;
use image::{imageops::FilterType::Lanczos3, GenericImageView, ImageOutputFormat};
use tokio::{sync::Semaphore, task::spawn_blocking};

use crate::server::error::ServerErrorExt;

#[derive(Debug, Hash, PartialEq, Eq, Clone)]
pub struct ThumbnailCacheKey {
    size: u32,
    local_path: PathBuf,
}

pub type ThumbnailCache = HashMap<ThumbnailCacheKey, Bytes>;

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
) -> super::Result<Bytes> {
    let mut cache_lock = cache.lock().unwrap();
    if cache_lock.len() > 200 {
        let k = cache_lock.keys().next().unwrap().clone();
        cache_lock.remove(&k);
    }

    let local_path = local_path.as_ref().to_path_buf();
    if let Some(b) = cache_lock.get(&ThumbnailCacheKey {
        local_path: local_path.clone(),
        size,
    }) {
        Ok(b.clone())
    } else {
        drop(cache_lock);

        let b = spawn_semaphore(semaphore, {
            let local_path = local_path.clone();
            move || make_thumbnail(local_path, size)
        })
        .await?;

        cache
            .lock()
            .unwrap()
            .insert(ThumbnailCacheKey { local_path, size }, b.clone());
        Ok(b)
    }
}

fn make_thumbnail(local_path: impl AsRef<Path>, size: u32) -> super::Result<Bytes> {
    let t = Instant::now();
    let img = image::io::Reader::open(&local_path)
        .with_status(StatusCode::NOT_FOUND)?
        .decode()
        .with_interal()?;
    let (w, h) = img.dimensions();
    let wdh = (w as f64) / (h as f64);
    let img = if wdh < 0.75 {
        img.resize_to_fill(size / 4 * 3, size, Lanczos3)
    } else if wdh > 1.33 {
        img.resize_to_fill(size, size / 4 * 3, Lanczos3)
    } else {
        img.resize(size, size, Lanczos3)
    };
    let mut b = Vec::with_capacity(1024 * 50);
    img.write_to(&mut b, ImageOutputFormat::Jpeg(85))
        .with_interal()?;
    b.shrink_to_fit();
    log::debug!(
        "made thumbnail for {:?}: {:?}",
        local_path.as_ref(),
        t.elapsed()
    );
    Ok(Bytes::from(b))
}
