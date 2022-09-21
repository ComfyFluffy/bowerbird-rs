use snafu::Snafu;

#[derive(Snafu, Debug)]
#[snafu(visibility(pub(crate)))]
#[snafu(context(suffix(false)))]
pub enum Error {
    #[snafu(display("pixiv app-api: {source}"))]
    PixivApi { source: pixivcrab::error::Error },

    #[snafu(display("unknown data from pixiv: {message}"))]
    UnknownData { message: String },

    #[snafu(display("database: {source}"))]
    Database { source: sqlx::Error },

    #[snafu(display("config: {source}"))]
    Config {
        source: bowerbird_core::config::Error,
    },

    #[snafu(display("utils: {source}"))]
    Utils {
        source: bowerbird_utils::error::Error,
    },
}
