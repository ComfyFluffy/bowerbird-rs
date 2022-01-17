use std::process::ExitStatus;

use snafu::Snafu;
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

#[derive(Snafu, Debug)]
#[snafu(visibility(pub(crate)))]
#[snafu(context(suffix(false)))]
pub enum Error {
    #[snafu(display("json error in config file: {}", source))]
    ConfigJSON {
        source: serde_json::Error,
    },
    #[snafu(display("io error with config file: {}", source))]
    ConfigIO {
        source: std::io::Error,
    },
    #[snafu(display("try to save config without path"))]
    ConfigPathNotSet,
    #[snafu(display("cannot parse proxy in config file: {}", source))]
    ProxyParse {
        source: reqwest::Error,
    },
    #[snafu(display("pixiv api error: {}", source))]
    PixivAPI {
        source: pixivcrab::error::Error,
    },
    #[snafu(display("cannot parse infomation from pixiv: {}", message))]
    PixivParse {
        message: String,
    },
    PixivParseURL {
        source: url::ParseError,
    },
    #[snafu(display("error on mongodb: {}", source))]
    MongoDB {
        source: mongodb::error::Error,
    },
    #[snafu(display("cannot parse result from mongodb: {}", source))]
    MongoValueAccess {
        source: mongodb::bson::document::ValueAccessError,
    },
    #[snafu(display("data struct cannot be parsed from mongodb"))]
    MongoNotMatch,
    #[snafu(display("error while serializing to bson: {}", source))]
    BsonSerialize {
        source: mongodb::bson::ser::Error,
    },
    #[snafu(display("server error: {}", source))]
    Rocket {
        source: rocket::Error,
    },
    #[snafu(display("aria2 error: {}", source))]
    Aria2 {
        source: aria2_ws::Error,
    },
    #[snafu(display("aria2 startup error: {}", source))]
    Aria2StartUpIO {
        source: std::io::Error,
    },
    #[snafu(display("aria2 startup error with code: {}", status))]
    Aria2StartUpExit {
        status: ExitStatus,
    },
}
