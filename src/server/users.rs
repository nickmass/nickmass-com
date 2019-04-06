use futures::{future, Future};
use serde_derive::{Deserialize, Serialize};

use super::db::Connection;

#[derive(Debug)]
pub enum Error {
    NotFound,
    Redis(redis::RedisError),
}

impl Error {
    pub fn reject(self) -> warp::Rejection {
        log::error!("user error {:?}", self);
        match self {
            Error::NotFound => warp::reject::forbidden(),
            e => warp::reject::custom(e.to_string()),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::NotFound => write!(f, "User Not Found"),
            Error::Redis(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::NotFound => None,
            Error::Redis(e) => e.source(),
        }
    }
}

impl From<redis::RedisError> for Error {
    fn from(other: redis::RedisError) -> Error {
        Error::Redis(other)
    }
}

#[derive(Serialize, Deserialize)]
pub struct User {
    pub id: u64,
    pub name: String,
}

pub struct MaybeUser(Option<User>);

impl redis::FromRedisValue for MaybeUser {
    fn from_redis_value(v: &redis::Value) -> redis::RedisResult<MaybeUser> {
        match ::std::collections::HashMap::<String, String>::from_redis_value(v) {
            Ok(mut h) => {
                if h.len() == 0 {
                    return Ok(MaybeUser(None));
                }
                let if_error = |s| (redis::ErrorKind::ResponseError, s);
                let id = h
                    .get("id")
                    .and_then(|i| i.parse().ok())
                    .ok_or_else(|| if_error("Unexpected user id"))?;
                let name = h
                    .remove("name")
                    .ok_or_else(|| if_error("Unexpected user name"))?;

                Ok(MaybeUser(Some(User { id, name })))
            }
            Err(e) => Err(e),
        }
    }
}

impl From<MaybeUser> for Option<User> {
    fn from(other: MaybeUser) -> Option<User> {
        other.0
    }
}

pub struct UserClient {
    db: Connection,
}

impl UserClient {
    pub fn new(db: Connection) -> UserClient {
        UserClient { db }
    }

    pub fn get(&self, id: u64) -> impl Future<Item = User, Error = Error> {
        Self::get_by_id(self.db.clone(), id)
    }

    pub fn get_social_user(
        &self,
        social_id: impl AsRef<str>,
    ) -> impl Future<Item = User, Error = Error> {
        redis::cmd("get")
            .arg(format!("socialUser:{}", social_id.as_ref()))
            .query_async(self.db.clone())
            .from_err::<Error>()
            .and_then(|(conn, user_id)| Self::get_by_id(conn, user_id))
    }

    fn get_by_id(conn: Connection, id: u64) -> impl Future<Item = User, Error = Error> {
        redis::cmd("hgetall")
            .arg(format!("user:{}", id))
            .query_async(conn)
            .from_err::<Error>()
            .and_then(|(_conn, user): (_, MaybeUser)| {
                if let Some(user) = Option::<User>::from(user) {
                    future::ok(user)
                } else {
                    future::err(Error::NotFound)
                }
            })
    }
}
