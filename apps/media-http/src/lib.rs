#![forbid(unsafe_code)]

mod security;
mod serving;

use axum::{Router, middleware, routing::get};
use board_media::{ApprovedFiles, MediaError};
use board_store::media_assets::MediaReader;
use std::sync::Arc;
use tokio::sync::Semaphore;

#[derive(Clone)]
pub struct AppState {
    reader: MediaReader,
    files: Arc<ApprovedFiles>,
    authority: String,
    requests: Arc<Semaphore>,
    reads: Arc<Semaphore>,
}

impl AppState {
    pub fn new(reader: MediaReader, files: ApprovedFiles, origin: &board_config::Origin) -> Self {
        let origin = origin.as_string();
        Self {
            reader,
            files: Arc::new(files),
            authority: origin
                .split_once("://")
                .expect("parsed origin")
                .1
                .to_owned(),
            requests: Arc::new(Semaphore::new(16)),
            reads: Arc::new(Semaphore::new(4)),
        }
    }

    async fn blocking<T: Send + 'static>(
        &self,
        work: impl FnOnce() -> Result<T, MediaError> + Send + 'static,
    ) -> Result<T, axum::http::StatusCode> {
        blocking(self.reads.clone(), work).await
    }
}

async fn blocking<T: Send + 'static>(
    reads: Arc<Semaphore>,
    work: impl FnOnce() -> Result<T, MediaError> + Send + 'static,
) -> Result<T, axum::http::StatusCode> {
    let permit = reads
        .try_acquire_owned()
        .map_err(|_| axum::http::StatusCode::SERVICE_UNAVAILABLE)?;
    // A timed-out/cancelled handler cannot cancel spawn_blocking. Ownership
    // stays in the task so it cannot free admission while still reading.
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        work()
    })
    .await
    .map_err(|_| axum::http::StatusCode::SERVICE_UNAVAILABLE)?
    .map_err(|_| axum::http::StatusCode::SERVICE_UNAVAILABLE)
}

#[cfg(test)]
mod tests;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/media/{name}", get(serving::image))
        .route("/healthz", get(|| async { "ok" }))
        .route("/readyz", get(serving::ready))
        .fallback(|| async { security::error(axum::http::StatusCode::NOT_FOUND) })
        .method_not_allowed_fallback(|| async {
            security::error(axum::http::StatusCode::METHOD_NOT_ALLOWED)
        })
        .layer(middleware::from_fn_with_state(
            state.clone(),
            security::protect,
        ))
        .layer(middleware::from_fn(board_http::retain_response_body))
        .with_state(state)
}
