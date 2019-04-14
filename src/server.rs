use futures::{future, Future};
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

type Db = Arc<db::Db>;

pub fn run(config: Config) {
    let db = Arc::new(db::Db::new(&*config.redis_url.to_string()).unwrap());

    let config = Arc::new(config);
    let server_config = config.clone();

    let db = warp::any()
        .map(move || db.clone())
        .and_then(|db: Db| db.get().map_err(reject_with));

    let config = warp::any().map(move || config.clone());

    let session = warp::any()
        .and(db.clone())
        .and(config.clone())
        .map(|db, config: Arc<Config>| Session::new(db, &config.session_key));

    let maybe_cookie = |name: &'static str| {
        warp::cookie(name)
            .map(|c| Some(c))
            .or(warp::any().map(|| None))
            .unify()
    };

    let session_store = warp::ext::get::<Store>()
        .or(warp::any()
            .and(session.clone())
            .and(warp::addr::remote().and_then(|addr| match addr {
                Some(addr) => future::ok(addr),
                None => future::err(reject_with(Error::IpRequired)),
            }))
            .and(maybe_cookie("sid"))
            .and_then(
                |session: Session, ip: std::net::SocketAddr, sid: Option<String>| {
                    session
                        .get_store(ip.ip(), sid)
                        .map(|store| {
                            warp::ext::set(store.clone());
                            store
                        })
                        .from_err::<Error>()
                        .map_err(reject_with)
                },
            ))
        .unify();

    let maybe_auth = warp::any()
        .and(session_store.clone())
        .and(db.clone())
        .and_then(|ses: Store, conn| {
            let id = ses.get("socialUser");
            if let Some(social_id) = id {
                let client = users::UserClient::new(conn);
                future::Either::A(client.get_social_user(social_id).map(Some))
            } else {
                future::Either::B(future::ok(None))
            }
            .map_err(reject_with)
        });

    let auth = maybe_auth.clone().and_then(|user| {
        if let Some(user) = user {
            future::ok(user)
        } else {
            future::err(Error::Unauthorized)
        }
        .map_err(reject_with)
    });

    let view_index = warp::path::end()
        .and(maybe_auth.clone())
        .and(db.clone())
        .and_then(|user, conn| views::index(user, conn, None).map_err(reject_with));

    let view_page = warp::path("page")
        .and(warp::path::param::<i64>())
        .and(warp::path::end())
        .and(maybe_auth.clone())
        .and(db.clone())
        .and_then(|page, user, conn| views::index(user, conn, Some(page)).map_err(reject_with));

    let view_post_id = warp::path("post")
        .and(warp::path::param::<u64>())
        .and(warp::path::end())
        .and(maybe_auth.clone())
        .and(db.clone())
        .and_then(|post, user, conn| views::post_id(user, conn, post).map_err(reject_with));

    let view_post_frag = warp::path("post")
        .and(warp::path::param::<String>())
        .and(warp::path::end())
        .and(maybe_auth.clone())
        .and(db.clone())
        .and_then(|post, user, conn| views::post_frag(user, conn, post).map_err(reject_with));

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
                "</css/bundle.css>; rel=preload; as=style, </js/nickmass_com_client.js>; rel=preload; as=script; crossorigin, </js/nickmass_com_client_bg.wasm>; rel=preload; as=fetch; crossorigin",
            )
        });

    let json_body = warp::body::content_length_limit(1024 * 1024 * 5).and(warp::body::json());

    let posts = warp::path("posts");
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
                .map_err(reject_with)
        });

    let posts_get = warp::get2()
        .and(posts_id)
        .and(db.clone())
        .and_then(|id, conn| {
            let client = posts::PostClient::new(conn);
            client
                .get(id)
                .map(|post| warp::reply::json(&post))
                .map_err(reject_with)
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
                    .map_err(reject_with)
            });

    let posts_post = warp::post2()
        .and(posts_index)
        .and(json_body)
        .and(auth.clone())
        .and(db.clone())
        .and_then(|post, user, conn| {
            let client = auth::Authenticated::new(user, posts::PostClient::new(conn));

            client
                .create(post)
                .map(|id| warp::reply::json(&id))
                .map_err(reject_with)
        });

    let posts_put = warp::put2()
        .and(posts_index)
        .and(posts_id)
        .and(json_body)
        .and(auth.clone())
        .and(db.clone())
        .and_then(|id, post: posts::Post, user, conn| {
            let client = auth::Authenticated::new(user, posts::PostClient::new(conn));

            client
                .update(id, post)
                .map(|id| warp::reply::json(&id))
                .map_err(reject_with)
        });

    let posts_delete = warp::delete2()
        .and(posts_index)
        .and(posts_id)
        .and(auth.clone())
        .and(db.clone())
        .and_then(|id, user, conn| {
            let client = auth::Authenticated::new(user, posts::PostClient::new(conn));

            client
                .delete(id)
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

    let api = warp::path("api").and(posts_api.or(users_api).recover(recover_json));

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
        .and(warp::query::<auth::OauthResponse>())
        .and(config.clone())
        .and_then(
            |store: Store, oauth: auth::OauthResponse, config: Arc<Config>| {
                let client = reqwest::r#async::Client::new();
                let redirect_uri = format!("{}auth/google/return", config.base_url);
                client
                    .post(&config.oauth_token_url.to_string())
                    .form(&auth::OauthTokenRequest {
                        code: &oauth.code,
                        client_id: &config.oauth_id,
                        client_secret: &config.oauth_secret,
                        redirect_uri: redirect_uri.as_str(),
                        grant_type: "authorization_code",
                    })
                    .send()
                    .and_then(|mut res| res.json::<auth::OauthTokenResponse>())
                    /*
                    .and_then(|res| res.into_body().concat2())
                    .map(|body| {
                        eprintln!("b: {:?}", body);
                        serde_json::from_slice(&body).unwrap()
                    })*/
                    .from_err::<Error>()
                    .map_err(reject_with)
                    .map(move |res: auth::OauthTokenResponse| {
                        store.set("socialUser", format!("google:{}", res.id_token.claims.sub));
                        warp::redirect(config.base_url.clone())
                    })
            },
        )
        .and(session.clone())
        .and(session_store.clone())
        .and_then(|reply, session: Session, store: Store| {
            let sid = store.sid();
            session
                .set_store(store)
                .from_err::<Error>()
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
                .map_err(reject_with)
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

    let socket_addr: std::net::SocketAddr =
        (server_config.listen_ip, server_config.listen_port).into();

    info!("Server starting on {}", socket_addr);
    warp::serve(
        api.or(views
            .or(logout)
            .or(google_auth)
            .or(static_content)
            .recover(recover_html))
            .with(logger),
    )
    .run(socket_addr);
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
    } else if let Some(err) = err.find_cause::<Error>() {
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
    } else if let Some(err) = err.find_cause::<Error>() {
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
