use snafu::Snafu;
use std::process::ExitStatus;

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Snafu, Debug)]
#[snafu(visibility(pub(crate)))]
#[snafu(context(suffix(false)))]
pub enum Error {
    #[snafu(display("json error in config file: {source}"))]
    ConfigJson {
        source: serde_json::Error,
    },
    #[snafu(display("io error with config file: {source}"))]
    ConfigIo {
        source: std::io::Error,
    },
    #[snafu(display("try to save config without path"))]
    ConfigPathNotSet,
    #[snafu(display("cannot parse proxy in config file: {source}"))]
    ProxyParse {
        source: reqwest::Error,
    },
    #[snafu(display("pixiv api error: {source}"))]
    PixivApi {
        source: pixivcrab::error::Error,
    },
    #[snafu(display("cannot parse infomation from pixiv: {message}"))]
    PixivParse {
        message: String,
    },
    PixivParseUrl {
        source: url::ParseError,
    },
    #[snafu(display("error on mongodb: {source}"))]
    MongoDb {
        source: mongodb::error::Error,
    },
    #[snafu(display("cannot parse result from mongodb: {source}"))]
    MongoValueAccess {
        source: mongodb::bson::document::ValueAccessError,
    },
    #[snafu(display("data struct cannot be parsed from mongodb"))]
    MongoNotMatch,
    #[snafu(display("error while serializing to bson: {source}"))]
    BsonSerialize {
        source: mongodb::bson::ser::Error,
    },
    #[snafu(display("aria2 error: {source}"))]
    Aria2 {
        source: aria2_ws::Error,
    },
    #[snafu(display("aria2 startup error: {source}"))]
    Aria2StartUpIo {
        source: std::io::Error,
    },
    #[snafu(display("aria2 startup error with code: {status}"))]
    Aria2EarlyExited {
        status: ExitStatus,
    },
    #[snafu(display("aria2 exit error: {source}"))]
    Aria2ExitIo {
        source: std::io::Error,
    },
    #[snafu(display("fail to find avalible port: {message}"))]
    NoAvaliablePort {
        message: String,
    },
    #[snafu(display("fail to start server: {source}"))]
    ServerIo {
        source: std::io::Error,
    },
    #[snafu(display("The database schema needs to be updated running `bowerbird migrate`. Backup is recommended before migration."))]
    MigrationRequired,
    #[snafu(display("The database schema is newer than this version of bowerbird. Please update to the latest version."))]
    DatabaseIsNewer,
}
