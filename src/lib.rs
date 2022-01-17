#![feature(map_first_last)]
// #![feature(drain_filter)]
pub mod cli;
mod command;
mod config;
mod downloader;
mod error;
mod log;
pub mod model;
mod server;
mod utils;
pub(crate) type Result<T> = std::result::Result<T, error::Error>;
pub use error::Error;
