#![forbid(unsafe_code)]

pub mod config;
mod http;
pub mod paired;

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
    staff_token: Option<String>,
    requests: Arc<Semaphore>,
}

impl AppState {
    pub fn new(
        store: IntakeStore,
        quarantine: Quarantine,
        token: String,
    ) -> Result<Self, config::ConfigError> {
        Self::with_staff_token(store, quarantine, token, None)
    }

    /// Allow a distinct staff service credential for the same intake operations.
    /// This credential does not carry staff identity or grant staff authority.
    pub fn with_staff_token(
        store: IntakeStore,
        quarantine: Quarantine,
        token: String,
        staff_token: Option<String>,
    ) -> Result<Self, config::ConfigError> {
        if !config::valid_credentials(&token, staff_token.as_deref()) {
            return Err(config::ConfigError);
        }
        Ok(Self {
            store,
            quarantine: Arc::new(quarantine),
            access: Access {
                token,
                staff_token,
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

/// Local qualification only. The production binary uses `router`, which has no
/// v2 routes. This harness retains v1 authentication, deadlines, and concurrency.
#[cfg(feature = "database-tests")]
pub fn paired_qualification_router(state: AppState) -> Router {
    Router::new()
        .route("/v2/reservations", post(paired::reserve))
        .route("/v2/uploads/{id}", put(paired::upload))
        .route("/v1/uploads/{id}", get(http::status))
        .layer(middleware::from_fn_with_state(
            state.access.clone(),
            http::protect,
        ))
        .layer(middleware::from_fn(board_http::retain_response_body))
        .with_state(state)
}
