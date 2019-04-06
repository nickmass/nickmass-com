use futures::{future, Future};
use redis::PipelineCommands;
use serde_derive::{Deserialize, Serialize};

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
                    .ok_or_else(|| if_error("Unexpect post url_fragment"))?;

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

    pub fn get_all(&self, limit: i64, skip: i64) -> impl Future<Item = PostPage, Error = Error> {
        redis::cmd("lrange")
            .arg("posts")
            .arg(skip)
            .arg(limit - 1 + skip)
            .query_async(self.db.clone())
            .from_err::<Error>()
            .and_then(|(conn, post_ids): (_, Vec<i64>)| {
                let mut pipe = redis::Pipeline::with_capacity(post_ids.len());

                for id in post_ids {
                    pipe.hgetall(format!("post:{}", id));
                }

                pipe.query_async(conn).from_err::<Error>()
            })
            .and_then(|(conn, posts): (_, Vec<MaybePost>)| {
                let posts: Vec<Post> = posts.into_iter().filter_map(Option::from).collect();
                let mut author_ids: Vec<_> = posts.iter().map(|p| p.author_id).collect();
                author_ids.sort_unstable();
                author_ids.dedup();
                let mut pipe = redis::Pipeline::with_capacity(author_ids.len());

                for id in &author_ids {
                    pipe.hgetall(format!("user:{}", id));
                }

                pipe.query_async(conn)
                    .from_err::<Error>()
                    .join(future::ok(posts))
            })
            .and_then(|((conn, authors), mut posts): ((_, Vec<MaybeUser>), _)| {
                let authors: Vec<User> = authors.into_iter().filter_map(Option::from).collect();
                let author_map: HashMap<_, _> = authors.into_iter().map(|u| (u.id, u)).collect();

                posts.iter_mut().for_each(|p| {
                    p.author = author_map.get(&p.author_id).map(|u| u.name.clone());
                });

                redis::cmd("llen")
                    .arg("posts")
                    .query_async(conn)
                    .from_err::<Error>()
                    .join(future::ok(posts))
            })
            .map(move |((_conn, total), posts): ((_, i64), _)| PostPage {
                posts,
                total,
                has_more: total > limit + skip,
            })
    }

    pub fn get(&self, id: u64) -> impl Future<Item = Post, Error = Error> {
        Self::get_by_id(self.db.clone(), id)
    }

    pub fn get_by_fragment(
        &self,
        fragment: impl AsRef<str>,
    ) -> impl Future<Item = Post, Error = Error> {
        redis::cmd("get")
            .arg(format!("postFragment:{}", fragment.as_ref()))
            .query_async(self.db.clone())
            .from_err::<Error>()
            .and_then(|(conn, id)| Self::get_by_id(conn, id))
    }

    fn get_by_id(db: Connection, id: u64) -> impl Future<Item = Post, Error = Error> {
        redis::cmd("hgetall")
            .arg(format!("post:{}", id))
            .query_async(db)
            .from_err::<Error>()
            .and_then(move |(conn, post): (_, MaybePost)| {
                if let Some(post) = Option::<Post>::from(post) {
                    future::Either::A(
                        redis::cmd("hget")
                            .arg(format!("user:{}", post.author_id))
                            .arg("name")
                            .query_async(conn)
                            .from_err::<Error>()
                            .join(future::ok(post)),
                    )
                } else {
                    future::Either::B(future::err(Error::ResourceNotFound(Resource::Post(id))))
                }
            })
            .map(|((_conn, author), mut post)| {
                post.author = author;
                post
            })
    }
}

impl Authenticated<PostClient> {
    pub fn create(&self, mut post: Post) -> impl Future<Item = u64, Error = Error> {
        post.id = 0;
        post.author_id = self.user().id;
        post.date = chrono::Utc::now().timestamp_millis() as u64;

        redis::cmd("incr")
            .arg("nextPostId")
            .query_async(self.db.clone())
            .from_err::<Error>()
            .and_then(|(conn, post_id)| {
                post.id = post_id;
                let mut pipe = redis::pipe();
                pipe.lpush("posts", post_id).ignore();
                pipe.set(format!("postFragment:{}", post.url_fragment), post.id)
                    .ignore();
                pipe.hset_multiple(
                    format!("post:{}", post_id),
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
                pipe.query_async(conn)
                    .from_err::<Error>()
                    .map(move |(conn, _): (_, ())| (conn, post_id))
            })
            .and_then(|(conn, post_id)| {
                redis::cmd("bgsave")
                    .query_async(conn)
                    .from_err::<Error>()
                    .map(move |_: (_, ())| post_id)
            })
    }

    pub fn update(&self, id: u64, post: Post) -> impl Future<Item = u64, Error = Error> {
        redis::cmd("hexists")
            .arg(format!("post:{}", id))
            .query_async(self.db.clone())
            .from_err::<Error>()
            .and_then(move |(conn, exists): (_, bool)| {
                if !exists {
                    future::Either::A(future::err(Error::ResourceNotFound(Resource::Post(id))))
                } else {
                    let mut pipe = redis::pipe();
                    pipe.set(format!("postFragment:{}", post.url_fragment), id)
                        .ignore();
                    pipe.hset_multiple(
                        format!("post:{}", post.id),
                        &[
                            ("title", post.title),
                            ("content", post.content),
                            ("urlFragment", post.url_fragment),
                        ],
                    )
                    .ignore();
                    let fut = pipe
                        .query_async(conn)
                        .from_err::<Error>()
                        .map(move |(conn, _): (_, ())| (conn, id))
                        .and_then(|(conn, post_id)| {
                            redis::cmd("bgsave")
                                .query_async(conn)
                                .from_err::<Error>()
                                .map(move |_: (_, ())| post_id)
                        });
                    future::Either::B(fut)
                }
            })
    }

    pub fn delete(&self, _id: u64) -> impl Future<Item = (), Error = Error> {
        futures::future::err(Error::NotFound)
    }
}
