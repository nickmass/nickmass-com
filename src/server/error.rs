use std::fmt;

use deadpool_redis::{BuildError, PoolError};
use serde::{Deserialize, Serialize};

#[derive(Debug)]
pub enum Error {
    Redis(redis::RedisError),
    Reqwest(reqwest::Error),
    Render((&'static str, askama::Error)),
    ResourceNotFound(Resource),
    Unauthorized,
    NotFound,
    Timeout(tokio::time::error::Elapsed),
    Pool(deadpool_redis::PoolError),
    CreatePool(deadpool_redis::CreatePoolError),
}

#[derive(Debug)]
pub enum Resource {
    User(u64),
    Post(u64),
}

impl Error {
    pub fn json(&self) -> JsonError {
        JsonError {
            code: self.status_code(),
            message: self.to_string(),
        }
    }

    pub fn status_code(&self) -> u16 {
        match *self {
            Error::NotFound => 404,
            Error::ResourceNotFound(_) => 404,
            Error::Unauthorized => 401,
            _ => 500,
        }
    }

    pub fn status(&self) -> axum::http::StatusCode {
        axum::http::StatusCode::from_u16(self.status_code()).unwrap()
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

impl From<tokio::time::error::Elapsed> for Error {
    fn from(other: tokio::time::error::Elapsed) -> Self {
        Error::Timeout(other)
    }
}

impl From<deadpool_redis::PoolError> for Error {
    fn from(other: deadpool_redis::PoolError) -> Self {
        match other {
            PoolError::Backend(e) => Error::Redis(e),
            _ => Error::Pool(other),
        }
    }
}

impl From<deadpool_redis::CreatePoolError> for Error {
    fn from(other: deadpool_redis::CreatePoolError) -> Self {
        match other {
            deadpool_redis::CreatePoolError::Build(BuildError::Backend(e)) => Error::Redis(e),
            _ => Error::CreatePool(other),
        }
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
            Error::Timeout(timeout) => write!(f, "Timeout: {}", timeout),
            Error::CreatePool(err) => write!(f, "Create Pool: {}", err),
            Error::Pool(err) => write!(f, "Pool: {}", err),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Serialize, Deserialize)]
pub struct JsonError {
    pub code: u16,
    pub message: String,
}

impl From<Error> for JsonError {
    fn from(err: Error) -> Self {
        err.json()
    }
}
