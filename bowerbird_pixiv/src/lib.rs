use bowerbird_core::config::Config;
use bowerbird_utils::{check_ffmpeg, downloader::Aria2Downloader};
use log::{debug, info, warn};
use pixivcrab::AppApi;
use reqwest::ClientBuilder;
use snafu::ResultExt;
use sqlx::PgPool;
use std::{
    collections::{BTreeSet, HashMap},
    env::var,
    path::PathBuf,
};

pub mod database;
pub mod download;
mod error;
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

// pub(crate) struct LocalCache {
//     pub user_to_id: HashMap<String, i64>,
//     pub tag_to_id: HashMap<String, i64>,
// }

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
}

impl PixivKit {
    /// Log in to pixiv, save token, start aria2, and check ffmpeg.
    pub async fn new(mut config: Config, db: PgPool) -> crate::Result<Self> {
        let mut api_client = ClientBuilder::new();
        if let Some(proxy) = config
            .pxoxy(&config.pixiv.proxy_api)
            .context(error::Config)?
        {
            debug!("pixiv api proxy set: {:?}", proxy);
            api_client = api_client.proxy(proxy);
        }
        if var("BOWERBIRD_ACCEPT_INVALID_CERTS").is_ok() {
            warn!("invalid certs will be accepted for pixiv api requests");
            api_client = api_client.danger_accept_invalid_certs(true);
        }
        let api = pixivcrab::AppApi::new(
            pixivcrab::AuthMethod::RefreshToken(config.pixiv.refresh_token.clone()),
            &config.pixiv.language,
            api_client,
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
        Ok(Self {
            api,
            db,
            downloader,
            task_config,
            config,
            auth_result,
        })
    }

    pub fn current_user_id(&self) -> &str {
        &self.auth_result.user.id
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
) -> crate::Result<()> {
    let mut users_need_update_set = BTreeSet::new();
    let mut items_sent = 0;
    let mut ugoira_map: HashMap<String, (String, Vec<i32>)> = HashMap::new();
    while let Some(r) = {
        info!("getting illusts with offset: {}", items_sent);
        utils::retry_pager(&mut pager, 3).await?
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

pub async fn illust_uploads(
    user_id: &str,
    limit: Option<u32>,
    kit: &PixivKit,
) -> crate::Result<()> {
    let pager = kit.api.illust_uploads(user_id);
    illusts(limit, pager, kit).await
}

pub async fn illust_bookmarks(
    user_id: &str,
    private: bool,
    limit: Option<u32>,
    kit: &PixivKit,
) -> crate::Result<()> {
    let pager = kit.api.illust_bookmarks(user_id, private);
    illusts(limit, pager, kit).await
}

async fn novels(
    limit: Option<u32>,
    update_exists: bool,
    mut pager: pixivcrab::Pager<pixivcrab::models::novel::Response>,
    kit: &PixivKit,
) -> crate::Result<()> {
    let mut users_need_update_set = BTreeSet::new();
    let mut items_sent = 0;

    while let Some(r) = {
        info!("getting novels with offset: {}", items_sent);
        utils::retry_pager(&mut pager, 3).await?
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
    user_id: &str,
    private: bool,
    limit: Option<u32>,
    update_exists: bool,
    kit: &PixivKit,
) -> crate::Result<()> {
    let pager = kit.api.novel_bookmarks(user_id, private);
    novels(limit, update_exists, pager, kit).await
}

pub async fn novel_uploads(
    user_id: &str,
    limit: Option<u32>,
    update_exists: bool,
    kit: &PixivKit,
) -> crate::Result<()> {
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
        bowerbird_cli::log::init_log4rs().unwrap();
        let uid = var("TEST_PIXIV_USER_ID").unwrap();
        let db = PgPool::connect(&var("DATABASE_URL").unwrap())
            .await
            .unwrap();
        let kit = PixivKit::new(generate_config(), db).await.unwrap();
        illust_bookmarks(&uid, false, Some(10), &kit).await.unwrap();
    }
}
