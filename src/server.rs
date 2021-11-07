use axum::body::Body;
use axum::error_handling::{HandleErrorExt, HandleErrorLayer};
use axum::extract::{ConnectInfo, Extension, Path, Query, RequestParts};
use axum::handler::Handler;
use axum::http::{header, HeaderValue, Request};
use axum::response::{Headers, Html, IntoResponse};
use axum::routing::get;
use axum::{async_trait, AddExtensionLayer, Json, Router};
use reqwest::StatusCode;
use tower::filter::AsyncFilterLayer;
use tower::ServiceBuilder;

use std::net::SocketAddr;
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

use auth::Authenticated;
use db::Db;
use error::{Error, JsonError};
use posts::{Post, PostClient, PostPage};
use sessions::{Session, Store};
use users::{User, UserClient};

const CSP_DIRECTIVE: &'static str = "default-src 'none'; connect-src 'self'; font-src 'self'; frame-src https://www.youtube.com; img-src 'self' https://img.youtube.com; media-src 'self'; script-src 'self' 'unsafe-eval'; style-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'";

pub async fn run(config: Config) {
    let config = Arc::new(config);
    let db = Db::new(config.redis_url.to_string()).unwrap();
    let session = Arc::new(Session::new(config.session_key.as_slice()));

    let html_layers = ServiceBuilder::new()
        .layer(AddExtensionLayer::new(config.clone()))
        .layer(AddExtensionLayer::new(db.clone()))
        .layer(AddExtensionLayer::new(session.clone()))
        .layer(HandleErrorLayer::new(handle_html_error))
        .layer(AsyncFilterLayer::new(add_session))
        .layer(
            tower_http::set_header::SetResponseHeaderLayer::<_, Body>::if_not_present(
                header::CONTENT_SECURITY_POLICY,
                HeaderValue::from_static(CSP_DIRECTIVE),
            ),
        );

    let api_layers = ServiceBuilder::new()
        .layer(AddExtensionLayer::new(config.clone()))
        .layer(AddExtensionLayer::new(db.clone()))
        .layer(AddExtensionLayer::new(session.clone()))
        .layer(HandleErrorLayer::new(handle_json_error))
        .layer(AsyncFilterLayer::new(add_session));

    let auth_layers = ServiceBuilder::new()
        .layer(AddExtensionLayer::new(config.clone()))
        .layer(AddExtensionLayer::new(db.clone()))
        .layer(AddExtensionLayer::new(session.clone()))
        .layer(HandleErrorLayer::new(handle_html_error))
        .layer(AsyncFilterLayer::new(add_session));

    tracing::info!("serving assets from: {}", config.asset_dir);
    let public_static = tower_http::services::ServeDir::new(config.asset_dir.as_str())
        .handle_error(|e| {
            tracing::error!("static file error: {}", e);
            StatusCode::INTERNAL_SERVER_ERROR
        });

    let static_files = Router::new()
        .route("/css/*path", public_static.clone())
        .route("/fonts/*path", public_static.clone())
        .route("/img/*path", public_static.clone())
        .route("/js/*path", public_static.clone());

    let api = Router::new()
        .route("/users/current", get(api_user))
        .route("/posts", get(api_posts_get_all).post(api_posts_post))
        .route(
            "/posts/:post",
            get(api_posts_get)
                .put(api_posts_put)
                .delete(api_posts_delete),
        )
        .layer(api_layers)
        .fallback(api_fallback.into_service());

    let auth = Router::new()
        .route("/logout", get(auth_logout))
        .route("/google", get(auth_google))
        .route("/google/return", get(auth_google_return))
        .layer(auth_layers);

    let app = Router::new()
        .route("/", get(view_index))
        .route("/page/:page", get(view_page))
        .route("/post/:post", get(view_post))
        .layer(html_layers)
        .nest("/api", api)
        .nest("/auth", auth)
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .merge(static_files)
        .fallback(view_fallback.into_service());

    tracing::info!(
        "starting server on: {}:{}",
        config.listen_ip,
        config.listen_port
    );
    axum::Server::bind(&(config.listen_ip, config.listen_port).into())
        .serve(app.into_make_service_with_connect_info::<SocketAddr, _>())
        .await
        .unwrap()
}

async fn add_session(mut req: Request<Body>) -> Result<Request<Body>, Error> {
    let addr = req.extensions().get::<ConnectInfo<SocketAddr>>();
    let db = req.extensions().get::<Db>();
    let session = req.extensions().get::<Arc<Session>>();

    if let (Some(db), Some(ConnectInfo(addr)), Some(session)) = (db, addr, session) {
        let sid = cookie_value(&req, "sid").map(String::from);

        let ip = addr.ip();
        let mut connection = db.get().await?;

        let store = session.get_store(&mut connection, ip, sid).await;
        req.extensions_mut().insert(store);
    }
    Ok(req)
}

fn cookie_value<'r>(request: &'r Request<Body>, key: &str) -> Option<&'r str> {
    request
        .headers()
        .get(header::COOKIE)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| {
            s.split(';')
                .map(|v| v.split_once('='))
                .flatten()
                .find(|(k, _v)| k.trim() == key)
                .map(|(_k, v)| v.trim())
        })
}

async fn view_index(
    Extension(db): Extension<Db>,
    user: Option<Auth>,
) -> Result<Html<String>, HtmlError> {
    let user = user.map(|Auth(user)| user);
    Ok(Html(views::index(user, db.get().await?, None).await?))
}

async fn view_page(
    Extension(db): Extension<Db>,
    user: Option<Auth>,
    Path(page): Path<i64>,
) -> Result<Html<String>, HtmlError> {
    let user = user.map(|Auth(user)| user);
    Ok(Html(views::index(user, db.get().await?, Some(page)).await?))
}

async fn view_post(
    Extension(db): Extension<Db>,
    user: Option<Auth>,
    Path(post): Path<String>,
) -> Result<Html<String>, HtmlError> {
    let user = user.map(|Auth(user)| user);

    let db = db.get().await?;

    let post = if let Some(post) = post.parse().ok() {
        views::post_id(user, db, post).await?
    } else {
        views::post_frag(user, db, post).await?
    };

    Ok(Html(post))
}

async fn view_fallback() -> HtmlError {
    Error::NotFound.into()
}

async fn api_user(ApiAuth(user): ApiAuth) -> Json<User> {
    Json(user)
}

async fn api_posts_get_all(Extension(db): Extension<Db>) -> Result<Json<PostPage>, JsonError> {
    let db = db.get().await?;
    let client = PostClient::new(db);
    let posts = client.get_all(100, 0).await?;

    Ok(Json(posts))
}

async fn api_posts_get(
    Extension(db): Extension<Db>,
    Path(post): Path<String>,
) -> Result<Json<Post>, JsonError> {
    let db = db.get().await?;
    let client = PostClient::new(db);

    let post = if let Some(post) = post.parse().ok() {
        client.get(post).await?
    } else {
        client.get_by_fragment(post).await?
    };

    Ok(Json(post))
}

async fn api_posts_post(
    Extension(db): Extension<Db>,
    ApiAuth(user): ApiAuth,
    Json(post): Json<Post>,
) -> Result<Json<u64>, JsonError> {
    let db = db.get().await?;
    let client = Authenticated::new(user, PostClient::new(db));

    let id = client.create(post).await?;

    Ok(Json(id))
}

async fn api_posts_put(
    Extension(db): Extension<Db>,
    ApiAuth(user): ApiAuth,
    Path(post_id): Path<u64>,
    Json(post): Json<Post>,
) -> Result<Json<u64>, JsonError> {
    let db = db.get().await?;
    let client = Authenticated::new(user, PostClient::new(db));

    let id = client.update(post_id, post).await?;

    Ok(Json(id))
}

async fn api_posts_delete(
    Extension(db): Extension<Db>,
    ApiAuth(user): ApiAuth,
    Path(post_id): Path<u64>,
) -> Result<Json<()>, JsonError> {
    let db = db.get().await?;
    let client = Authenticated::new(user, PostClient::new(db));

    client.delete(post_id).await?;

    Ok(Json(()))
}

async fn api_fallback() -> JsonError {
    Error::NotFound.into()
}

async fn auth_logout(Extension(config): Extension<Arc<Config>>) -> impl IntoResponse {
    let headers = Headers(vec![
        (header::LOCATION, config.base_url.to_string()),
        (
            header::SET_COOKIE,
            "sid=; Expires=Thu, 01 Jan 1970 00:00:00 GMT; Path=/; SameSite=Lax; HttpOnly"
                .to_string(),
        ),
        (
            header::CACHE_CONTROL,
            "no-cache, no-store, must-revalidate".to_string(),
        ),
    ]);

    (StatusCode::TEMPORARY_REDIRECT, headers)
}

async fn auth_google(
    Extension(db): Extension<Db>,
    Extension(config): Extension<Arc<Config>>,
    Extension(session): Extension<Arc<Session>>,
    Extension(store): Extension<Store>,
) -> Result<impl IntoResponse, HtmlError> {
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
    let sid = store.sid();
    let mut connection = db.get().await?;

    session.set_store(&mut connection, store).await;

    let headers = Headers(vec![
        (header::LOCATION, http_uri),
        (
            header::SET_COOKIE,
            format!(
                "sid={}; Max-Age={}; Path=/; SameSite=Lax; HttpOnly",
                sid,
                60 * 60 * 24 * 30
            ),
        ),
        (
            header::CACHE_CONTROL,
            "no-cache, no-store, must-revalidate".to_string(),
        ),
    ]);

    Ok((StatusCode::TEMPORARY_REDIRECT, headers))
}
async fn auth_google_return(
    Extension(db): Extension<Db>,
    Extension(config): Extension<Arc<Config>>,
    Extension(session): Extension<Arc<Session>>,
    Extension(store): Extension<Store>,
    Query(oauth): Query<auth::OauthResponse>,
) -> Result<impl IntoResponse, HtmlError> {
    let client = reqwest::Client::new();
    let redirect_uri = format!("{}auth/google/return", config.base_url);
    let nounce = store.get("socialNounce");

    if Some(oauth.state) != nounce {
        Err(Error::Unauthorized.into())
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
            .map_err(Error::from)?;

        let token_res = raw_res
            .json::<auth::OauthTokenResponse>()
            .await
            .map_err(Error::from)?;

        store.set(
            "socialUser",
            format!("google:{}", token_res.id_token.claims.sub),
        );
        let sid = store.sid();

        let mut connection = db.get().await?;

        session.set_store(&mut connection, store).await;

        let headers = Headers(vec![
            (header::LOCATION, config.base_url.to_string()),
            (
                header::SET_COOKIE,
                format!(
                    "sid={}; Max-Age={}; Path=/; SameSite=Lax; HttpOnly",
                    sid,
                    60 * 60 * 24 * 30
                ),
            ),
            (
                header::CACHE_CONTROL,
                "no-cache, no-store, must-revalidate".to_string(),
            ),
        ]);

        Ok((StatusCode::TEMPORARY_REDIRECT, headers))
    }
}

fn handle_html_error(error: axum::BoxError) -> (StatusCode, Html<String>) {
    if let Some(error) = error.downcast_ref::<Error>() {
        if error.status_code() >= 500 && error.status_code() < 600 {
            tracing::error!("server error: {}", error);
        }
        (
            error.status(),
            Html(views::error(None, error).unwrap_or(error.to_string())),
        )
    } else {
        tracing::error!("unhandled error: {}", error);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Html(views::error(None, &Error::NotFound).unwrap_or(error.to_string())),
        )
    }
}

fn handle_json_error(error: axum::BoxError) -> (StatusCode, Json<JsonError>) {
    if let Some(error) = error.downcast_ref::<Error>() {
        if error.status_code() >= 500 && error.status_code() < 600 {
            tracing::error!("server error: {}", error);
        }
        (error.status(), Json(error.json()))
    } else {
        tracing::error!("unhandled error: {}", error);
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(JsonError {
                code: 500,
                message: "internal error".to_string(),
            }),
        )
    }
}

struct HtmlError(Error);

impl From<Error> for HtmlError {
    fn from(err: Error) -> Self {
        HtmlError(err)
    }
}

impl IntoResponse for HtmlError {
    type Body = axum::body::Full<axum::body::Bytes>;

    type BodyError = <Self::Body as axum::body::HttpBody>::Error;

    fn into_response(self) -> axum::http::Response<Self::Body> {
        let status = self.0.status_code();

        let html = if status == 404 {
            views::not_found(None)
        } else {
            tracing::error!("server error: {}", self.0);
            views::error(None, &self.0)
        };

        let res = (
            self.0.status(),
            Html(html.unwrap_or("internal server error".to_string())),
        );

        res.into_response()
    }
}

impl IntoResponse for JsonError {
    type Body = axum::body::Full<axum::body::Bytes>;

    type BodyError = <Self::Body as axum::body::HttpBody>::Error;

    fn into_response(self) -> axum::http::Response<Self::Body> {
        if self.code >= 500 && self.code < 600 {
            tracing::error!("server error: {}", self.message);
        }
        let res = (
            StatusCode::from_u16(self.code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
            Json(self),
        );

        res.into_response()
    }
}

struct Auth(pub super::server::users::User);

#[async_trait]
impl<B> axum::extract::FromRequest<B> for Auth
where
    B: Send,
{
    type Rejection = HtmlError;

    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        let db = req.extensions().and_then(|ext| ext.get::<Db>());

        let user = if let Some(db) = db {
            let db = db.clone().get().await?;
            let store = req.extensions().and_then(|ext| ext.get::<Store>());

            if let Some(store) = store {
                let id = store.get("socialUser");
                if let Some(social_id) = id {
                    let mut client = UserClient::new(db);
                    client.get_social_user(social_id).await.map(Some)?
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let user = user.ok_or(Error::Unauthorized)?;

        Ok(Auth(user))
    }
}

struct ApiAuth(pub super::server::users::User);

#[async_trait]
impl<B> axum::extract::FromRequest<B> for ApiAuth
where
    B: Send,
{
    type Rejection = JsonError;

    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        let db = req.extensions().and_then(|ext| ext.get::<Db>());

        let user = if let Some(db) = db {
            let db = db.clone().get().await?;
            let store = req.extensions().and_then(|ext| ext.get::<Store>());

            if let Some(store) = store {
                let id = store.get("socialUser");
                if let Some(social_id) = id {
                    let mut client = UserClient::new(db);
                    client.get_social_user(social_id).await.map(Some)?
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            None
        };

        let user = user.ok_or(Error::Unauthorized)?;

        Ok(ApiAuth(user))
    }
}
