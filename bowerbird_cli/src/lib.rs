use crate::log::init_log4rs;
use ::log::{debug, error, info};
use bowerbird_core::{config::Config, migrate};
use bowerbird_pixiv::PixivKit;
use clap::Parser;
use std::path::PathBuf;

mod log;

#[derive(Parser)]
#[clap(version)]
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
    user_id: Option<String>,
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

async fn run_internal() -> anyhow::Result<()> {
    init_log4rs()?;

    let opts = Main::parse();

    let config_builder = || {
        let config_path = if let Some(c) = &opts.config {
            PathBuf::from(c)
        } else {
            dirs::home_dir().unwrap_or_default().join(".bowerbird")
        }
        .join("config.json");
        let config = Config::from_file(&config_path)?;
        debug!("config loaded: {:?}", config_path);

        Ok(config) as anyhow::Result<Config>
    };

    let pre_fn = async move {
        let config = config_builder()?;

        let db = sqlx::PgPool::connect(&config.mysql_uri).await?;

        migrate(&db).await?;

        Ok((config, db)) as anyhow::Result<_>
    };

    match opts.subcommand {
        SubcommandMain::Migrate => {
            pre_fn.await?;
            info!("migration finished");
        }
        SubcommandMain::Serve => {
            todo!()
        }
        SubcommandMain::Init => {
            config_builder()?;
        }
        SubcommandMain::Pixiv(c) => {
            let user_id = c.user_id;
            let limit = c.limit;
            let pre_fn = async move {
                let (config, db) = pre_fn.await?;
                let kit = PixivKit::new(config, db).await?;
                let target_user_id = if let Some(user_id) = user_id {
                    user_id
                } else {
                    kit.current_user_id().to_string()
                };
                Ok((kit, target_user_id)) as anyhow::Result<(PixivKit, String)>
            };
            match &c.subcommand {
                SubcommandPixiv::Illust(c) => match &c.subcommand {
                    SubcommandPixivAction::Bookmarks(c) => {
                        let (kit, target_user_id) = pre_fn.await?;
                        bowerbird_pixiv::illust_bookmarks(&target_user_id, c.private, limit, &kit)
                            .await?;
                        kit.downloader.wait_shutdown().await;
                    }
                    SubcommandPixivAction::Uploads => {
                        let (kit, target_user_id) = pre_fn.await?;
                        bowerbird_pixiv::illust_uploads(&target_user_id, limit, &kit).await?;
                        kit.downloader.wait_shutdown().await;
                    }
                },
                SubcommandPixiv::Novel(c) => {
                    let update_exists = c.update_exists;
                    match &c.subcommand {
                        SubcommandPixivAction::Bookmarks(c) => {
                            let (kit, target_user_id) = pre_fn.await?;
                            bowerbird_pixiv::novel_bookmarks(
                                &target_user_id,
                                c.private,
                                limit,
                                update_exists,
                                &kit,
                            )
                            .await?;
                        }
                        SubcommandPixivAction::Uploads => {
                            let (kit, target_user_id) = pre_fn.await?;
                            bowerbird_pixiv::novel_uploads(
                                &target_user_id,
                                limit,
                                update_exists,
                                &kit,
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

/// Run the app and return the exit code.
pub async fn run() -> i32 {
    if let Err(e) = run_internal().await {
        error!("{}", e);
        1
    } else {
        0
    }
}
