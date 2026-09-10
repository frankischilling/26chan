#![forbid(unsafe_code)]
pub mod auth;
pub mod config;
mod handlers;
pub mod store;
mod views;
use axum::{
    Router,
    extract::DefaultBodyLimit,
    http::StatusCode,
    middleware,
    response::{IntoResponse, Response},
    routing::{get, post},
};
pub use config::{Config, valid_origin};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::sync::Arc;
use webauthn_rs::prelude::*;

pub struct AppState {
    pub config: Config,
    pub auth: PgPool,
    pub staff: PgPool,
    pub webauthn: Webauthn,
    pub limits: Limits,
}
pub struct Limits {
    pub permits: Arc<tokio::sync::Semaphore>,
    pub attempts: std::sync::Mutex<(std::time::Instant, u32)>,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            permits: Arc::new(tokio::sync::Semaphore::new(16)),
            attempts: std::sync::Mutex::new((std::time::Instant::now(), 0)),
        }
    }
}
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("Authentication required")]
    Unauthorized,
    #[error("Request forbidden")]
    Forbidden,
    #[error("Sign in again before changing moderation state")]
    Recent,
    #[error("Invalid request")]
    Invalid,
    #[error("Object unavailable")]
    NotFound,
    #[error("Service unavailable")]
    Database(#[from] sqlx::Error),
    #[error("Service unavailable")]
    Internal,
    #[error("Request capacity reached; try again later")]
    Capacity,
}
impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let code = match self {
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden | Self::Recent => StatusCode::FORBIDDEN,
            Self::Invalid => StatusCode::BAD_REQUEST,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Capacity => StatusCode::TOO_MANY_REQUESTS,
            _ => StatusCode::SERVICE_UNAVAILABLE,
        };
        (code, self.to_string()).into_response()
    }
}
impl AppState {
    pub async fn connect(config: Config) -> Result<Arc<Self>, AppError> {
        let auth = PgPoolOptions::new()
            .max_connections(8)
            .acquire_timeout(std::time::Duration::from_secs(2))
            .connect(&config.auth_database)
            .await?;
        let staff = PgPoolOptions::new()
            .max_connections(8)
            .acquire_timeout(std::time::Duration::from_secs(2))
            .connect(&config.staff_database)
            .await?;
        auth::check_identity(&auth, "board_auth").await?;
        auth::check_identity(&staff, "board_staff").await?;
        let origin = Url::parse(&config.origin).map_err(|_| AppError::Invalid)?;
        let webauthn = WebauthnBuilder::new(origin.host_str().ok_or(AppError::Invalid)?, &origin)
            .map_err(|_| AppError::Invalid)?
            .rp_name("Board staff")
            .build()
            .map_err(|_| AppError::Invalid)?;
        Ok(Arc::new(Self {
            config,
            auth,
            staff,
            webauthn,
            limits: Limits::default(),
        }))
    }
}
/// Observe the existing authentication/moderation pools without querying them.
pub fn observed_router(state: Arc<AppState>) -> (board_observe::Metrics, Router) {
    use board_observe::{Listener, Metrics, Pool, PoolSample};
    let mut metrics = Metrics::new();
    for (name, pool) in [
        (Pool::StaffAuth, state.auth.clone()),
        (Pool::StaffContent, state.staff.clone()),
    ] {
        metrics
            .register_pool(name, move || PoolSample {
                size: pool.size(),
                idle: pool.num_idle(),
                max: pool.options().get_max_connections(),
            })
            .expect("distinct pools registered before sharing metrics");
    }
    let app = metrics.layer(router(state), Listener::Staff);
    (metrics, app)
}

pub fn router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/", get(handlers::landing))
        .route("/staff.js", get(handlers::javascript))
        .route("/readyz", get(handlers::ready))
        .route("/reports", get(handlers::queue))
        .route("/enroll/start", post(handlers::enroll_start))
        .route("/enroll/finish", post(handlers::enroll_finish))
        .route("/login/start", post(handlers::login_start))
        .route("/login/finish", post(handlers::login_finish))
        .route("/logout", post(handlers::logout))
        .route("/moderate", post(handlers::moderate))
        .layer(DefaultBodyLimit::max(32768))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            handlers::request_limits,
        ))
        .layer(middleware::from_fn(handlers::security_headers))
        .layer(middleware::from_fn(board_http::retain_response_body))
        .with_state(state)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn production_origin_requires_https_and_exact_origin() {
        assert!(!valid_origin("http://staff.example", true));
        assert!(!valid_origin("https://staff.example/path", true));
        assert!(!valid_origin("https://user:pass@staff.example", true));
        assert!(valid_origin("https://staff.example", true));
        assert!(!valid_origin("http://staff.example", false));
        assert!(valid_origin("http://localhost:3001", false));
    }
    #[test]
    fn origin_cookie_and_csrf_fail_closed() {
        use axum::http::{HeaderMap, HeaderValue};
        let mut h = HeaderMap::new();
        assert!(auth::origin(&h, "https://staff.example").is_err());
        h.insert("origin", HeaderValue::from_static("https://staff.example"));
        assert!(auth::origin(&h, "https://staff.example").is_err());
        h.insert("sec-fetch-site", HeaderValue::from_static("same-origin"));
        assert!(auth::origin(&h, "https://staff.example").is_ok());
        h.insert(
            "origin",
            HeaderValue::from_static("https://staff.example.other"),
        );
        assert!(auth::origin(&h, "https://staff.example").is_err());
        assert!(auth::cookie(&h, "staff").is_err());
        let t = auth::token();
        h.insert(
            "cookie",
            HeaderValue::from_str(&format!("staff={t}; staff={t}")).unwrap(),
        );
        assert!(auth::cookie(&h, "staff").is_err());
        let s = auth::Session {
            account_id: 1,
            role: "moderator".into(),
            csrf_hash: auth::hash(&t),
            recent: true,
        };
        assert!(auth::csrf(&s, &t).is_ok());
        assert!(auth::csrf(&s, &auth::token()).is_err());
    }
}
