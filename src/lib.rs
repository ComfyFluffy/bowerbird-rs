#![feature(map_first_last)]
#![feature(drain_filter)]
pub mod cli;
pub mod commands;
pub mod config;
pub mod downloader;
pub mod error;
pub mod log;
pub mod models;
pub(crate) type Result<T> = std::result::Result<T, error::Error>;
