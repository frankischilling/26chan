#![forbid(unsafe_code)]

mod api;
mod api_http;
pub mod catalog;
mod handlers;
mod intake;
mod security;
pub mod themes;
mod ui_assets;
mod uploads;
mod views;
use axum::{
    Router,
    extract::DefaultBodyLimit,
    middleware,
    routing::{get, post},
};
use sqlx::PgPool;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pool: PgPool,
    origin: String,
    production: bool,
    limits: Arc<security::Limits>,
    media: Option<intake::IntakeClient>,
}

pub fn router(pool: PgPool, origin: String, production: bool) -> Router {
    routers(pool, origin, production).0
}

/// Build the serving routers and a process-local, fixed-label metrics snapshot.
pub fn observed_routers(
    pool: PgPool,
    origin: String,
    production: bool,
    api_enabled: bool,
) -> (board_observe::Metrics, Router, Router) {
    observed_routers_with_media(pool, origin, production, api_enabled, None)
}

pub fn observed_routers_with_media(
    pool: PgPool,
    origin: String,
    production: bool,
    api_enabled: bool,
    media: Option<board_config::PublicMediaSettings>,
) -> (board_observe::Metrics, Router, Router) {
    use board_observe::{Listener, Metrics, Pool, PoolSample};
    let mut metrics = Metrics::new();
    let observed_pool = pool.clone();
    metrics
        .register_pool(Pool::Public, move || PoolSample {
            size: observed_pool.size(),
            idle: observed_pool.num_idle(),
            max: observed_pool.options().get_max_connections(),
        })
        .expect("one pool registered before sharing metrics");
    let (public, api) = routers_with_media(pool, origin, production, media);
    let public = metrics.layer(public, Listener::Public);
    let api = if api_enabled {
        metrics.layer(api, Listener::Api)
    } else {
        api
    };
    (metrics, public, api)
}

pub fn routers(pool: PgPool, origin: String, production: bool) -> (Router, Router) {
    routers_with_media(pool, origin, production, None)
}

pub fn routers_with_media(
    pool: PgPool,
    origin: String,
    production: bool,
    media: Option<board_config::PublicMediaSettings>,
) -> (Router, Router) {
    assert!(
        !production || media.is_none(),
        "Production media is not qualified"
    );
    let state = AppState {
        pool,
        origin,
        production,
        limits: Arc::new(security::Limits::default()),
        media: media.map(|settings| intake::IntakeClient { settings }),
    };
    let mut public = Router::new()
        .merge(themes::routes(state.origin.clone(), state.production))
        .merge(ui_assets::routes())
        .route("/", get(handlers::home))
        .route("/healthz", get(|| async { "ok" }))
        .route("/readyz", get(handlers::ready))
        .route("/static/board.css", get(handlers::css))
        .route("/boards.json", get(api::boards))
        .route("/{board}", get(handlers::board_redirect))
        .route("/{board}/", get(handlers::board_index))
        .route("/{board}/thread/{key}", get(handlers::thread))
        .route("/{board}/post/{id}", get(handlers::quote))
        .route("/{board}/post", post(handlers::post))
        .route("/{board}/imgboard.php", post(handlers::post))
        .route("/{board}/delete", post(handlers::delete))
        .route("/{board}/report", post(handlers::report));
    if state.media.is_some() {
        public = public
            .route(
                "/{board}/upload",
                post(uploads::upload).layer(DefaultBodyLimit::max(8_388_608 + 16_384)),
            )
            .route("/{board}/upload/status", post(uploads::status))
            .route("/{board}/upload/cancel", post(uploads::cancel));
    }
    let public = public
        .route("/{board}/{page}", get(handlers::page))
        .fallback(handlers::not_found)
        // A 16,000-character comment can occupy 192,000 bytes when four-byte
        // UTF-8 characters are percent-encoded. Bound the collected body too.
        .layer(DefaultBodyLimit::max(262_144))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            security::protect,
        ))
        .layer(middleware::from_fn(board_http::retain_response_body))
        .with_state(state.clone());
    let api = api_http::router(state);
    (public, api)
}

pub async fn media_ready(settings: &board_config::PublicMediaSettings) -> Result<(), &'static str> {
    intake::IntakeClient {
        settings: settings.clone(),
    }
    .ready()
    .await
    .map_err(|_| "Media intake is unavailable.")
}
