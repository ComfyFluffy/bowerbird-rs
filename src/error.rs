use snafu::{Backtrace, Snafu};

#[derive(Snafu, Debug)]
#[snafu(visibility = "pub")]
pub enum Error {
    #[snafu(display("JSON error with config file: {}", source))]
    ConfigJSON {
        source: serde_json::Error,
        backtrace: Backtrace,
    },
    #[snafu(display("IO error with config file: {}", source))]
    ConfigIO {
        source: std::io::Error,
        backtrace: Backtrace,
    },
    #[snafu(display("pixiv api error: {}", source))]
    PixivAPI {
        source: pixivcrab::error::Error,
        backtrace: Backtrace,
    },
    PixivParse {
        backtrace: Backtrace,
    },
    PixivParseURL {
        source: url::ParseError,
        backtrace: Backtrace,
    },
    DownloadNotInitialized {
        backtrace: Backtrace,
    },
    DownloadHTTP {
        source: reqwest::Error,
        backtrace: Backtrace,
    },
    DownloadHTTPStatus {
        status: reqwest::StatusCode,
        response: bytes::Bytes,
        backtrace: Backtrace,
    },
    DownloadIO {
        source: std::io::Error,
        backtrace: Backtrace,
    },
    DownloadPathNotSet {
        backtrace: Backtrace,
    },
    DownloadPathNotAbsolute {
        backtrace: Backtrace,
    },
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
}
