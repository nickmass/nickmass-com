use serde::{Deserialize, Serialize};

use super::auth::Authenticated;
use super::db::Connection;
use super::error::Resource;
use super::users::{MaybeUser, User};
use super::Error;

use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct Post {
    #[serde(default)]
    pub id: u64,
    #[serde(skip_deserializing)]
    pub author_id: u64,
    #[serde(skip_deserializing)]
    pub date: u64,
    pub content: String,
    pub title: String,
    pub url_fragment: String,
    #[serde(skip_deserializing)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
}

struct MaybePost(Option<Post>);

impl redis::FromRedisValue for MaybePost {
    fn from_redis_value(v: &redis::Value) -> redis::RedisResult<MaybePost> {
        match HashMap::<String, String>::from_redis_value(v) {
            Ok(mut h) => {
                if h.len() == 0 {
                    return Ok(MaybePost(None));
                }
                let if_error = |s| (redis::ErrorKind::ResponseError, s);
                let id = h
                    .get("id")
                    .and_then(|i| i.parse().ok())
                    .ok_or_else(|| if_error("Unexpected post id"))?;
                let author_id = h
                    .get("authorId")
                    .and_then(|i| i.parse().ok())
                    .ok_or_else(|| if_error("Unexpected post author_id"))?;
                let date = h
                    .get("date")
                    .and_then(|i| i.parse().ok())
                    .ok_or_else(|| if_error("Unexpected post date"))?;
                let content = h
                    .remove("content")
                    .ok_or_else(|| if_error("Unexpected post content"))?;
                let title = h
                    .remove("title")
                    .ok_or_else(|| if_error("Unexpected post title"))?;
                let url_fragment = h
                    .remove("urlFragment")
                    .ok_or_else(|| if_error("Unexpected post url_fragment"))?;

                Ok(MaybePost(Some(Post {
                    id,
                    author_id,
                    content,
                    date,
                    title,
                    url_fragment,
                    author: None,
                })))
            }
            Err(e) => Err(e),
        }
    }
}

impl From<MaybePost> for Option<Post> {
    fn from(other: MaybePost) -> Option<Post> {
        other.0
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PostPage {
    pub posts: Vec<Post>,
    pub has_more: bool,
    pub total: i64,
}

pub struct PostClient {
    db: Connection,
}

impl PostClient {
    pub fn new(db: Connection) -> PostClient {
        PostClient { db }
    }

    pub async fn get_all(mut self, limit: i64, skip: i64) -> Result<PostPage, Error> {
        let post_ids: Vec<i64> = redis::cmd("lrange")
            .arg("posts")
            .arg(skip)
            .arg(limit - 1 + skip)
            .query_async(&mut self.db)
            .await?;
        let mut pipe = redis::Pipeline::with_capacity(post_ids.len());

        for id in post_ids {
            pipe.hgetall(format!("post:{}", id));
        }

        let posts: Vec<MaybePost> = pipe.query_async(&mut self.db).await?;

        let mut posts: Vec<Post> = posts.into_iter().filter_map(Option::from).collect();
        let mut author_ids: Vec<_> = posts.iter().map(|p| p.author_id).collect();
        author_ids.sort_unstable();
        author_ids.dedup();

        let mut pipe = redis::Pipeline::with_capacity(author_ids.len());

        for id in &author_ids {
            pipe.hgetall(format!("user:{}", id));
        }

        let authors: Vec<MaybeUser> = pipe.query_async(&mut self.db).await?;

        let authors: Vec<User> = authors.into_iter().filter_map(Option::from).collect();
        let author_map: HashMap<_, _> = authors.into_iter().map(|u| (u.id, u)).collect();

        posts.iter_mut().for_each(|p| {
            p.author = author_map.get(&p.author_id).map(|u| u.name.clone());
        });

        let total: i64 = redis::cmd("llen")
            .arg("posts")
            .query_async(&mut self.db)
            .await?;
        Ok(PostPage {
            posts,
            total,
            has_more: total > limit + skip,
        })
    }

    pub async fn get(mut self, id: u64) -> Result<Post, Error> {
        Self::get_by_id(&mut self.db, id).await
    }

    pub async fn get_by_fragment(mut self, fragment: impl AsRef<str>) -> Result<Post, Error> {
        let fragment_key: String = format!("postFragment:{}", fragment.as_ref());
        let id = redis::cmd("get")
            .arg(fragment_key)
            .query_async(&mut self.db)
            .await?;

        if let Some(id) = id {
            Self::get_by_id(&mut self.db, id).await
        } else {
            Err(Error::NotFound)
        }
    }

    async fn get_by_id(db: &mut Connection, id: u64) -> Result<Post, Error> {
        let post_key = format!("post:{}", id);
        let post: MaybePost = redis::cmd("hgetall").arg(post_key).query_async(db).await?;
        if let Some(mut post) = Option::<Post>::from(post) {
            let author: String = format!("user:{}", post.author_id);
            let author = redis::cmd("hget")
                .arg(author)
                .arg("name")
                .query_async(db)
                .await?;
            post.author = author;
            Ok(post)
        } else {
            Err(Error::ResourceNotFound(Resource::Post(id)))
        }
    }
}

impl Authenticated<PostClient> {
    pub async fn create(mut self, mut post: Post) -> Result<u64, Error> {
        post.id = 0;
        post.author_id = self.user().id;
        post.date = chrono::Utc::now().timestamp_millis() as u64;

        let post_id = redis::cmd("incr")
            .arg("nextPostId")
            .query_async(&mut self.db)
            .await?;

        post.id = post_id;

        let fragment_key = format!("postFragment:{}", post.url_fragment);
        let post_key = format!("post:{}", post_id);

        let mut pipe = redis::pipe();
        pipe.lpush("posts", post_id).ignore();
        pipe.set(fragment_key, post.id).ignore();
        pipe.hset_multiple(
            post_key,
            &[
                ("id", post_id.to_string()),
                ("title", post.title),
                ("content", post.content),
                ("date", post.date.to_string()),
                ("authorId", post.author_id.to_string()),
                ("urlFragment", post.url_fragment),
            ],
        )
        .ignore();

        let _: () = pipe.query_async(&mut self.db).await?;
        let _: () = redis::cmd("bgsave").query_async(&mut self.db).await?;

        Ok(post.id)
    }

    pub async fn update(mut self, id: u64, post: Post) -> Result<u64, Error> {
        let post_key = format!("post:{}", id);
        let exists: bool = redis::cmd("exists")
            .arg(post_key.clone())
            .query_async(&mut self.db)
            .await?;
        if !exists {
            Err(Error::ResourceNotFound(Resource::Post(id)))
        } else {
            let mut pipe = redis::pipe();
            let fragment_key = format!("postFragment:{}", post.url_fragment);
            pipe.set(fragment_key, id).ignore();
            pipe.hset_multiple(
                post_key,
                &[
                    ("title", post.title),
                    ("content", post.content),
                    ("urlFragment", post.url_fragment),
                ],
            )
            .ignore();

            let _: () = pipe.query_async(&mut self.db).await?;
            let _: () = redis::cmd("bgsave").query_async(&mut self.db).await?;
            Ok(id)
        }
    }

    pub async fn delete(self, _id: u64) -> Result<(), Error> {
        Err(Error::NotFound)
    }
}
