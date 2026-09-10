#![forbid(unsafe_code)]

mod api;
mod api_http;
mod handlers;
mod security;
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
    let (public, api) = routers(pool, origin, production);
    let public = metrics.layer(public, Listener::Public);
    let api = if api_enabled {
        metrics.layer(api, Listener::Api)
    } else {
        api
    };
    (metrics, public, api)
}

pub fn routers(pool: PgPool, origin: String, production: bool) -> (Router, Router) {
    let state = AppState {
        pool,
        origin,
        production,
        limits: Arc::new(security::Limits::default()),
    };
    let public = Router::new()
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
        .route("/{board}/report", post(handlers::report))
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
