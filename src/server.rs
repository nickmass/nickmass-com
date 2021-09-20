use log::{error, info, warn};
use warp::reject::custom as reject_with;
use warp::{self, Filter};

use std::sync::Arc;

mod auth;
mod config;
mod db;
mod error;
mod models;
mod posts;
mod sessions;
mod users;
mod views;

pub use config::Config;
pub use error::Error;

use sessions::{Session, Store};

pub async fn run(config: Config) {
    let db = db::with_db(config.redis_url.to_string()).unwrap();

    let config = Arc::new(config);
    let server_config = config.clone();

    let config = warp::any().map(move || config.clone());

    let session = warp::any()
        .and(db.clone())
        .and(config.clone())
        .map(|db, config: Arc<Config>| Session::new(db, &config.session_key));

    let session_store = warp::any()
        .and(session.clone())
        .and(warp::addr::remote().and_then(|addr| async move {
            match addr {
                Some(addr) => Ok(addr),
                None => Err(reject_with(Error::IpRequired)),
            }
        }))
        .and(maybe_cookie("sid"))
        .and_then(
            |session: Session, ip: std::net::SocketAddr, sid: Option<String>| async move {
                let store = session.get_store(ip.ip(), sid).await;
                Ok::<_, Error>(store).map_err(reject_with)
            },
        );

    let maybe_auth = warp::any()
        .and(session_store.clone())
        .and(db.clone())
        .and_then(|ses: Store, conn| {
            let id = ses.get("socialUser");
            async move {
                if let Some(social_id) = id {
                    let mut client = users::UserClient::new(conn);
                    client
                        .get_social_user(social_id)
                        .await
                        .map(Some)
                        .map_err(reject_with)
                } else {
                    Ok(None)
                }
            }
        });

    let auth = maybe_auth.clone().and_then(|user| async move {
        if let Some(user) = user {
            Ok(user)
        } else {
            Err(Error::Unauthorized).map_err(reject_with)
        }
    });

    let view_index = warp::path::end()
        .and(maybe_auth.clone())
        .and(db.clone())
        .and_then(
            |user, conn| async move { views::index(user, conn, None).await.map_err(reject_with) },
        );

    let view_page = warp::path("page")
        .and(warp::path::param::<i64>())
        .and(warp::path::end())
        .and(maybe_auth.clone())
        .and(db.clone())
        .and_then(|page, user, conn| async move {
            views::index(user, conn, Some(page))
                .await
                .map_err(reject_with)
        });

    let view_post_id = warp::path("post")
        .and(warp::path::param::<u64>())
        .and(warp::path::end())
        .and(maybe_auth.clone())
        .and(db.clone())
        .and_then(|post, user, conn| async move {
            views::post_id(user, conn, post).await.map_err(reject_with)
        });

    let view_post_frag = warp::path("post")
        .and(warp::path::param::<String>())
        .and(warp::path::end())
        .and(maybe_auth.clone())
        .and(db.clone())
        .and_then(|post, user, conn| async move {
            views::post_frag(user, conn, post)
                .await
                .map_err(reject_with)
        });

    let views = view_index
        .or(view_page)
        .unify()
        .or(view_post_id)
        .unify()
        .or(view_post_frag)
        .unify()
        .map(warp::reply::html)
        .map(|reply| warp::reply::with_header(reply, "Content-Security-Policy", "default-src 'none'; connect-src 'self'; font-src 'self'; frame-src https://www.youtube.com; img-src 'self' https://img.youtube.com; media-src 'self'; script-src 'self' 'unsafe-eval'; style-src 'self' 'unsafe-inline'; frame-ancestors 'none'"));

    let json_body = warp::body::content_length_limit(1024 * 1024 * 5).and(warp::body::json());

    let posts = warp::path("posts");
    let posts_index = posts.and(warp::path::end());
    let posts_id = posts.and(warp::path::param::<u64>()).and(warp::path::end());
    let posts_frag = posts
        .and(warp::path::param::<String>())
        .and(warp::path::end());

    let posts_get_all = warp::get()
        .and(posts_index)
        .and(db.clone())
        .and_then(|db| async move {
            let client = posts::PostClient::new(db);
            client
                .get_all(100, 0)
                .await
                .map(|posts| warp::reply::json(&posts))
                .map_err(reject_with)
        });

    let posts_get = warp::get()
        .and(posts_id)
        .and(db.clone())
        .and_then(|id, conn| async move {
            let client = posts::PostClient::new(conn);
            client
                .get(id)
                .await
                .map(|post| warp::reply::json(&post))
                .map_err(reject_with)
        });

    let posts_get_fragment =
        warp::get()
            .and(posts_frag)
            .and(db.clone())
            .and_then(|fragment, conn| async move {
                let client = posts::PostClient::new(conn);
                client
                    .get_by_fragment(fragment)
                    .await
                    .map(|post| warp::reply::json(&post))
                    .map_err(reject_with)
            });

    let posts_post = warp::post()
        .and(posts_index)
        .and(json_body)
        .and(auth.clone())
        .and(db.clone())
        .and_then(|post, user, conn| async move {
            let client = auth::Authenticated::new(user, posts::PostClient::new(conn));
            client
                .create(post)
                .await
                .map(|id| warp::reply::json(&id))
                .map_err(reject_with)
        });

    let posts_put = warp::put()
        .and(posts_id)
        .and(json_body)
        .and(auth.clone())
        .and(db.clone())
        .and_then(|id, post: posts::Post, user, conn| async move {
            let client = auth::Authenticated::new(user, posts::PostClient::new(conn));
            client
                .update(id, post)
                .await
                .map(|id| warp::reply::json(&id))
                .map_err(reject_with)
        });

    let posts_delete = warp::delete()
        .and(posts_id)
        .and(auth.clone())
        .and(db.clone())
        .and_then(|id, user, conn| async move {
            let client = auth::Authenticated::new(user, posts::PostClient::new(conn));
            client
                .delete(id)
                .await
                .map(|id| warp::reply::json(&id))
                .map_err(reject_with)
        });

    let posts_api = posts_get_all
        .or(posts_get)
        .or(posts_get_fragment)
        .or(posts_post)
        .or(posts_put)
        .or(posts_delete);

    let users_api = warp::path("users")
        .and(warp::path("current"))
        .and(warp::path::end())
        .and(auth.clone())
        .map(|user: users::User| warp::reply::json(&user));

    let api = warp::path("api").and(
        posts_api
            .or(users_api)
            .recover(|err| async move { recover_json(err) }),
    );

    let auth = warp::path("auth");

    let logout = auth
        .and(warp::path("logout"))
        .and(config.clone())
        .map(|config: Arc<Config>| {
            let reply = warp::reply::with_header(
                warp::http::StatusCode::TEMPORARY_REDIRECT,
                warp::http::header::LOCATION,
                config.base_url.to_string(),
            );
            let reply = warp::reply::with_header(
                reply,
                warp::http::header::SET_COOKIE,
                "sid=; Expires=Thu, 01 Jan 1970 00:00:00 GMT; Path=/; SameSite=Lax; HttpOnly",
            );
            warp::reply::with_header(
                reply,
                warp::http::header::CACHE_CONTROL,
                "no-cache, no-store, must-revalidate",
            )
        });

    let google = auth.and(warp::path("google"));

    let google_return = google.and(warp::path("return")).and(warp::path::end());
    let google_oath_return =
        warp::get()
            .and(google_return)
            .and(session.clone())
            .and(session_store.clone())
            .and(warp::query::<auth::OauthResponse>())
            .and(config.clone())
            .and_then(
                |session: Session,
                 store: Store,
                 oauth: auth::OauthResponse,
                 config: Arc<Config>| {
                    async move {
                        let client = reqwest::Client::new();
                        let redirect_uri = format!("{}auth/google/return", config.base_url);
                        let nounce = store.get("socialNounce");

                        if Some(oauth.state) != nounce {
                            return Err(Error::Unauthorized).map_err(reject_with);
                        } else {
                            let raw_res = client
                                .post(&config.oauth_token_url.to_string())
                                .form(&auth::OauthTokenRequest {
                                    code: &oauth.code,
                                    client_id: &config.oauth_id,
                                    client_secret: &config.oauth_secret,
                                    redirect_uri: redirect_uri.as_str(),
                                    grant_type: "authorization_code",
                                })
                                .send()
                                .await
                                .map_err(Error::from)
                                .map_err(reject_with)?;

                            let token_res = raw_res
                                .json::<auth::OauthTokenResponse>()
                                .await
                                .map_err(Error::from)
                                .map_err(reject_with)?;

                            store.set(
                                "socialUser",
                                format!("google:{}", token_res.id_token.claims.sub),
                            );
                            let sid = store.sid();
                            session.set_store(store).await;

                            let reply = warp::reply::with_header(
                                warp::http::StatusCode::TEMPORARY_REDIRECT,
                                warp::http::header::LOCATION,
                                config.base_url.to_string(),
                            );
                            let reply = warp::reply::with_header(
                                reply,
                                warp::http::header::SET_COOKIE,
                                format!(
                                    "sid={}; Max-Age={}; Path=/; SameSite=Lax; HttpOnly",
                                    sid,
                                    60 * 60 * 24 * 30
                                ),
                            );
                            let reply = warp::reply::with_header(
                                reply,
                                warp::http::header::CACHE_CONTROL,
                                "no-cache, no-store, must-revalidate",
                            );

                            Ok(reply)
                        }
                    }
                },
            );

    let google_login = warp::get()
        .and(google.and(warp::path::end()))
        .and(session.clone())
        .and(session_store.clone())
        .and(config.clone())
        .and_then(|session: Session, store: Store, config: Arc<Config>| {
            let redirect_uri = format!("{}auth/google/return", config.base_url);
            let social_nounce = session.create_nounce();
            store.set("socialNounce", social_nounce.as_str());
            let nounce = session.create_nounce();
            let auth_url = url::Url::parse_with_params(
                &*config.oauth_login_url.to_string(),
                &[
                    ("client_id", config.oauth_id.as_str()),
                    ("response_type", "code"),
                    ("scope", "openid email profile"),
                    ("redirect_uri", redirect_uri.as_str()),
                    ("state", social_nounce.as_str()),
                    ("nounce", nounce.as_str()),
                ],
            )
            .expect("Config allows valid google url");
            let http_uri = auth_url.to_string();

            let reply = warp::reply::with_header(
                warp::http::StatusCode::TEMPORARY_REDIRECT,
                warp::http::header::LOCATION,
                http_uri,
            );

            async move {
                let sid = store.sid();
                session.set_store(store).await;
                let reply = warp::reply::with_header(
                    reply,
                    warp::http::header::SET_COOKIE,
                    format!(
                        "sid={}; Max-Age={}; Path=/; SameSite=Lax; HttpOnly",
                        sid,
                        60 * 60 * 24 * 30
                    ),
                );
                let reply = warp::reply::with_header(
                    reply,
                    warp::http::header::CACHE_CONTROL,
                    "no-cache, no-store, must-revalidate",
                );

                Ok::<_, Error>(reply).map_err(reject_with)
            }
        });

    let google_auth = google_login.or(google_oath_return);

    let static_content = warp::fs::dir("./public");

    let logger = warp::log("nickmass_com::api");

    let socket_addr: std::net::SocketAddr =
        (server_config.listen_ip, server_config.listen_port).into();

    info!("Server starting on {}", socket_addr);
    let _server = warp::serve(
        api.or(views
            .or(logout)
            .or(google_auth)
            .or(static_content)
            .recover(|err| async move { recover_html(err) }))
            .with(logger),
    )
    .run(socket_addr)
    .await;
}

fn recover_json(err: warp::Rejection) -> Result<impl warp::Reply, warp::Rejection> {
    if err.is_not_found() {
        warn!("Not Found - {:?}", err);

        let err = Error::NotFound.to_json();

        let json = warp::reply::json(&err);

        Ok(warp::reply::with_status(
            json,
            warp::http::StatusCode::from_u16(err.code)
                .unwrap_or(warp::http::StatusCode::INTERNAL_SERVER_ERROR),
        ))
    } else if let Some(err) = err.find::<Error>() {
        error!("{} - {:?}", err, err);

        let err = err.to_json();

        let json = warp::reply::json(&err);

        Ok(warp::reply::with_status(
            json,
            warp::http::StatusCode::from_u16(err.code)
                .unwrap_or(warp::http::StatusCode::INTERNAL_SERVER_ERROR),
        ))
    } else {
        error!("Unhandled Error - {:?}", err);
        Err(err)
    }
}

fn recover_html(err: warp::Rejection) -> Result<impl warp::Reply, warp::Rejection> {
    if err.is_not_found() {
        warn!("Not Found - {:?}", err);

        let html = warp::reply::html(
            views::not_found(None).unwrap_or(String::from("Internal Server Error")),
        );

        Ok(warp::reply::with_status(
            html,
            warp::http::StatusCode::NOT_FOUND,
        ))
    } else if let Some(err) = err.find::<Error>() {
        error!("{} - {:?}", err, err);

        let html = warp::reply::html(
            views::error(None, err).unwrap_or(String::from("Internal Server Error")),
        );

        Ok(warp::reply::with_status(
            html,
            warp::http::StatusCode::from_u16(err.status_code())
                .unwrap_or(warp::http::StatusCode::INTERNAL_SERVER_ERROR),
        ))
    } else {
        error!("Unhandled Error - {:?}", err);
        Err(err)
    }
}

fn maybe_cookie(
    name: &'static str,
) -> impl Filter<Extract = (Option<String>,), Error = std::convert::Infallible> + Clone {
    warp::cookie(name)
        .map(|c| Some(c))
        .or(warp::any().map(|| None))
        .unify()
}
