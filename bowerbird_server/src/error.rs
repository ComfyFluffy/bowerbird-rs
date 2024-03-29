use actix_web::http::StatusCode;
use log::error;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Debug};

#[derive(Serialize, Deserialize, Debug)]
pub struct Error {
    pub message: String,
    #[serde(skip)]
    pub status: StatusCode,
    #[serde(skip)]
    pub source: Option<anyhow::Error>,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.status, self.message)
    }
}
impl std::error::Error for Error {}

impl Error {
    pub fn new<E: std::error::Error + Send + Sync + 'static>(
        status: StatusCode,
        message: &str,
        source: E,
        print_source: bool,
    ) -> Error {
        Error {
            status,
            message: if print_source {
                if !message.is_empty() {
                    format!("{message}: {source}")
                } else {
                    source.to_string()
                }
            } else {
                message.to_string()
            },
            source: Some(anyhow::anyhow!(source)),
        }
    }

    pub fn with_msg(status: StatusCode, message: &str) -> Error {
        Error {
            status,
            message: message.to_string(),
            source: None,
        }
    }

    pub fn not_found() -> Error {
        Error::with_msg(StatusCode::NOT_FOUND, "not found in database")
    }
}
impl actix_web::error::ResponseError for Error {
    fn error_response(&self) -> actix_web::HttpResponse {
        actix_web::HttpResponse::build(self.status_code()).body(self.message.clone())
    }

    fn status_code(&self) -> StatusCode {
        self.status
    }
}

pub trait ServerErrorExt<T>
where
    Self: Sized,
{
    fn with_msg(self, status: StatusCode, message: &str) -> Result<T, Error>;

    fn with_msg_source(self, status: StatusCode, message: &str) -> Result<T, Error>;

    fn with_status(self, status: StatusCode) -> Result<T, Error>;

    fn with_interal(self) -> Result<T, Error>;
}

impl<T, E> ServerErrorExt<T> for Result<T, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    fn with_msg(self, status: StatusCode, message: &str) -> Result<T, Error> {
        self.map_err(|err| Error::new(status, message, err, false))
    }

    fn with_msg_source(self, status: StatusCode, message: &str) -> Result<T, Error> {
        self.map_err(|err| Error::new(status, message, err, true))
    }

    fn with_status(self, status: StatusCode) -> Result<T, Error> {
        self.map_err(|err| Error::new(status, "", err, true))
    }

    fn with_interal(self) -> Result<T, Error> {
        self.map_err(|err| {
            error!("Internal Server Error: {}", err);
            Error::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal server error",
                err,
                false,
            )
        })
    }
}

// pub struct StrErr(pub &'static str);
// impl Display for StrErr {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         Display::fmt(&self, f)
//     }
// }
// impl Debug for StrErr {
//     fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
//         Debug::fmt(&self, f)
//     }
// }
// impl std::error::Error for StrErr {}

// pub trait ToError<T> {
//     fn to_error(self) -> Result<T, StrErr>;
// }
// impl<T> ToError<T> for Result<T, &'static str> {
//     fn to_error(self) -> Result<T, StrErr> {
//         self.map_err(StrErr)
//     }
// }
