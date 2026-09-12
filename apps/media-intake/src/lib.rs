#![forbid(unsafe_code)]

pub mod config;
mod http;

use axum::{
    Router, middleware,
    routing::{get, post, put},
};
use board_media::Quarantine;
use board_store::media_intake::IntakeStore;
use std::sync::Arc;
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct AppState {
    store: IntakeStore,
    quarantine: Arc<Quarantine>,
    access: Access,
    uploads: Arc<Semaphore>,
}

#[derive(Clone)]
struct Access {
    token: String,
    requests: Arc<Semaphore>,
}

impl AppState {
    pub fn new(
        store: IntakeStore,
        quarantine: Quarantine,
        token: String,
    ) -> Result<Self, config::ConfigError> {
        if !config::valid_token(&token) {
            return Err(config::ConfigError);
        }
        Ok(Self {
            store,
            quarantine: Arc::new(quarantine),
            access: Access {
                token,
                requests: Arc::new(Semaphore::new(8)),
            },
            uploads: Arc::new(Semaphore::new(4)),
        })
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/v1/reservations", post(http::reserve))
        .route("/v1/uploads/{id}", put(http::upload).get(http::status))
        .route(
            "/healthz",
            get(|| async { axum::Json(serde_json::json!({"status":"ok"})) }),
        )
        .route("/readyz", get(http::ready))
        .fallback(|| async { http::error(axum::http::StatusCode::NOT_FOUND) })
        .method_not_allowed_fallback(|| async {
            http::error(axum::http::StatusCode::METHOD_NOT_ALLOWED)
        })
        .layer(middleware::from_fn_with_state(
            state.access.clone(),
            http::protect,
        ))
        .layer(middleware::from_fn(board_http::retain_response_body))
        .with_state(state)
}

pub fn observed_router(state: AppState) -> (board_observe::Metrics, Router) {
    let metrics = board_observe::Metrics::new();
    let app = metrics.layer(router(state), board_observe::Listener::Intake);
    (metrics, app)
}

#[cfg(test)]
mod tests;
