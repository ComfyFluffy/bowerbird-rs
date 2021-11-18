#![feature(map_first_last)]
#![feature(drain_filter)]
pub mod cli;
mod commands;
mod config;
mod downloader;
pub mod error;
mod log;
pub mod models;
pub(crate) type Result<T> = std::result::Result<T, error::Error>;
