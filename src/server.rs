use futures::{future, Future};
use log::{error, info};
use serde_derive::{Deserialize, Serialize};
use warp::{self, Filter};

use std::sync::Arc;

pub mod config;
pub mod model;
pub mod view;

pub use config::Config;

type Db = Arc<self::db::Db>;

pub fn run(config: Config) {
    let db = Arc::new(self::db::Db::new(&*config.redis_url.to_string()).unwrap());

    let config = Arc::new(config);
    let server_config = config.clone();

    let db = warp::any()
        .map(move || db.clone())
        .and_then(|db: Db| db.get().map_err(err_log));

    let config = warp::any().map(move || config.clone());

    let session = warp::any()
        .and(db.clone())
        .and(config.clone())
        .map(|db, config: Arc<Config>| session::Session::new(db, &config.session_key));

    let maybe_cookie = |name: &'static str| {
        warp::cookie(name)
            .map(|c| Some(c))
            .or(warp::any().map(|| None))
            .unify()
    };

    let session_store = warp::ext::get::<session::Store>()
        .or(warp::any()
            .and(session.clone())
            .and(warp::addr::remote().and_then(|addr| match addr {
                Some(addr) => future::ok(addr),
                None => future::err(warp::reject::custom("Ip required")),
            }))
            .and(maybe_cookie("sid"))
            .and_then(
                |session: session::Session, ip: std::net::SocketAddr, sid: Option<String>| {
                    session
                        .get_store(ip.ip(), sid)
                        .map(|store| {
                            warp::ext::set(store.clone());
                            store
                        })
                        .map_err(err_log)
                },
            ))
        .unify();

    let maybe_auth = warp::any()
        .and(session_store.clone())
        .and(db.clone())
        .and_then(|ses: session::Store, conn| {
            let id = ses.get("socialUser");
            if let Some(social_id) = id {
                let client = users::UserClient::new(conn);
                future::Either::A(client.get_social_user(social_id).map(Some))
            } else {
                future::Either::B(future::ok(None))
            }
            .map_err(users::Error::reject)
        });

    let auth = maybe_auth.clone().and_then(|user| {
        if let Some(user) = user {
            future::ok(user)
        } else {
            future::err(users::Error::NotFound)
        }
        .map_err(users::Error::reject)
    });

    let view_index = warp::path::end()
        .and(maybe_auth.clone())
        .and(db.clone())
        .and_then(|user, conn| view::index(user, conn, None).map_err(err_log));

    let view_page = warp::path("page")
        .and(warp::path::param::<i64>())
        .and(warp::path::end())
        .and(maybe_auth.clone())
        .and(db.clone())
        .and_then(|page, user, conn| view::index(user, conn, Some(page)).map_err(err_log));

    let view_post_id = warp::path("post")
        .and(warp::path::param::<u64>())
        .and(warp::path::end())
        .and(maybe_auth.clone())
        .and(db.clone())
        .and_then(|post, user, conn| view::post_id(user, conn, post).map_err(err_log));

    let view_post_frag = warp::path("post")
        .and(warp::path::param::<String>())
        .and(warp::path::end())
        .and(maybe_auth.clone())
        .and(db.clone())
        .and_then(|post, user, conn| view::post_frag(user, conn, post).map_err(err_log));

    let views = view_index
        .or(view_page)
        .unify()
        .or(view_post_id)
        .unify()
        .or(view_post_frag)
        .unify()
        .map(warp::reply::html)
        .map(|reply| {
            warp::reply::with_header(
                reply,
                "link",
                "</css/bundle.css>; rel=preload; as=style, </js/header.js>; rel=preload; as=script",
            )
        });

    let json_body = warp::body::content_length_limit(1024 * 1024 * 5).and(warp::body::json());
    let api = warp::path("api");
    let posts = api.and(warp::path("posts"));
    let posts_index = posts.and(warp::path::end());
    let posts_id = posts.and(warp::path::param::<u64>()).and(warp::path::end());
    let posts_frag = posts
        .and(warp::path::param::<String>())
        .and(warp::path::end());

    let posts_get_all = warp::get2()
        .and(posts_index)
        .and(db.clone())
        .and_then(|db| {
            let client = posts::PostClient::new(db);
            client
                .get_all(100, 0)
                .map(|posts| warp::reply::json(&posts))
                .map_err(posts::Error::reject)
        });

    let posts_get = warp::get2()
        .and(posts_id)
        .and(db.clone())
        .and_then(|id, conn| {
            let client = posts::PostClient::new(conn);
            client
                .get(id)
                .map(|post| warp::reply::json(&post))
                .map_err(posts::Error::reject)
        });

    let posts_get_fragment =
        warp::get2()
            .and(posts_frag)
            .and(db.clone())
            .and_then(|fragment, conn| {
                let client = posts::PostClient::new(conn);
                client
                    .get_by_fragment(fragment)
                    .map(|post| warp::reply::json(&post))
                    .map_err(posts::Error::reject)
            });

    let posts_post = warp::post2()
        .and(posts_index)
        .and(json_body)
        .and(auth.clone())
        .and(db.clone())
        .and_then(|post, user, conn| {
            let client = Authenticated {
                user,
                resource: posts::PostClient::new(conn),
            };

            client
                .create(post)
                .map(|id| warp::reply::json(&id))
                .map_err(posts::Error::reject)
        });

    let posts_put = warp::put2()
        .and(posts_index)
        .and(posts_id)
        .and(json_body)
        .and(auth.clone())
        .and(db.clone())
        .and_then(|id, post: posts::Post, user, conn| {
            let client = Authenticated {
                user,
                resource: posts::PostClient::new(conn),
            };

            client
                .update(id, post)
                .map(|id| warp::reply::json(&id))
                .map_err(posts::Error::reject)
        });

    let posts_delete = warp::delete2()
        .and(posts_index)
        .and(posts_id)
        .and(auth.clone())
        .and(db.clone())
        .and_then(|id, user, conn| {
            let client = Authenticated {
                user,
                resource: posts::PostClient::new(conn),
            };

            client
                .delete(id)
                .map(|id| warp::reply::json(&id))
                .map_err(posts::Error::reject)
        });

    let posts_api = posts_get_all
        .or(posts_get)
        .or(posts_get_fragment)
        .or(posts_post)
        .or(posts_put)
        .or(posts_delete);

    let users_api = api
        .and(warp::path("users"))
        .and(warp::path("current"))
        .and(warp::path::end())
        .and(auth.clone())
        .map(|user: users::User| warp::reply::json(&user));

    let auth = warp::path("auth");

    let logout = auth
        .and(warp::path("logout"))
        .and(config.clone())
        .map(|config: Arc<Config>| {
            let reply = warp::redirect(config.base_url.clone());
            warp::reply::with_header(
                reply,
                "set-cookie",
                "sid=; Expires=Thu, 01 Jan 1970 00:00:00 GMT; Path=/; SameSite=Strict; HttpOnly",
            )
        });

    let google = auth.and(warp::path("google"));

    let google_return = google.and(warp::path("return")).and(warp::path::end());
    let google_oath_return = warp::get2()
        .and(google_return)
        .and(session_store.clone())
        .and(warp::query::<OauthResponse>())
        .and(config.clone())
        .and_then(
            |store: session::Store, oauth: OauthResponse, config: Arc<Config>| {
                let client = reqwest::r#async::Client::new();
                let redirect_uri = format!("{}auth/google/return", config.base_url);
                client
                    .post(&config.oauth_token_url.to_string())
                    .form(&OauthTokenRequest {
                        code: &oauth.code,
                        client_id: &config.oauth_id,
                        client_secret: &config.oauth_secret,
                        redirect_uri: redirect_uri.as_str(),
                        grant_type: "authorization_code",
                    })
                    .send()
                    .and_then(|mut res| res.json::<OauthTokenResponse>())
                    /*
                    .and_then(|res| res.into_body().concat2())
                    .map(|body| {
                        eprintln!("b: {:?}", body);
                        serde_json::from_slice(&body).unwrap()
                    })*/
                    .map_err(err_log)
                    .map(move |res: OauthTokenResponse| {
                        store.set("socialUser", format!("google:{}", res.id_token.claims.sub));
                        warp::redirect(config.base_url.clone())
                    })
            },
        )
        .and(session.clone())
        .and(session_store.clone())
        .and_then(|reply, session: session::Session, store: session::Store| {
            let sid = store.sid();
            session
                .set_store(store)
                .map(move |_| {
                    warp::reply::with_header(
                        reply,
                        "set-cookie",
                        format!(
                            "sid={}; Max-Age={}; Path=/; SameSite=Strict; HttpOnly",
                            sid,
                            60 * 60 * 24 * 30
                        ),
                    )
                })
                .map_err(err_log)
        });

    let google_login = warp::get2()
        .and(google.and(warp::path::end()))
        .and(config.clone())
        .map(|config: Arc<Config>| {
            let redirect_uri = format!("{}auth/google/return", config.base_url);
            let auth_url = url::Url::parse_with_params(
                &*config.oauth_login_url.to_string(),
                &[
                    ("client_id", config.oauth_id.as_str()),
                    ("response_type", "code"),
                    ("scope", "openid email profile"),
                    ("redirect_uri", redirect_uri.as_str()),
                    ("state", "abc"),
                ],
            )
            .expect("Config allows valid google url");
            let http_uri: warp::http::Uri = auth_url.to_string().parse().expect("Url is valid uri");
            warp::redirect(http_uri)
        });

    let google_auth = google_login.or(google_oath_return);

    let static_content = warp::fs::dir("./public");

    let logger = warp::log("nickmass_com::api");

    let fallback = warp::get2()
        .or(warp::post2())
        .unify()
        .and(warp::any())
        .and(maybe_auth.clone())
        .and(db.clone())
        .and_then(|user, conn| view::not_found(user, conn).map_err(err_log))
        .map(warp::reply::html)
        .map(|reply| warp::reply::with_status(reply, warp::http::StatusCode::NOT_FOUND));

    let socket_addr: std::net::SocketAddr =
        (server_config.listen_ip, server_config.listen_port).into();
    info!("server starting on {}", socket_addr);
    warp::serve(
        views
            .or(posts_api)
            .or(users_api)
            .or(logout)
            .or(google_auth)
            .or(static_content)
            .or(fallback)
            .with(logger),
    )
    .run(socket_addr);
}

fn err_log(error: impl std::error::Error + Send + Sync + 'static) -> warp::Rejection {
    error!("{:?}", error);
    warp::reject::custom(error)
}

#[derive(Debug, Serialize, Deserialize)]
struct OauthResponse {
    state: String,
    code: String,
}

#[derive(Debug, Serialize)]
struct OauthTokenRequest<'a> {
    code: &'a str,
    client_id: &'a str,
    client_secret: &'a str,
    redirect_uri: &'a str,
    grant_type: &'a str,
}

#[derive(Debug, Serialize, Deserialize)]
struct OauthTokenResponse {
    access_token: String,
    #[serde(deserialize_with = "GoogleToken::deser")]
    id_token: GoogleToken,
    expires_in: u64,
    token_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct GoogleToken {
    header: GoogleTokenHeader,
    claims: GoogleTokenClaims,
}

impl GoogleToken {
    fn deser<'d, D: serde::Deserializer<'d>>(de: D) -> Result<GoogleToken, D::Error> {
        use serde::Deserialize;
        let base64 = String::deserialize(de)?;
        let token = jwt::Token::parse(base64.as_str())
            .map_err(|e| serde::de::Error::custom(format!("{:?}", e)))?;
        Ok(GoogleToken {
            header: token.header,
            claims: token.claims,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct GoogleTokenHeader {
    alg: String,
    kid: String,
    typ: String,
}

impl jwt::Component for GoogleTokenHeader {
    fn from_base64(raw: &str) -> Result<Self, jwt::Error> {
        let json = base64::decode(raw).map_err(|_e| jwt::Error::Format)?;
        serde_json::from_slice(json.as_slice()).map_err(|_e| jwt::Error::Format)
    }

    fn to_base64(&self) -> Result<String, jwt::Error> {
        let json = serde_json::to_string(self).map_err(|_e| jwt::Error::Format)?;
        Ok(base64::encode(&*json))
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct GoogleTokenClaims {
    iss: String,
    sub: String,
    email: String,
    name: String,
}

impl jwt::Component for GoogleTokenClaims {
    fn from_base64(raw: &str) -> Result<Self, jwt::Error> {
        let json = base64::decode(raw).map_err(|_e| jwt::Error::Format)?;
        serde_json::from_slice(json.as_slice()).map_err(|_e| jwt::Error::Format)
    }

    fn to_base64(&self) -> Result<String, jwt::Error> {
        let json = serde_json::to_string(self).map_err(|_e| jwt::Error::Format)?;
        Ok(base64::encode(&*json))
    }
}

struct Authenticated<T> {
    user: users::User,
    resource: T,
}

impl<T> std::ops::Deref for Authenticated<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.resource
    }
}

mod session {
    use futures::{future, Future};
    use redis::PipelineCommands;
    use ring::{aead, rand};

    use super::db::Connection;

    use std::collections::HashMap;
    use std::net::IpAddr;
    use std::sync::{Arc, Mutex};

    #[allow(dead_code)]
    #[derive(Debug)]
    pub enum Void {}

    impl std::fmt::Display for Void {
        fn fmt(&self, _: &mut std::fmt::Formatter) -> std::fmt::Result {
            unreachable!()
        }
    }

    impl std::error::Error for Void {}

    pub struct Session {
        db: Connection,
        rand: rand::SystemRandom,
        sealing_key: aead::SealingKey,
        opening_key: aead::OpeningKey,
    }

    impl Session {
        pub fn new(db: Connection, session_key: impl AsRef<[u8]>) -> Session {
            let sealing_key = aead::SealingKey::new(&aead::AES_256_GCM, session_key.as_ref())
                .expect("Valid session key");
            let opening_key = aead::OpeningKey::new(&aead::AES_256_GCM, session_key.as_ref())
                .expect("Valid session key");
            Session {
                db,
                rand: rand::SystemRandom::new(),
                sealing_key,
                opening_key,
            }
        }

        pub fn get_store(
            &self,
            addr: IpAddr,
            sid: Option<impl AsRef<str>>,
        ) -> impl Future<Item = Store, Error = Void> {
            if let Some((sid, key)) =
                sid.and_then(|sid| self.decode_sid(addr, &sid).map(|key| (sid, key)))
            {
                let ok_key = key.clone();
                let ok_sid = sid.as_ref().to_string();
                let fut = redis::cmd("hgetall")
                    .arg(format!("session:{}", key))
                    .query_async(self.db.clone())
                    .map(move |(_conn, hash)| Store::new(ok_key, ok_sid, hash))
                    .or_else(move |_e| Ok(Store::empty(key, sid.as_ref())));
                future::Either::A(fut)
            } else {
                let key = self.create_key();
                let sid = self.create_sid(&key, addr);
                future::Either::B(future::ok(Store::empty(key, sid)))
            }
        }

        pub fn set_store(&self, store: Store) -> impl Future<Item = (), Error = Void> {
            let mut pipe = redis::pipe();
            let redis_key = format!("session:{}", store.key);
            pipe.hset_multiple(redis_key.as_str(), store.values().as_slice());
            pipe.expire(redis_key.as_str(), 60 * 60 * 24 * 90);
            pipe.query_async(self.db.clone())
                .map(|_: (_, ())| ())
                .or_else(|_e| Ok(()))
        }

        fn decode_sid(&self, addr: IpAddr, sid: impl AsRef<str>) -> Option<String> {
            let mut sid_bytes = base64::decode(sid.as_ref()).ok()?;

            let nonce = aead::Nonce::try_assume_unique_for_key(&sid_bytes[0..12]).ok()?;

            let sid_bytes = aead::open_in_place(
                &self.opening_key,
                nonce,
                aead::Aad::empty(),
                12,
                &mut sid_bytes,
            )
            .ok()?;
            let sid_string = String::from_utf8(sid_bytes.to_vec()).ok()?;
            let mut parts = sid_string.splitn(2, '.');
            let user_key = parts.next()?;
            let ip = parts.next()?;

            if ip == addr.to_string() {
                Some(user_key.to_string())
            } else {
                None
            }
        }

        fn create_key(&self) -> String {
            use ring::rand::SecureRandom;
            let mut user_key = [0; 128];
            self.rand
                .fill(&mut user_key)
                .expect("Crypto error, could not fill session user key random");
            base64::encode(&user_key[..])
        }

        fn create_sid(&self, user_key: impl AsRef<str>, addr: IpAddr) -> String {
            use ring::rand::SecureRandom;
            let mut sid: Vec<u8> = format!("{}.{}", user_key.as_ref(), addr).as_bytes().into();
            let mut nonce_bytes = [0; 12];
            self.rand
                .fill(&mut nonce_bytes)
                .expect("Crypto error, could not fill session nonce random");
            let nonce = aead::Nonce::try_assume_unique_for_key(&nonce_bytes[..])
                .expect("Crypto error, incorrect nonce length");

            let suffix_len = self.sealing_key.algorithm().tag_len();
            sid.resize(sid.len() + suffix_len, 0);

            let out_len = aead::seal_in_place(
                &self.sealing_key,
                nonce,
                aead::Aad::empty(),
                &mut sid,
                suffix_len,
            )
            .expect("Crypto error, failed to encrypt");

            let mut sid_bytes = Vec::from(&nonce_bytes[..]);
            sid_bytes.extend_from_slice(&sid[..out_len]);
            base64::encode(&sid_bytes)
        }
    }

    #[derive(Debug, Clone)]
    pub struct Store {
        key: Arc<String>,
        sid: Arc<String>,
        inner: Arc<Mutex<HashMap<String, String>>>,
    }

    impl Store {
        fn new(
            key: impl Into<String>,
            sid: impl Into<String>,
            data: HashMap<String, String>,
        ) -> Store {
            Store {
                key: Arc::new(key.into()),
                sid: Arc::new(sid.into()),
                inner: Arc::new(Mutex::new(data)),
            }
        }

        fn empty(key: impl Into<String>, sid: impl Into<String>) -> Store {
            Store {
                key: Arc::new(key.into()),
                sid: Arc::new(sid.into()),
                inner: Arc::new(Mutex::new(HashMap::new())),
            }
        }

        fn values(&self) -> Vec<(String, String)> {
            let hash = self.inner.lock().unwrap();
            hash.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
        }

        pub fn get(&self, key: impl AsRef<str>) -> Option<String> {
            self.inner.lock().unwrap().get(key.as_ref()).cloned()
        }

        pub fn set(&self, key: impl Into<String>, value: impl Into<String>) {
            self.inner.lock().unwrap().insert(key.into(), value.into());
        }

        pub fn sid(&self) -> String {
            self.sid.to_string()
        }
    }
}

mod db {
    use futures::{future, Future};
    use redis::r#async::SharedConnection;
    use redis::{Client, IntoConnectionInfo};
    use std::sync::{Arc, RwLock};

    pub type Connection = SharedConnection;

    pub struct Db {
        client: Client,
        connection: Arc<RwLock<Option<Connection>>>,
    }

    impl Db {
        pub fn new(params: impl IntoConnectionInfo) -> Result<Db, redis::RedisError> {
            Ok(Db {
                client: Client::open(params)?,
                connection: Arc::new(RwLock::new(None)),
            })
        }
        pub fn get(&self) -> impl Future<Item = Connection, Error = redis::RedisError> + Send {
            let conn = self.connection.read().unwrap();

            if let Some(conn) = conn.as_ref() {
                let conn = conn.clone();
                future::Either::A(future::ok(conn))
            } else {
                drop(conn);
                log::info!("creating redis connection");
                let conn = self.connection.clone();
                let fut = self.client.get_shared_async_connection().map(move |c| {
                    log::info!("redis connection established");
                    let mut conn = conn.write().unwrap();
                    *conn = Some(c.clone());
                    c
                });
                future::Either::B(fut)
            }
        }
    }
}

mod posts {
    use futures::{future, Future};
    use redis::PipelineCommands;
    use serde_derive::{Deserialize, Serialize};

    use super::db::Connection;
    use super::users::{MaybeUser, User};
    use super::Authenticated;

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

    #[derive(Debug)]
    pub enum Error {
        NotFound,
        Redis(redis::RedisError),
    }

    impl Error {
        pub fn reject(self) -> warp::Rejection {
            log::error!("post error {:?}", self);
            match self {
                Error::NotFound => warp::reject::not_found(),
                e => warp::reject::custom(e.to_string()),
            }
        }
    }

    impl std::fmt::Display for Error {
        fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            match self {
                Error::NotFound => write!(f, "Post Not Found"),
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

    pub struct PostClient {
        db: Connection,
    }

    impl PostClient {
        pub fn new(db: Connection) -> PostClient {
            PostClient { db }
        }

        pub fn get_all(
            &self,
            limit: i64,
            skip: i64,
        ) -> impl Future<Item = PostPage, Error = Error> {
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
                    let author_map: HashMap<_, _> =
                        authors.into_iter().map(|u| (u.id, u)).collect();

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
                .and_then(|(conn, post): (_, MaybePost)| {
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
                        future::Either::B(future::err(Error::NotFound))
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
            post.author_id = self.user.id;
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
                        future::Either::A(future::err(Error::NotFound))
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

}

mod users {
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
}
