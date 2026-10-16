#![forbid(unsafe_code)]
//! Isolated public verification authority. Challenge and email mutation routes
//! remain under construction; unavailable routes fail closed.
mod access;
mod challenge;
pub mod config;
mod handlers;
use axum::{Router, extract::DefaultBodyLimit, http::StatusCode, middleware, routing::post};
use board_domain::{challenge_secrets::VerificationKey, verification_mail::MailKey};
use board_store::verification::VerificationStore;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    store: VerificationStore,
    key: Arc<VerificationKey>,
    mail_key: Arc<MailKey>,
    access: access::Access,
    rendering: Arc<tokio::sync::Semaphore>,
}
impl AppState {
    pub fn new(
        store: VerificationStore,
        key: VerificationKey,
        mail_key: MailKey,
        service_token: &str,
    ) -> Result<Self, config::ConfigError> {
        Ok(Self {
            store,
            key: Arc::new(key),
            mail_key: Arc::new(mail_key),
            access: access::Access::new(service_token)?,
            rendering: Arc::new(tokio::sync::Semaphore::new(4)),
        })
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/readyz", post(handlers::ready))
        .route("/v1/session", post(handlers::ensure_session))
        .route("/v1/challenge", post(handlers::challenge))
        .route("/v1/signout", post(handlers::signout))
        .route("/v1/email/state", post(handlers::email_state))
        .route("/v1/email/request", post(handlers::request_email))
        .route("/v1/email/verify", post(handlers::verify_email))
        .fallback(|| async { access::error(StatusCode::NOT_FOUND) })
        .method_not_allowed_fallback(|| async { access::error(StatusCode::METHOD_NOT_ALLOWED) })
        .layer(DefaultBodyLimit::max(8192))
        .layer(middleware::from_fn_with_state(
            state.access.clone(),
            access::gate,
        ))
        .with_state(state)
}
