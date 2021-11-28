use std::fmt;

use rocket::http::Status;
use rocket::serde::json::Json;
use serde::{Deserialize, Serialize};

pub use crate::error::*;
use crate::{downloader::BoxError, log::warning};

#[derive(Serialize, Deserialize, Debug)]
pub struct ErrorResponse {
    pub message: String,
    #[serde(skip)]
    pub status: Status,
    #[serde(skip)]
    pub source: Option<BoxError>,
}

impl fmt::Display for ErrorResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}
impl std::error::Error for ErrorResponse {}

use rocket::response::{self, Responder};

// #[rocket::async_trait]
impl<'r> Responder<'r, 'static> for ErrorResponse {
    fn respond_to(self, request: &'r rocket::request::Request<'_>) -> response::Result<'static> {
        let status = self.status;
        warning!("{}", self.message);
        match Json(self).respond_to(request) {
            Ok(mut r) => {
                r.set_status(status);
                Ok(r)
            }
            Err(s) => Err(s),
        }
    }
}

pub trait ServerErrorExt<T, E>
where
    E: std::error::Error + Send + Sync + 'static,
    Self: Sized,
{
    fn with_msg(self, status: Status, message: &str) -> Result<T, ErrorResponse>;

    fn with_msg_source(self, status: Status, message: &str) -> Result<T, ErrorResponse>;

    fn with_status(self, status: Status) -> Result<T, ErrorResponse>;
}

impl<T, E> ServerErrorExt<T, E> for Result<T, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    fn with_msg(self, status: Status, message: &str) -> Result<T, ErrorResponse> {
        self.map_err(|err| ErrorResponse::generate(status, message, err, false))
    }

    fn with_msg_source(self, status: Status, message: &str) -> Result<T, ErrorResponse> {
        self.map_err(|err| ErrorResponse::generate(status, message, err, true))
    }

    fn with_status(self, status: Status) -> Result<T, ErrorResponse> {
        self.map_err(|err| ErrorResponse::generate(status, "", err, true))
    }
}

impl ErrorResponse {
    pub fn generate<E: std::error::Error + Send + Sync + 'static>(
        status: Status,
        message: &str,
        source: E,
        print_source: bool,
    ) -> ErrorResponse {
        ErrorResponse {
            status,
            message: if print_source {
                if !message.is_empty() {
                    format!("{}: {}", message, source)
                } else {
                    source.to_string()
                }
            } else {
                source.to_string()
            },
            source: Some(Box::new(source)),
        }
    }
}
