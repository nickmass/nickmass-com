use std::fmt;

use serde_derive::{Deserialize, Serialize};

#[derive(Debug)]
pub enum Error {
    Redis(redis::RedisError),
    Reqwest(reqwest::Error),
    Render((&'static str, askama::Error)),
    ResourceNotFound(Resource),
    Unauthorized,
    NotFound,
    IpRequired,
    Void(Void),
}

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

impl From<Void> for Error {
    fn from(other: Void) -> Self {
        Error::Void(other)
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
            Error::Void(_v) => unreachable!(),
        }
    }
}

impl std::error::Error for Error {}

#[allow(dead_code)]
#[derive(Debug)]
pub enum Void {}

impl std::fmt::Display for Void {
    fn fmt(&self, _: &mut std::fmt::Formatter) -> std::fmt::Result {
        unreachable!()
    }
}

impl std::error::Error for Void {}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonError {
    pub code: u16,
    pub message: String,
}
