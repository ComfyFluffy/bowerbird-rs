use log::{debug, info};
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, DurationSeconds};
use snafu::{ResultExt, Snafu};
use std::{
    fs::{File, OpenOptions},
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Snafu, Debug)]
pub enum Error {
    #[snafu(display("json error in file: {source}"))]
    Json { source: serde_json::Error },

    #[snafu(display("io error with file: {source}"))]
    Io { source: std::io::Error },

    #[snafu(display("try to save without path"))]
    PathNotSet,

    #[snafu(display("cannot parse proxy from: {source}"))]
    ProxyParse { source: reqwest::Error },
}
type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct Config {
    #[serde(skip)]
    pub config_path: Option<PathBuf>,

    pub ssl_key_log: bool,
    pub postgres_uri: String,
    pub root_storage_dir: String,
    pub proxy_all: String,
    pub ffmpeg_path: String,
    pub aria2_path: String,
    pub pixiv: PixivConfig,
    pub server: ServerConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            config_path: None,
            postgres_uri: "postgresql://postgres:password@localhost/bowerbird".to_string(),
            root_storage_dir: dirs::home_dir()
                .unwrap_or_default()
                .join(".bowerbird")
                .to_string_lossy()
                .to_string(),
            proxy_all: "".to_string(),
            ffmpeg_path: "ffmpeg".to_string(),
            aria2_path: "aria2c".to_string(),
            ssl_key_log: false,
            pixiv: PixivConfig::default(),
            server: ServerConfig::default(),
        }
    }
}

#[serde_as]
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct PixivConfig {
    pub storage_dir: String,
    pub proxy_api: String,
    pub proxy_download: String,
    pub refresh_token: String,
    pub language: String,

    #[serde_as(as = "DurationSeconds<u64>")]
    pub user_need_update_interval: Duration,
    pub user_update_sleep_threshold: usize,
    #[serde_as(as = "DurationSeconds<u64>")]
    pub user_update_sleep_interval: Duration,
}

impl Default for PixivConfig {
    fn default() -> Self {
        Self {
            proxy_api: "".to_string(),
            proxy_download: "".to_string(),
            storage_dir: "pixiv".to_string(),
            refresh_token: "".to_string(),
            language: "en".to_string(),
            user_need_update_interval: chrono::Duration::days(7).to_std().unwrap(),
            user_update_sleep_threshold: 100,
            user_update_sleep_interval: Duration::from_secs(1),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct ServerConfig {
    pub listen_addr: SocketAddr,
    pub thumbnail_jpeg_quality: u8,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            listen_addr: "127.0.0.1:5000".parse().unwrap(),
            thumbnail_jpeg_quality: 85,
        }
    }
}

impl Config {
    pub fn from_file(path: impl AsRef<Path>) -> Result<Config> {
        debug!("loading config from: {:?}", path.as_ref());
        let path = path.as_ref();

        if !path.exists() {
            info!("creating config file: {}", path.to_string_lossy());
            let defaults = Config {
                config_path: Some(path.to_owned()),
                ..Default::default()
            };

            defaults.save()?;
            return Ok(defaults);
        }

        let file = File::open(path).context(IoSnafu)?;
        let mut config_loaded: Config = serde_json::from_reader(file).context(JsonSnafu)?;
        config_loaded.config_path = Some(PathBuf::from(path));
        config_loaded.save()?;
        Ok(config_loaded)
    }

    pub fn save(&self) -> Result<()> {
        if let Some(path) = &self.config_path {
            if let Some(p) = path.parent() {
                std::fs::create_dir_all(p).context(IoSnafu)?;
            }
            let file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(path)
                .context(IoSnafu)?;
            serde_json::to_writer_pretty(file, &self).context(JsonSnafu)?;
            return Ok(());
        }
        PathNotSetSnafu.fail()
    }

    /// If the given directory is relative, join the given path to the root storage directory
    /// and return the joined path.
    /// If the given directory is absolute, return it as is.
    pub fn sub_dir(&self, dir: impl AsRef<Path>) -> PathBuf {
        let dir = dir.as_ref();
        if dir.is_relative() {
            PathBuf::from(&self.root_storage_dir).join(dir)
        } else {
            dir.to_owned()
        }
    }

    pub fn pxoxy(&self, url: &str) -> Result<Option<reqwest::Proxy>> {
        use reqwest::Proxy;
        if !url.is_empty() {
            Ok(Some(Proxy::all(url).context(ProxyParseSnafu)?))
        } else if !self.proxy_all.is_empty() {
            Ok(Some(Proxy::all(&self.proxy_all).context(ProxyParseSnafu)?))
        } else {
            Ok(None)
        }
    }

    pub fn pxoxy_string(&self, url: &str) -> Option<String> {
        if !url.is_empty() {
            Some(url.to_string())
        } else if !self.proxy_all.is_empty() {
            Some(self.proxy_all.clone())
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_log::test;

    #[test]
    fn test_save_defaults_error() {
        // should raise PathNotSet error
        let config = Config::default();
        assert!(config.save().is_err());
    }

    #[test]
    fn test_save_and_load_tempdir() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("config.json");
        let config = Config {
            config_path: Some(path.clone()),
            aria2_path: "xxxxx".to_string(),
            ..Default::default()
        };
        config.save().unwrap();
        let config_loaded = Config::from_file(path).unwrap();
        assert_eq!(config, config_loaded);
    }

    #[test]
    fn test_from_nonexist_file() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("config.json");
        let mut config = Config::from_file(path).unwrap();
        config.config_path = None;
        assert_eq!(config, Config::default());
    }

    #[test]
    fn test_sub_dir_rel_abs() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("config.json");
        let config = Config {
            config_path: Some(path),
            root_storage_dir: "/tmp".to_string(),
            ..Default::default()
        };
        let sub_dir = config.sub_dir("/another/xxxx");
        assert_eq!(sub_dir, PathBuf::from("/another/xxxx"));
        let rel_sub_dir = config.sub_dir("rel/xxxx");
        assert_eq!(rel_sub_dir, PathBuf::from("/tmp/rel/xxxx"));
    }

    #[test]
    fn test_proxy() {
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join("config.json");
        let mut config = Config {
            config_path: Some(path),
            proxy_all: "http://127.0.0.1:1080".to_string(),
            ..Default::default()
        };

        assert_eq!(config.pxoxy_string("").unwrap(), config.proxy_all);
        assert!(config.pxoxy("").unwrap().is_some());

        assert_eq!(
            config
                .pxoxy_string("http://127.0.0.1:3242")
                .unwrap()
                .as_str(),
            "http://127.0.0.1:3242"
        );
        assert!(config.pxoxy("http://127.0.0.1:3242").unwrap().is_some());

        config.proxy_all = "".to_string();
        assert_eq!(
            config
                .pxoxy_string("http://127.0.0.1:3242")
                .unwrap()
                .as_str(),
            "http://127.0.0.1:3242"
        );
        assert!(config.pxoxy("").unwrap().is_none());
    }
}
