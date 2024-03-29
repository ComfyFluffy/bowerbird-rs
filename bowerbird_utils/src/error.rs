use snafu::Snafu;
use std::{path::PathBuf, process::ExitStatus};

#[derive(Snafu, Debug)]
#[snafu(visibility(pub(crate)))]
#[snafu(context(suffix(false)))]
pub enum Error {
    #[snafu(display("aria2: {source}"))]
    Aria2 { source: aria2_ws::Error },

    #[snafu(display("aria2 startup: {source}"))]
    Aria2StartUpIo { source: std::io::Error },

    #[snafu(display("aria2 exited with code: {status}"))]
    Aria2EarlyExited { status: ExitStatus },

    #[snafu(display("aria2 exit: {source}"))]
    Aria2ExitIo { source: std::io::Error },

    #[snafu(display("fail to find avalible port: {message}"))]
    NoAvaliablePort { message: String },

    #[snafu(display("color thief: {source}"))]
    ColorTheifError { source: color_thief::Error },

    #[snafu(display("image: {source}"))]
    ImageError { source: image::ImageError },

    #[snafu(display("loading system certifcates: {source}"))]
    LoadCertsIo { source: std::io::Error },

    #[snafu(display("parsing system certifcates: {source}"))]
    RustlsParseCerts { source: webpki::Error },

    #[snafu(display("ffmpeg (path: {path:?}) not found, some functions may not work: {source}"))]
    FFmpegNotFound {
        source: std::io::Error,
        path: PathBuf,
    },
}
