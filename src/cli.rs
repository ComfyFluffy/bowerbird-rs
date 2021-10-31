use std::path::PathBuf;

use clap::Parser;
use pixivcrab::AuthMethod;
use snafu::ResultExt;

use crate::{commands, config, error};

#[derive(Parser)]
#[clap(version = env!("CARGO_PKG_VERSION"))]
struct Main {
    #[clap(short, long)]
    config: Option<String>,
    #[clap(subcommand)]
    subcommand: SubcommandMain,
}

#[derive(Parser)]
enum SubcommandMain {
    Pixiv(Pixiv),
    Init,
}

#[derive(Parser)]
struct Pixiv {
    #[clap(short, long)]
    limit: Option<u32>,
    #[clap(short, long)]
    user_id: Option<i32>,
    #[clap(subcommand)]
    subcommand: SubcommandPixiv,
}

#[derive(Parser)]
enum SubcommandPixiv {
    Illust(PixivIllust),
}

#[derive(Parser)]
struct PixivIllust {
    #[clap(subcommand)]
    subcommand: SubcommandPixivIllust,
}

#[derive(Parser)]
enum SubcommandPixivIllust {
    Bookmarks(PixivIllustBookmarks),
    Uploads,
}

#[derive(Parser)]
struct PixivIllustBookmarks {
    #[clap(long)]
    private: bool,
}

// #[derive(Parser)]
// struct PixivIllustUploads {}

pub async fn run() -> crate::Result<()> {
    let opts = Main::parse();

    let config_builder = || {
        let config_path = if let Some(c) = &opts.config {
            PathBuf::from(c)
        } else {
            dirs::home_dir().unwrap_or_default().join(".bowerbird")
        }
        .join("config.json");
        let config = config::Config::from_file(config_path)?;

        Ok(config)
    };

    let pre_fn = async {
        let config = config_builder()?;
        let db_client = mongodb::Client::with_options(
            mongodb::options::ClientOptions::parse(&config.mongodb.uri)
                .await
                .context(error::MongoDB)?,
        )
        .context(error::MongoDB)?;
        Ok((db_client.database(&config.mongodb.database_name), config))
    };

    match &opts.subcommand {
        SubcommandMain::Init => {
            config_builder()?;
        }
        SubcommandMain::Pixiv(c) => {
            let user_id = c.user_id;
            let limit = c.limit;
            let pre_fn = async {
                let (db, config) = pre_fn.await?;
                let api = pixivcrab::AppAPI::new(
                    AuthMethod::RefreshToken(config.pixiv.refresh_token.clone()),
                    &config.pixiv.language,
                )
                .context(error::PixivAPI)?;
                Ok((config, db, api))
            };
            // TODO: Save refresh_token
            match &c.subcommand {
                SubcommandPixiv::Illust(c) => {
                    let pre_fn = async {
                        let (config, db, api) = pre_fn.await?;
                        let downloader =
                            crate::downloader::Downloader::new(reqwest::Client::default(), 5);
                        Ok((config, db, api, downloader))
                    };
                    match &c.subcommand {
                        SubcommandPixivIllust::Bookmarks(c) => {
                            let (config, db, api, downloader) = pre_fn.await?;
                            commands::pixiv::illust_bookmarks(
                                &api,
                                &db,
                                &downloader,
                                config.sub_dir(&config.pixiv.storage_dir),
                                user_id,
                                c.private,
                                limit,
                            )
                            .await?;
                            downloader.wait().await;
                        }
                        SubcommandPixivIllust::Uploads => {
                            let (config, db, api, downloader) = pre_fn.await?;
                            commands::pixiv::illust_uploads(
                                &api,
                                &db,
                                &downloader,
                                config.sub_dir(&config.pixiv.storage_dir),
                                user_id,
                                limit,
                            )
                            .await?;
                            downloader.wait().await;
                        }
                    }
                }
            }
        }
    };

    Ok(())
}
