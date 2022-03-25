pub mod cli;
mod command;
mod config;
mod downloader;
mod error;
pub mod model;
mod server;
mod utils;

pub(crate) type Result<T> = std::result::Result<T, error::Error>;

pub use error::Error;
