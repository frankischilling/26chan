#![forbid(unsafe_code)]
//! Isolated challenge and email verification authority. Only explicitly
//! registered, authenticated routes are available.
mod access;
mod challenge;
pub mod config;
mod handlers;
mod pass;
mod pass_checkout;
mod pass_purchase;
mod pass_recovery;
mod vip;
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
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
    pass_hashing: Arc<tokio::sync::Semaphore>,
    pass_dummy: Arc<String>,
}
impl AppState {
    pub fn new(
        store: VerificationStore,
        key: VerificationKey,
        mail_key: MailKey,
        service_token: &str,
    ) -> Result<Self, config::ConfigError> {
        let salt =
            SaltString::encode_b64(b"pass-dummy-salt-v1").map_err(|_| config::ConfigError)?;
        let dummy = Argon2::default()
            .hash_password(b"unrecognized-pass-credential", &salt)
            .map_err(|_| config::ConfigError)?
            .to_string();
        Ok(Self {
            store,
            key: Arc::new(key),
            mail_key: Arc::new(mail_key),
            access: access::Access::new(service_token)?,
            rendering: Arc::new(tokio::sync::Semaphore::new(4)),
            pass_hashing: Arc::new(tokio::sync::Semaphore::new(2)),
            pass_dummy: Arc::new(dummy),
        })
    }
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/readyz", post(handlers::ready))
        .route("/v1/session", post(handlers::ensure_session))
        .route("/v1/pass/ready", post(handlers::ready))
        .route("/v1/vip/ready", post(vip::ready))
        .route("/v1/vip/receipt", post(vip::receipt))
        .route(
            "/v1/pass/checkout/purchase-offer",
            post(pass_purchase::offer),
        )
        .route("/v1/pass/checkout/purchase", post(pass_purchase::create))
        .route("/v1/pass/checkout/offer", post(pass_checkout::offer))
        .route("/v1/pass/checkout/renewal", post(pass_checkout::renewal))
        .route("/v1/pass/checkout/state", post(pass_checkout::status))
        .route("/v1/pass/login", post(pass::login))
        .route("/v1/pass/recovery/request", post(pass_recovery::request))
        .route("/v1/pass/recovery/state", post(pass_recovery::status))
        .route("/v1/pass/recovery/complete", post(pass_recovery::complete))
        .route("/v1/pass/state", post(pass::status))
        .route("/v1/pass/posting-state", post(pass::posting_status))
        .route("/v1/pass/signout", post(pass::signout))
        .route("/v1/pass/receipt", post(pass::write_receipt))
        .route(
            "/v1/pass/restriction-receipt",
            post(pass::restriction_receipt),
        )
        .route("/v1/challenge", post(handlers::challenge))
        .route("/v1/check", post(challenge::check::check))
        .route("/v1/credit", post(challenge::credit::claim))
        .route("/v1/posting/challenge", post(challenge::posting::challenge))
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
