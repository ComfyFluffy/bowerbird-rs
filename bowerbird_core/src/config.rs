use log::{info, warn};
use serde::{Deserialize, Serialize};
use snafu::{ResultExt, Snafu};
use std::{
    fs::{File, OpenOptions},
    net::SocketAddr,
    path::{Path, PathBuf},
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
    config_path: Option<PathBuf>,

    pub mysql_uri: String,
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
            mysql_uri: "mysql://root:password@localhost:3306/bowerbird".to_string(),
            root_storage_dir: dirs::home_dir()
                .unwrap_or_default()
                .join(".bowerbird")
                .to_string_lossy()
                .to_string(),
            proxy_all: "".to_string(),
            ffmpeg_path: "".to_string(),
            aria2_path: "aria2c".to_string(),
            pixiv: PixivConfig::default(),
            server: ServerConfig::default(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct PixivConfig {
    pub storage_dir: String,
    pub proxy_api: String,
    pub proxy_download: String,
    pub refresh_token: String,
    pub language: String,
}

impl Default for PixivConfig {
    fn default() -> Self {
        Self {
            proxy_api: "".to_string(),
            proxy_download: "".to_string(),
            storage_dir: "pixiv".to_string(),
            refresh_token: "".to_string(),
            language: "en".to_string(),
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
        let path = path.as_ref();
        if !path.exists() {
            info!("creating config file: {}", path.to_string_lossy());
            let defaults = Config {
                config_path: Some(path.to_owned()),
                ..Default::default()
            };

            defaults.save()?;
            Ok(defaults)
        } else {
            let file = File::open(&path).context(IoSnafu)?;
            let mut config_loaded: Config = serde_json::from_reader(file).context(JsonSnafu)?;
            config_loaded.config_path = Some(PathBuf::from(path));
            config_loaded.save()?;
            Ok(config_loaded)
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = self
            .config_path
            .as_ref()
            .ok_or_else(|| PathNotSetSnafu.build())?;
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).context(IoSnafu)?;
        }
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .context(IoSnafu)?;
        serde_json::to_writer_pretty(file, &self).context(JsonSnafu)
    }

    pub fn sub_dir(&self, dir: impl AsRef<Path>) -> PathBuf {
        let dir = dir.as_ref();
        if dir.is_relative() {
            let rel = PathBuf::from(&self.root_storage_dir).join(dir);
            match rel.canonicalize() {
                Ok(abs) => abs,
                Err(e) => {
                    warn!(
                        "cannot canonicalize path: {}, error: {}",
                        rel.to_string_lossy(),
                        e
                    );
                    rel
                }
            }
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
        if url.is_empty() {
            if self.proxy_all.is_empty() {
                None
            } else {
                Some(self.proxy_all.clone())
            }
        } else {
            Some(self.proxy_all.clone())
        }
    }
}
