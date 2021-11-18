use serde::{Deserialize, Serialize};
use snafu::ResultExt;
use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
};

use crate::{
    error,
    log::{info, LogLevel},
};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct Config {
    #[serde(skip)]
    config_path: Option<PathBuf>,

    pub log_file: String,
    pub log_level: String,
    pub root_storage_dir: String,
    pub mongodb: MongoDBConfig,
    pub pixiv: PixivConfig,
    pub proxy_all: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            config_path: None,
            log_file: "".to_string(),
            log_level: "debug".to_string(),
            root_storage_dir: dirs::home_dir()
                .unwrap_or_default()
                .join(".bowerbird")
                .to_string_lossy()
                .to_string(),
            mongodb: MongoDBConfig::default(),
            pixiv: PixivConfig::default(),
            proxy_all: "".to_string(),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct MongoDBConfig {
    pub uri: String,
    pub database_name: String,
}

impl Default for MongoDBConfig {
    fn default() -> Self {
        Self {
            database_name: "bowerbird".to_string(),
            uri: "mongodb://localhost/bowerbird".to_string(),
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
    pub ignore_certificate: bool,
}

impl Default for PixivConfig {
    fn default() -> Self {
        Self {
            proxy_api: "".to_string(),
            proxy_download: "".to_string(),
            storage_dir: "pixiv".to_string(),
            refresh_token: "".to_string(),
            language: "en".to_string(),
            ignore_certificate: false,
        }
    }
}

impl Config {
    pub fn from_file<P: AsRef<Path>>(path: P) -> crate::Result<Config> {
        if !path.as_ref().exists() {
            info!("Creating config file: {}", path.as_ref().to_string_lossy());
            let mut defaults = Config::default();
            defaults.config_path = Some(path.as_ref().to_owned());
            defaults.save()?;
            Ok(defaults)
        } else {
            let file = File::open(&path).context(error::ConfigIO)?;
            let mut config_loaded: Config =
                serde_json::from_reader(file).context(error::ConfigJSON)?;
            config_loaded.config_path = Some(PathBuf::from(path.as_ref()));
            config_loaded.save()?;
            Ok(config_loaded)
        }
    }

    pub fn save(&self) -> crate::Result<()> {
        let path = self
            .config_path
            .as_ref()
            .ok_or(error::DownloadPathNotSet.build())?;
        if let Some(p) = path.parent() {
            std::fs::create_dir_all(p).context(error::ConfigIO)?;
        }
        let file = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .context(error::ConfigIO)?;
        serde_json::to_writer_pretty(file, &self).context(error::ConfigJSON)
    }

    pub fn sub_dir<P: AsRef<Path>>(&self, dir: P) -> PathBuf {
        if dir.as_ref().is_relative() {
            PathBuf::from(&self.root_storage_dir).join(dir)
        } else {
            dir.as_ref().to_owned()
        }
    }

    pub fn log_level(&self) -> Option<LogLevel> {
        Some(match self.log_level.to_ascii_lowercase().as_str() {
            "debug" => LogLevel::Debug,
            "info" => LogLevel::Info,
            "warn" | "warning" => LogLevel::Warn,
            "error" => LogLevel::Error,
            _ => return None,
        })
    }

    pub fn pxoxy(&self, url: &str) -> crate::Result<Option<reqwest::Proxy>> {
        use reqwest::Proxy;
        if !url.is_empty() {
            Ok(Some(Proxy::all(url).context(error::ProxyParse)?))
        } else if !self.proxy_all.is_empty() {
            Ok(Some(
                Proxy::all(&self.proxy_all).context(error::ProxyParse)?,
            ))
        } else {
            Ok(None)
        }
    }
}
