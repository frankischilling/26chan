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
