use snafu::{Backtrace, Snafu};

#[derive(Snafu, Debug)]
#[snafu(visibility = "pub")]
pub enum Error {
    #[snafu(display("JSON error in config file: {}", source))]
    ConfigJSON {
        source: serde_json::Error,
        backtrace: Backtrace,
    },
    #[snafu(display("IO error with config file: {}", source))]
    ConfigIO {
        source: std::io::Error,
        backtrace: Backtrace,
    },
    #[snafu(display("try to save config without path"))]
    ConfigPathNotSet {
        backtrace: Backtrace,
    },
    #[snafu(display("Cannot parse proxy in config file: {}", source))]
    ProxyParse {
        source: reqwest::Error,
        backtrace: Backtrace,
    },
    #[snafu(display("pixiv api error: {}", source))]
    PixivAPI {
        source: pixivcrab::error::Error,
        backtrace: Backtrace,
    },
    #[snafu(display("Cannot parse infomation from pixiv: {}", message))]
    PixivParse {
        message: String,
        backtrace: Backtrace,
    },
    PixivParseURL {
        source: url::ParseError,
        backtrace: Backtrace,
    },
    #[snafu(display("HTTP error while downloading: {}", source))]
    DownloadHTTP {
        source: reqwest::Error,
        backtrace: Backtrace,
    },
    #[snafu(display("HTTP status is not OK while downloading: {}: {}", status, response))]
    DownloadHTTPStatus {
        status: reqwest::StatusCode,
        response: String,
        backtrace: Backtrace,
    },
    #[snafu(display("IO error while downloading: {}", source))]
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
    MongoDB {
        source: mongodb::error::Error,
        backtrace: Backtrace,
    },
    MongoValueAccess {
        source: mongodb::bson::document::ValueAccessError,
        backtrace: Backtrace,
    },
    MongoNotMatch {
        backtrace: Backtrace,
    },
    MongoBsonConvert {
        source: mongodb::bson::ser::Error,
        backtrace: Backtrace,
    },
    Image {
        source: image::ImageError,
        backtrace: Backtrace,
    },
    ImageColorThief {
        source: color_thief::Error,
        backtrace: Backtrace,
    },
    UgoiraToVideo {
        backtrace: Backtrace,
    },
}
