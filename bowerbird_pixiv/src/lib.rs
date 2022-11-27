use bowerbird_core::config::Config;
use bowerbird_utils::{check_ffmpeg, downloader::Aria2Downloader, logged_rustls_with_native_root};
use futures::Future;
use log::{debug, error, info, warn};
use pixivcrab::{AppApi, Pager};
use reqwest::ClientBuilder;
use serde::de::DeserializeOwned;
use snafu::ResultExt;
use sqlx::PgPool;
use std::{
    collections::{BTreeSet, HashMap},
    fmt::Debug,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tokio::{spawn, sync::Semaphore};

pub mod database;
pub mod download;
mod error;
mod queries;
mod utils;

pub use error::Error;

pub(crate) type Result<T> = std::result::Result<T, Error>;

fn limit_reached<T>(limit: Option<T>, items_sent: T) -> bool
where
    T: std::cmp::PartialOrd,
{
    if let Some(limit) = limit {
        items_sent >= limit
    } else {
        false
    }
}

#[derive(Debug, Clone)]
pub struct TaskConfig {
    pub ffmpeg_path: Option<PathBuf>,
    pub proxy: Option<String>,
    pub parent_dir: PathBuf,
}

pub struct PixivKit {
    pub api: AppApi,
    pub db: PgPool,
    pub downloader: Aria2Downloader,
    pub task_config: TaskConfig,
    pub config: Config,
    pub auth_result: pixivcrab::AuthResult,
    pub max_tries: i32,
    tasks_semaphore: Arc<Semaphore>,
    tasks_initial_permits: usize,
}

macro_rules! retry_impl {
    ($fut:expr, $max_tries:expr) => {
        use pixivcrab::error::Error;

        let mut tries = 1;
        loop {
            match $fut.await {
                Ok(r) => {
                    return Ok(r);
                }
                Err(e) => {
                    match &e {
                        Error::Http { .. } => {
                            if tries < $max_tries {
                                warn!("retrying on pixiv api error: {}", e);
                                tokio::time::sleep(Duration::from_secs(2)).await;
                                tries += 1;
                                continue;
                            }
                        }
                        Error::UnexpectedStatus { status, text } => {
                            if status.as_u16() == 403
                                && text.to_ascii_lowercase().contains("rate limit")
                            {
                                warn!("pixiv api rate limit reached, will retry in 60s: {}", e);
                                tokio::time::sleep(Duration::from_secs(60)).await;
                                continue;
                            }
                        }
                        _ => (),
                    };
                    return Err(e).context(error::PixivApi);
                }
            }
        }
    };
}

impl PixivKit {
    /// Log in to pixiv, save token, start aria2, and check ffmpeg.
    pub async fn new(mut config: Config, db: PgPool) -> Result<Self> {
        let mut api_client = ClientBuilder::new().cookie_store(true);
        if config.ssl_key_log {
            api_client = logged_rustls_with_native_root(api_client).context(error::Utils)?;
        }
        if let Some(proxy) = config
            .pxoxy(&config.pixiv.proxy_api)
            .context(error::Config)?
        {
            debug!("pixiv api proxy set: {:?}", proxy);
            api_client = api_client.proxy(proxy);
        }
        let mut api_config = pixivcrab::AppApiConfig::default();
        api_config.set_language(&config.pixiv.language);
        let api = pixivcrab::AppApi::new_with_config(
            pixivcrab::AuthMethod::RefreshToken(config.pixiv.refresh_token.clone()),
            api_client,
            api_config,
        )
        .context(error::PixivApi)?;
        let auth_result = api.auth().await.context(error::PixivApi)?;
        debug!("pixiv authed: {:?}", auth_result);
        info!(
            "pixiv logged in as: {} ({})",
            auth_result.user.name, auth_result.user.id
        );
        config.pixiv.refresh_token = auth_result.refresh_token.clone();
        config.save().context(error::Config)?;
        let downloader = Aria2Downloader::new(&config.aria2_path)
            .await
            .context(error::Utils)?;

        let task_config = TaskConfig {
            ffmpeg_path: check_ffmpeg(&config.ffmpeg_path).await,
            parent_dir: config.sub_dir(&config.pixiv.storage_dir),
            proxy: config.pxoxy_string(&config.pixiv.proxy_download),
        };
        let tasks_initial_permits = num_cpus::get();
        Ok(Self {
            tasks_initial_permits,
            tasks_semaphore: Arc::new(Semaphore::new(tasks_initial_permits)),
            api,
            db,
            downloader,
            task_config,
            config,
            auth_result,
            max_tries: 3,
        })
    }

    pub fn current_user_id(&self) -> &str {
        &self.auth_result.user.id
    }

    pub async fn retry_pager<T>(&self, pager: &mut Pager<T>) -> Result<Option<T>>
    where
        T: DeserializeOwned + pixivcrab::NextUrl + Debug + Send,
    {
        retry_impl!(pager.next(), self.max_tries);
    }

    pub async fn retry_api<'a, T, Fut>(&'a self, mut f: impl FnMut(&'a AppApi) -> Fut) -> Result<T>
    where
        Fut: Future<Output = std::result::Result<T, pixivcrab::error::Error>>,
    {
        retry_impl!(f(&self.api), self.max_tries);
    }

    pub async fn wait_tasks(self) {
        self.downloader.wait_and_shutdown().await;
        let _ = self
            .tasks_semaphore
            .acquire_many(self.tasks_initial_permits as u32)
            .await
            .unwrap();
    }

    pub fn spawn_limited<F>(&self, f: F)
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let semaphore = self.tasks_semaphore.clone();
        spawn(async move {
            let _permit = semaphore.acquire().await.unwrap();
            if let Err(e) = f.await {
                error!("task error: {}", e);
            }
        });
    }
}

macro_rules! generate_limiter {
    ($limit:expr, $items_sent:expr) => {
        || {
            let r = limit_reached($limit, $items_sent);
            $items_sent += 1;
            !r
        }
    };
}

async fn illusts(
    limit: Option<u32>,
    mut pager: pixivcrab::Pager<pixivcrab::models::illust::Response>,
    kit: &PixivKit,
) -> Result<()> {
    let mut users_need_update_set = BTreeSet::new();
    let mut items_sent = 0;
    let mut ugoira_map: HashMap<String, (String, Vec<i32>)> = HashMap::new();
    while let Some(r) = {
        info!("getting illusts with offset: {}", items_sent);
        kit.retry_pager(&mut pager).await?
    } {
        database::save_illusts(
            &r.illusts,
            kit,
            |u| {
                users_need_update_set.insert(u.to_string());
            },
            |sid, (url, duration)| {
                ugoira_map.insert(sid.to_string(), (url.to_string(), duration.to_vec()));
            },
        )
        .await?;
        download::download_illusts(
            &r.illusts,
            &mut ugoira_map,
            generate_limiter!(limit, items_sent),
            kit,
        )
        .await?;
        if limit_reached(limit, items_sent) {
            break;
        }
    }
    info!("{} illusts processed", items_sent);

    database::update_user_id_set(users_need_update_set, kit).await?;

    Ok(())
}

pub async fn illust_uploads(kit: &PixivKit, user_id: &str, limit: Option<u32>) -> Result<()> {
    let pager = kit.api.illust_uploads(user_id);
    illusts(limit, pager, kit).await
}

pub async fn illust_bookmarks(
    kit: &PixivKit,
    user_id: &str,
    limit: Option<u32>,
    private: bool,
) -> Result<()> {
    let pager = kit.api.illust_bookmarks(user_id, private);
    illusts(limit, pager, kit).await
}

async fn novels(
    limit: Option<u32>,
    update_exists: bool,
    mut pager: pixivcrab::Pager<pixivcrab::models::novel::Response>,
    kit: &PixivKit,
) -> Result<()> {
    let mut users_need_update_set = BTreeSet::new();
    let mut items_sent = 0;

    while let Some(r) = {
        info!("getting novels with offset: {}", items_sent);
        kit.retry_pager(&mut pager).await?
    } {
        debug!("novels: {:?}", r);
        database::save_novels(
            &r.novels,
            update_exists,
            kit,
            generate_limiter!(limit, items_sent),
            |u| {
                users_need_update_set.insert(u.to_string());
            },
        )
        .await?;
        if limit_reached(limit, items_sent) {
            break;
        }
    }
    info!("{} novels processed", items_sent);

    database::update_user_id_set(users_need_update_set, kit).await?;

    Ok(())
}

pub async fn novel_bookmarks(
    kit: &PixivKit,
    user_id: &str,
    limit: Option<u32>,
    update_exists: bool,
    private: bool,
) -> Result<()> {
    let pager = kit.api.novel_bookmarks(user_id, private);
    novels(limit, update_exists, pager, kit).await
}

pub async fn novel_uploads(
    kit: &PixivKit,
    user_id: &str,
    limit: Option<u32>,
    update_exists: bool,
) -> Result<()> {
    let pager = kit.api.novel_uploads(user_id);
    novels(limit, update_exists, pager, kit).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use bowerbird_core::config::Config;
    use std::env::var;

    fn generate_config() -> Config {
        let mut c = Config::default();
        c.pixiv.refresh_token = var("TEST_PIXIV_REFRESH_TOKEN").unwrap();
        c
    }

    #[tokio::test]
    async fn test_illusts() {
        dotenvy::dotenv().ok();
        let uid = var("TEST_PIXIV_USER_ID").unwrap();
        let db = PgPool::connect(&var("DATABASE_URL").unwrap())
            .await
            .unwrap();
        let kit = PixivKit::new(generate_config(), db).await.unwrap();
        illust_bookmarks(&kit, &uid, Some(10), false).await.unwrap();
    }
}
