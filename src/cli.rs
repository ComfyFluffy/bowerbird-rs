use std::{
    path::PathBuf,
    process::{self, Command},
};

use clap::Parser;

use snafu::{ErrorCompat, ResultExt};

use crate::{
    commands, config, error,
    log::{error, warning},
};

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
    Migrate,
    Serve,
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
    Novel(PixivNovel),
}

#[derive(Parser)]
struct PixivIllust {
    #[clap(subcommand)]
    subcommand: SubcommandPixivAction,
}

#[derive(Parser)]
struct PixivNovel {
    #[clap(long)]
    update_exists: bool,
    #[clap(subcommand)]
    subcommand: SubcommandPixivAction,
}

#[derive(Parser)]
enum SubcommandPixivAction {
    Bookmarks(PixivBookmarks),
    Uploads,
}

#[derive(Parser)]
struct PixivBookmarks {
    #[clap(long)]
    private: bool,
}

async fn run_internal() -> crate::Result<()> {
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
        config.set_log_level();
        let db_client = mongodb::Client::with_options(
            mongodb::options::ClientOptions::parse(&config.mongodb.uri)
                .await
                .context(error::MongoDB)?,
        )
        .context(error::MongoDB)?;

        let ffmpeg_path = if config.ffmpeg_path.is_empty() {
            PathBuf::from("ffmpeg")
        } else {
            PathBuf::from(&config.ffmpeg_path)
        };
        let mut ffmpeg = Command::new(&ffmpeg_path);
        ffmpeg.args(["-hide_banner", "-loglevel", "error"]);
        let ffmpeg_exists = ffmpeg.spawn().is_ok();
        if !ffmpeg_exists {
            warning!(
                "FFmpeg not found, some functions will not work: {}",
                ffmpeg_path.to_string_lossy()
            );
        }
        let ffmpeg_path = if ffmpeg_exists {
            Some(ffmpeg_path)
        } else {
            None
        };

        let db = db_client.database(&config.mongodb.database_name);

        Ok((config, ffmpeg_path, db))
    };

    match &opts.subcommand {
        SubcommandMain::Migrate => {}
        SubcommandMain::Serve => {
            let (config, _, db) = pre_fn.await?;
            crate::server::run(db, config).await?;
        }
        SubcommandMain::Init => {
            config_builder()?;
        }
        SubcommandMain::Pixiv(c) => {
            use pixivcrab::AuthMethod;
            let user_id = c.user_id;
            let limit = c.limit;
            let pre_fn = async {
                let (mut config, ffmpeg_path, db) = pre_fn.await?;
                commands::pixiv::database::create_indexes(&db).await?;
                let mut api_client = reqwest::ClientBuilder::new();
                if let Some(proxy) = config.pxoxy(&config.pixiv.proxy_api)? {
                    api_client = api_client.proxy(proxy);
                }
                if let Ok(_) = std::env::var("BOWERBIRD_ACCEPT_INVALID_CERTS") {
                    api_client = api_client.danger_accept_invalid_certs(true);
                }
                let api = pixivcrab::AppAPI::new(
                    AuthMethod::RefreshToken(config.pixiv.refresh_token.clone()),
                    &config.pixiv.language,
                    api_client,
                )
                .context(error::PixivAPI)?;
                let auth_result = api.auth().await.context(error::PixivAPI)?;
                config.pixiv.refresh_token = auth_result.refresh_token;
                config.save()?;
                let selected_user_id = user_id.map_or(auth_result.user.id, |i| i.to_string());
                Ok((config, ffmpeg_path, db, api, selected_user_id))
            };
            match &c.subcommand {
                SubcommandPixiv::Illust(c) => {
                    let pre_fn = async {
                        let (config, ffmpeg_path, db, api, selected_user_id) = pre_fn.await?;
                        let mut downloader_client = reqwest::ClientBuilder::new();
                        if let Some(proxy) = config.pxoxy(&config.pixiv.proxy_download)? {
                            downloader_client = downloader_client.proxy(proxy);
                        }
                        let downloader = crate::downloader::Downloader::new(
                            downloader_client.build().context(error::DownloadHTTP)?,
                            5,
                        );
                        Ok((config, ffmpeg_path, db, api, selected_user_id, downloader))
                    };
                    match &c.subcommand {
                        SubcommandPixivAction::Bookmarks(c) => {
                            let (config, ffmpeg_path, db, api, selected_user_id, downloader) =
                                pre_fn.await?;
                            commands::pixiv::illust_bookmarks(
                                &api,
                                &db,
                                &downloader,
                                config.sub_dir(&config.pixiv.storage_dir),
                                &selected_user_id,
                                c.private,
                                limit,
                                &ffmpeg_path,
                            )
                            .await?;
                            downloader.wait().await;
                        }
                        SubcommandPixivAction::Uploads => {
                            let (config, ffmpeg_path, db, api, selected_user_id, downloader) =
                                pre_fn.await?;
                            commands::pixiv::illust_uploads(
                                &api,
                                &db,
                                &downloader,
                                config.sub_dir(&config.pixiv.storage_dir),
                                &selected_user_id,
                                limit,
                                &ffmpeg_path,
                            )
                            .await?;
                            downloader.wait().await;
                        }
                    }
                }
                SubcommandPixiv::Novel(c) => {
                    let update_exists = c.update_exists;
                    match &c.subcommand {
                        SubcommandPixivAction::Bookmarks(c) => {
                            let (_, _, db, api, selected_user_id) = pre_fn.await?;
                            commands::pixiv::novel_bookmarks(
                                &api,
                                &db,
                                update_exists,
                                &selected_user_id,
                                c.private,
                                limit,
                            )
                            .await?;
                        }
                        SubcommandPixivAction::Uploads => {
                            let (_, _, db, api, selected_user_id) = pre_fn.await?;
                            commands::pixiv::novel_uploads(
                                &api,
                                &db,
                                update_exists,
                                &selected_user_id,
                                limit,
                            )
                            .await?;
                        }
                    };
                }
            }
        }
    };

    Ok(())
}

pub async fn run() {
    match run_internal().await {
        Err(e) => {
            error!(e);
            process::exit(1);
        }
        _ => {}
    };
}
