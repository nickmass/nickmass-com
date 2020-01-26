use serde::{Deserialize, Serialize};

use super::db::Connection;
use super::error::Resource;
use super::Error;

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

    pub async fn get(&mut self, id: u64) -> Result<User, Error> {
        Self::get_by_id(&mut self.db, id).await
    }

    pub async fn get_social_user(&mut self, social_id: impl AsRef<str>) -> Result<User, Error> {
        let social_user_key = format!("socialUser:{}", social_id.as_ref());
        let user_id = redis::cmd("get")
            .arg(social_user_key)
            .query_async(&mut self.db)
            .await?;
        Self::get_by_id(&mut self.db, user_id).await
    }

    async fn get_by_id(conn: &mut Connection, id: u64) -> Result<User, Error> {
        let user_key = format!("user:{}", id);
        let user: MaybeUser = redis::cmd("hgetall")
            .arg(user_key)
            .query_async(conn)
            .await?;
        let user = Option::<User>::from(user);
        user.ok_or(Error::ResourceNotFound(Resource::User(id)))
    }
}
