use axum::body::Body;
use axum::extract::{ConnectInfo, Extension, Path, Query, State};
use axum::http::request::Parts;
use axum::http::{header, HeaderValue, Request, StatusCode};
use axum::middleware::Next;
use axum::response::{Html, IntoResponse, IntoResponseParts, Redirect};
use axum::routing::{get, get_service};
use axum::{async_trait, Json, RequestPartsExt, Router};
use axum_extra::extract::cookie::Cookie;
use axum_extra::{headers, TypedHeader};
use tower::ServiceBuilder;
use tower_http::classify::ServerErrorsFailureClass;
use tower_http::trace::{MakeSpan, OnFailure, OnRequest, OnResponse};

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
use sessions::{Session, SessionStore};
use users::{User, UserClient};

const CSP_DIRECTIVE: &'static str = "default-src 'none'; connect-src 'self'; font-src 'self'; frame-src https://www.youtube.com; img-src 'self' https://img.youtube.com; media-src 'self'; script-src 'self' 'unsafe-eval'; style-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'";

#[derive(axum::extract::FromRef, Clone)]
struct ServerState {
    config: Arc<Config>,
    db: Db,
    session: Arc<Session>,
}

pub async fn run(config: Config) {
    let config = Arc::new(config);
    let db = Db::new(config.redis_url.to_string()).unwrap();
    let session = Arc::new(Session::new(config.session_key.as_slice()));

    let state = ServerState {
        config: config.clone(),
        db,
        session,
    };

    let html_layers = ServiceBuilder::new().layer(
        tower_http::set_header::SetResponseHeaderLayer::<_>::if_not_present(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static(CSP_DIRECTIVE),
        ),
    );

    tracing::info!("serving assets from: {}", config.asset_dir);
    let public_static = get_service(
        tower_http::services::ServeDir::new(config.asset_dir.as_str())
            .append_index_html_on_directories(false),
    );

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
        .with_session_layer::<JsonError>(state.clone())
        .fallback(api_fallback);

    let auth = Router::new()
        .route("/logout", get(auth_logout))
        .route("/google", get(auth_google))
        .route("/google/return", get(auth_google_return));

    let app = Router::new()
        .route("/", get(view_index))
        .route("/page/:page", get(view_page))
        .route("/post/:post", get(view_post))
        .nest("/auth", auth)
        .with_session_layer::<HtmlError>(state.clone())
        .layer(html_layers)
        .nest("/api", api)
        .layer(
            tower_http::trace::TraceLayer::new_for_http()
                .make_span_with(MassTraceLog)
                .on_request(MassTraceLog)
                .on_response(MassTraceLog)
                .on_failure(MassTraceLog),
        )
        .merge(static_files)
        .fallback(view_fallback)
        .with_state(state.clone());

    tracing::info!(
        "starting server on: {}:{}",
        config.listen_ip,
        config.listen_port
    );

    let listener = tokio::net::TcpListener::bind(&(config.listen_ip, config.listen_port))
        .await
        .unwrap();

    let mut make_service = app.into_make_service_with_connect_info::<SocketAddr>();

    let (close_tx, close_rx) = tokio::sync::watch::channel(());

    loop {
        let (socket, remote_addr) = tokio::select! {
            _ = shutdown() => break,
            conn = listener.accept() => conn.unwrap(),
        };

        use tower::Service;
        let tower_service = make_service.call(remote_addr).await.unwrap();

        let close_rx = close_rx.clone();

        tokio::spawn(async move {
            let socket = hyper_util::rt::TokioIo::new(socket);

            let hyper_service =
                hyper::service::service_fn(move |request: Request<hyper::body::Incoming>| {
                    tower_service.clone().call(request)
                });

            let conn = hyper::server::conn::http1::Builder::new()
                .serve_connection(socket, hyper_service)
                .with_upgrades();

            let mut conn = std::pin::pin!(conn);

            loop {
                tokio::select! {
                    result = conn.as_mut() => {
                        if let Err(err) = result {
                            tracing::error!("failed to serve connection: {err:#}");
                        }
                        break;
                    },
                    _ = shutdown() => {
                        conn.as_mut().graceful_shutdown();
                    }
                }
            }

            drop(close_rx);
        });
    }

    drop(close_rx);
    drop(listener);
    tracing::info!(
        "signal received shutting down, waiting for {} tasks to complete",
        close_tx.receiver_count()
    );
    close_tx.closed().await;
}

async fn shutdown() {
    use tokio::signal::unix::{signal, SignalKind};

    let mut term = signal(SignalKind::terminate()).expect("unabled to listen to sigterm");
    let mut int = signal(SignalKind::interrupt()).expect("unabled to listen to sigint");

    tokio::select! {
        _ = term.recv() => (),
        _ = int.recv() => (),
    }
}

#[derive(Clone, Copy, Debug)]
struct MassTraceLog;

impl<B> MakeSpan<B> for MassTraceLog {
    fn make_span(&mut self, request: &Request<B>) -> tracing::Span {
        let id = uuid::Uuid::new_v4();
        if let Some(ConnectInfo(conn)) = request.extensions().get::<ConnectInfo<SocketAddr>>() {
            tracing::span!(
                tracing::Level::DEBUG,
                "request",
                method = %request.method(),
                uri = %request.uri(),
                remote_addr = %conn,
                request_id = %id,
                status_code = tracing::field::Empty
            )
        } else {
            tracing::span!(
                tracing::Level::DEBUG,
                "request",
                method = %request.method(),
                uri = %request.uri(),
                request_id = %id,
                status_code = tracing::field::Empty,
            )
        }
    }
}

impl<B> OnRequest<B> for MassTraceLog {
    fn on_request(&mut self, _request: &Request<B>, _span: &tracing::Span) {
        tracing::info!("request started");
    }
}

impl<B> OnResponse<B> for MassTraceLog {
    fn on_response(
        self,
        response: &http::Response<B>,
        latency: std::time::Duration,
        span: &tracing::Span,
    ) {
        let status = response.status().as_u16().to_string();
        span.record("status_code", &tracing::field::display(status));

        tracing::info!(
            latency_ms = latency.as_secs_f64() * 1000.0,
            status = response.status().as_u16(),
            "request completed"
        )
    }
}

impl OnFailure<ServerErrorsFailureClass> for MassTraceLog {
    fn on_failure(
        &mut self,
        _failure: ServerErrorsFailureClass,
        latency: std::time::Duration,
        _span: &tracing::Span,
    ) {
        tracing::error!(
            latency_ms = latency.as_secs_f64() * 1000.0,
            "request failed"
        )
    }
}

impl SessionLayerExt<ServerState> for Router<ServerState> {
    fn with_session_layer<E: From<Error> + IntoResponse + 'static>(
        self,
        state: ServerState,
    ) -> Self {
        self.layer(axum::middleware::from_fn_with_state(
            state,
            add_session::<E>,
        ))
    }
}

trait SessionLayerExt<S> {
    fn with_session_layer<E: From<Error> + IntoResponse + 'static>(self, state: S) -> Self;
}

#[derive(Clone)]
struct SessionClear;

impl IntoResponse for SessionClear {
    fn into_response(self) -> axum::response::Response {
        (self, ()).into_response()
    }
}

impl IntoResponseParts for SessionClear {
    type Error = std::convert::Infallible;

    fn into_response_parts(
        self,
        mut res: axum::response::ResponseParts,
    ) -> Result<axum::response::ResponseParts, Self::Error> {
        res.extensions_mut().insert(SessionClear);
        Ok(res)
    }
}

impl IntoResponse for SessionStore {
    fn into_response(self) -> axum::response::Response {
        (self, ()).into_response()
    }
}

impl IntoResponseParts for SessionStore {
    type Error = std::convert::Infallible;

    fn into_response_parts(
        self,
        mut res: axum::response::ResponseParts,
    ) -> Result<axum::response::ResponseParts, Self::Error> {
        res.extensions_mut().insert(self);
        Ok(res)
    }
}

async fn add_session<E: From<Error> + IntoResponse>(
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    db: State<Db>,
    session: State<Arc<Session>>,
    jar: axum_extra::extract::CookieJar,
    mut req: Request<Body>,
    next: Next,
) -> Result<impl IntoResponse, E> {
    let sid = jar.get("sid").map(|c| c.value().to_string());

    let ip = addr.ip();
    let mut connection = db.get().await?;

    let store = session.get_store(&mut connection, ip, sid).await;
    req.extensions_mut().insert(store);

    let mut res = next.run(req).await;
    if let Some(_) = res.extensions_mut().remove::<SessionClear>() {
        let cookie = Cookie::build(("sid", ""))
            .path("/")
            .http_only(true)
            .same_site(axum_extra::extract::cookie::SameSite::Lax);
        Ok((jar.remove(cookie), res).into_response())
    } else if let Some(store) = res.extensions_mut().remove::<SessionStore>() {
        let sid = store.sid();
        session.set_store(&mut connection, store).await;
        let cookie = Cookie::build(("sid", sid))
            .path("/")
            .http_only(true)
            .max_age(time::Duration::days(30))
            .same_site(axum_extra::extract::cookie::SameSite::Lax);
        Ok((jar.add(cookie), res).into_response())
    } else {
        Ok(res.into_response())
    }
}

async fn view_index(
    State(db): State<Db>,
    user: Option<HtmlAuth>,
) -> Result<Html<String>, HtmlError> {
    let user = user.map(|HtmlAuth(user)| user);
    Ok(Html(views::index(user, db.get().await?, None).await?))
}

async fn view_page(
    State(db): State<Db>,
    user: Option<HtmlAuth>,
    Path(page): Path<i64>,
) -> Result<Html<String>, HtmlError> {
    let user = user.map(|HtmlAuth(user)| user);
    Ok(Html(views::index(user, db.get().await?, Some(page)).await?))
}

async fn view_post(
    State(db): State<Db>,
    user: Option<HtmlAuth>,
    Path(post): Path<String>,
) -> Result<Html<String>, HtmlError> {
    let user = user.map(|HtmlAuth(user)| user);

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

async fn api_posts_get_all(State(db): State<Db>) -> Result<Json<PostPage>, JsonError> {
    let db = db.get().await?;
    let client = PostClient::new(db);
    let posts = client.get_all(100, 0).await?;

    Ok(Json(posts))
}

async fn api_posts_get(
    State(db): State<Db>,
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
    State(db): State<Db>,
    ApiAuth(user): ApiAuth,
    Json(post): Json<Post>,
) -> Result<Json<u64>, JsonError> {
    let db = db.get().await?;
    let client = Authenticated::new(user, PostClient::new(db));

    let id = client.create(post).await?;

    Ok(Json(id))
}

async fn api_posts_put(
    State(db): State<Db>,
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
    State(db): State<Db>,
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

async fn auth_logout(State(config): State<Arc<Config>>) -> impl IntoResponse {
    let no_cache = headers::CacheControl::new().with_no_store();

    (
        SessionClear,
        TypedHeader(no_cache),
        Redirect::temporary(&config.base_url.to_string()),
    )
}

async fn auth_google(
    State(config): State<Arc<Config>>,
    State(session): State<Arc<Session>>,
    store: SessionStore,
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

    let no_cache = headers::CacheControl::new().with_no_store();

    Ok((store, TypedHeader(no_cache), Redirect::temporary(&http_uri)))
}

async fn auth_google_return(
    State(config): State<Arc<Config>>,
    store: SessionStore,
    Query(oauth): Query<auth::OauthResponse>,
) -> Result<impl IntoResponse, HtmlError> {
    let client = reqwest::Client::new();
    let redirect_uri = format!("{}auth/google/return", config.base_url);
    let nounce = store.get("socialNounce");

    if Some(oauth.state) != nounce {
        return Err(Error::Unauthorized.into());
    }

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

    let no_cache = headers::CacheControl::new().with_no_store();

    Ok((
        store,
        TypedHeader(no_cache),
        Redirect::temporary(&config.base_url.to_string()),
    ))
}

struct HtmlError(Error);

impl From<Error> for HtmlError {
    fn from(err: Error) -> Self {
        HtmlError(err)
    }
}

impl IntoResponse for HtmlError {
    fn into_response(self) -> axum::response::Response {
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
    fn into_response(self) -> axum::response::Response {
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

#[async_trait]
impl axum::extract::FromRequestParts<ServerState> for SessionStore {
    type Rejection = axum::extract::rejection::ExtensionRejection;

    async fn from_request_parts(
        parts: &mut Parts,
        _state: &ServerState,
    ) -> Result<Self, Self::Rejection> {
        let Extension(store) = parts.extract::<Extension<SessionStore>>().await?;

        Ok(store)
    }
}

struct Auth<E>(pub super::server::users::User, std::marker::PhantomData<E>);

#[async_trait]
impl<E> axum::extract::FromRequestParts<ServerState> for Auth<E>
where
    E: IntoResponse + From<Error>,
{
    type Rejection = E;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &ServerState,
    ) -> Result<Self, Self::Rejection> {
        let db = parts
            .extract_with_state::<State<Db>, ServerState>(state)
            .await
            .ok();

        let user = if let Some(db) = db {
            let db = db.clone().get().await?;
            let store = parts.extract::<Extension<SessionStore>>().await.ok();

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

        Ok(Auth(user, Default::default()))
    }
}

impl From<Auth<HtmlError>> for HtmlAuth {
    fn from(auth: Auth<HtmlError>) -> Self {
        HtmlAuth(auth.0)
    }
}

struct HtmlAuth(pub super::server::users::User);

#[async_trait]
impl axum::extract::FromRequestParts<ServerState> for HtmlAuth {
    type Rejection = HtmlError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &ServerState,
    ) -> Result<Self, Self::Rejection> {
        Ok(Auth::<HtmlError>::from_request_parts(parts, state)
            .await?
            .into())
    }
}

impl From<Auth<JsonError>> for ApiAuth {
    fn from(auth: Auth<JsonError>) -> Self {
        ApiAuth(auth.0)
    }
}

struct ApiAuth(pub super::server::users::User);

#[async_trait]
impl axum::extract::FromRequestParts<ServerState> for ApiAuth {
    type Rejection = JsonError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &ServerState,
    ) -> Result<Self, Self::Rejection> {
        Ok(Auth::<JsonError>::from_request_parts(parts, state)
            .await?
            .into())
    }
}
