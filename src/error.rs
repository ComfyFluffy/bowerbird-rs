use snafu::{Backtrace, Snafu};

#[derive(Snafu, Debug)]
#[snafu(visibility = "pub")]
pub enum Error {
    #[snafu(display("json error in config file: {}", source))]
    ConfigJSON {
        source: serde_json::Error,
        backtrace: Backtrace,
    },
    #[snafu(display("io error with config file: {}", source))]
    ConfigIO {
        source: std::io::Error,
        backtrace: Backtrace,
    },
    #[snafu(display("try to save config without path"))]
    ConfigPathNotSet { backtrace: Backtrace },
    #[snafu(display("cannot parse proxy in config file: {}", source))]
    ProxyParse {
        source: reqwest::Error,
        backtrace: Backtrace,
    },
    #[snafu(display("pixiv api error: {}", source))]
    PixivAPI {
        source: pixivcrab::error::Error,
        backtrace: Backtrace,
    },
    #[snafu(display("cannot parse infomation from pixiv: {}", message))]
    PixivParse {
        message: String,
        backtrace: Backtrace,
    },
    PixivParseURL {
        source: url::ParseError,
        backtrace: Backtrace,
    },
    #[snafu(display("http error while downloading: {}", source))]
    DownloadHTTP {
        source: reqwest::Error,
        backtrace: Backtrace,
    },
    #[snafu(display("http status is not OK while downloading: {}: {}", status, response))]
    DownloadHTTPStatus {
        status: reqwest::StatusCode,
        response: String,
        backtrace: Backtrace,
    },
    #[snafu(display("io error while downloading: {}", source))]
    DownloadIO {
        source: std::io::Error,
        backtrace: Backtrace,
    },
    #[snafu(display("downloader task has empty target path: {}", message))]
    DownloadPathNotSet {
        message: String,
        backtrace: Backtrace,
    },
    #[snafu(display("downloader task do not have an absolute path: {}", message))]
    DownloadPathNotAbsolute {
        message: String,
        backtrace: Backtrace,
    },
    #[snafu(display("Error while building download request: {}", source))]
    DownloadRequestBuild {
        source: reqwest::Error,
        backtrace: Backtrace,
    },
    #[snafu(display("error on download hook `{}`: {}", hook, source))]
    DownloadHook {
        hook: &'static str,
        source: crate::downloader::BoxError,
    },
    #[snafu(display("error on mongodb: {}", source))]
    MongoDB {
        source: mongodb::error::Error,
        backtrace: Backtrace,
    },
    #[snafu(display("got value that cannot be parsed from mongodb: {}", source))]
    MongoValueAccess {
        source: mongodb::bson::document::ValueAccessError,
        backtrace: Backtrace,
    },
    #[snafu(display("data struct cannot be parsed from mongodb"))]
    MongoNotMatch { backtrace: Backtrace },
    #[snafu(display("Error while serializing to bson: {}", source))]
    BsonSerialize {
        source: mongodb::bson::ser::Error,
        backtrace: Backtrace,
    },
    #[snafu(display("error running server: {}", source))]
    Rocket { source: rocket::Error },
}
