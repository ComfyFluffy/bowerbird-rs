use snafu::Snafu;
use std::process::ExitStatus;

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
}
