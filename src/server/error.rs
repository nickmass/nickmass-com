use std::fmt;

use serde::{Deserialize, Serialize};
use warp::reject::Reject;

#[derive(Debug)]
pub enum Error {
    Redis(redis::RedisError),
    Reqwest(reqwest::Error),
    Render((&'static str, askama::Error)),
    ResourceNotFound(Resource),
    Unauthorized,
    NotFound,
    IpRequired,
    Timeout(tokio::time::Elapsed),
}

impl Reject for Error {}

#[derive(Debug)]
pub enum Resource {
    User(u64),
    Post(u64),
}

impl Error {
    pub fn to_json(&self) -> JsonError {
        JsonError {
            code: self.status_code(),
            message: self.to_string(),
        }
    }

    pub fn status_code(&self) -> u16 {
        500
    }
}

impl From<redis::RedisError> for Error {
    fn from(other: redis::RedisError) -> Self {
        Error::Redis(other)
    }
}

impl From<reqwest::Error> for Error {
    fn from(other: reqwest::Error) -> Self {
        Error::Reqwest(other)
    }
}

impl From<tokio::time::Elapsed> for Error {
    fn from(other: tokio::time::Elapsed) -> Self {
        Error::Timeout(other)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Redis(redis) => write!(f, "Redis: {}", redis),
            Error::Reqwest(reqwest) => write!(f, "Reqwest: {}", reqwest),
            Error::ResourceNotFound(res) => write!(f, "Unable to find: {:?}", res),
            Error::Unauthorized => write!(f, "Unauthorized"),
            Error::Render((name, err)) => write!(f, "Failed to render {} {}", name, err),
            Error::NotFound => write!(f, "Not found"),
            Error::IpRequired => write!(f, "Ip required for session data"),
            Error::Timeout(timeout) => write!(f, "Timeout: {}", timeout),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonError {
    pub code: u16,
    pub message: String,
}
