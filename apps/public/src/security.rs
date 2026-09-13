use crate::AppState;
use axum::{
    extract::{ConnectInfo, Request, State},
    http::{HeaderValue, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, SocketAddr},
    sync::Mutex,
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;

pub struct Limits {
    settings: board_config::PublicRequestLimits,
    active: std::sync::Arc<Semaphore>,
    pub hashes: std::sync::Arc<Semaphore>,
    pub uploads: Semaphore,
    peers: Mutex<HashMap<IpAddr, (Instant, u32)>>,
}

impl Limits {
    pub fn new(settings: board_config::PublicRequestLimits) -> Self {
        Self {
            settings,
            active: std::sync::Arc::new(Semaphore::new(settings.active_requests())),
            hashes: std::sync::Arc::new(Semaphore::new(settings.hash_operations())),
            uploads: Semaphore::new(settings.uploads()),
            peers: Mutex::new(HashMap::new()),
        }
    }
}

pub async fn protect(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let Ok(permit) = state.limits.active.clone().try_acquire_owned() else {
        return headers(
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "Server is busy. Try again shortly.",
            )
                .into_response(),
            &state,
        );
    };
    let response = protect_inner(&state, request, next).await;
    headers(board_http::hold_permit(response, permit), &state)
}

async fn protect_inner(state: &AppState, request: Request, next: Next) -> Response {
    if !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) {
        if request
            .headers()
            .get("origin")
            .and_then(|h| h.to_str().ok())
            != Some(&state.origin)
        {
            return (StatusCode::FORBIDDEN, "A same-origin request is required.").into_response();
        }
        if request
            .headers()
            .get("sec-fetch-site")
            .is_some_and(|v| v != "same-origin" && v != "none")
        {
            return (StatusCode::FORBIDDEN, "Cross-origin request rejected.").into_response();
        }
        let peer = request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0.ip())
            .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        let Ok(mut peers) = state.limits.peers.lock() else {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        };
        let now = Instant::now();
        peers.retain(|_, (start, _)| now.duration_since(*start) < Duration::from_secs(60));
        if peers.len() >= state.limits.settings.tracked_peers() && !peers.contains_key(&peer) {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        let (_, used) = peers.entry(peer).or_insert((now, 0));
        if *used >= state.limits.settings.writes_per_minute() {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                [("retry-after", "60")],
                "Too many submissions. Wait a minute.",
            )
                .into_response();
        }
        *used += 1;
    }
    match tokio::time::timeout(state.limits.settings.handler_timeout(), next.run(request)).await {
        Ok(response) => response,
        Err(_) => (
            StatusCode::REQUEST_TIMEOUT,
            "Request timed out. Check the thread before retrying a post.",
        )
            .into_response(),
    }
}

fn headers(mut response: Response, state: &AppState) -> Response {
    let headers = response.headers_mut();
    // Only fixed, release-owned UI images may load from the public origin.
    // Do not broaden this to 'self': uploaded content stays on the media origin.
    let mut images = format!(
        "{}/static/themes/fade.png {}/static/themes/fade-blue.png {}",
        state.origin,
        state.origin,
        crate::ui_assets::image_sources(&state.origin)
    );
    if let Some(media) = &state.media {
        images = format!("{} {images}", media.settings.origin.as_string());
    }
    let policy = format!(
        "default-src 'none'; style-src 'self'; img-src {images}; script-src 'none'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'; object-src 'none'"
    );
    headers.insert(
        "content-security-policy",
        HeaderValue::from_str(&policy).expect("validated application origins"),
    );
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    // no-referrer makes browser form navigations send Origin: null. Keep the
    // same-origin signal while withholding referrers from external destinations.
    headers.insert("referrer-policy", HeaderValue::from_static("same-origin"));
    headers.insert(
        "permissions-policy",
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
    headers
        .entry("cache-control")
        .or_insert(HeaderValue::from_static("no-store"));
    if state.production {
        headers.insert(
            "strict-transport-security",
            HeaderValue::from_static("max-age=31536000"),
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, http::Request, middleware, routing::get};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use tower::ServiceExt;

    #[test]
    fn configured_hash_and_upload_semaphores_enforce_and_release_their_budgets() {
        let settings = board_config::PublicRequestLimits::from_lookup(|key| match key {
            "PUBLIC_MAX_HASH_OPERATIONS" => Some("2".into()),
            "PUBLIC_MAX_UPLOADS" => Some("3".into()),
            _ => None,
        })
        .unwrap();
        let limits = Limits::new(settings);
        for (semaphore, count) in [(&*limits.hashes, 2), (&limits.uploads, 3)] {
            let mut permits = Vec::new();
            for _ in 0..count {
                permits.push(semaphore.try_acquire().unwrap());
            }
            assert!(semaphore.try_acquire().is_err());
            drop(permits.pop());
            let recovered = semaphore.try_acquire().unwrap();
            assert!(semaphore.try_acquire().is_err());
            drop(recovered);
            drop(permits);
            assert_eq!(semaphore.available_permits(), count);
        }
    }

    #[tokio::test]
    async fn configured_timeout_drops_the_handler_and_releases_admission() {
        struct Dropped(Arc<AtomicBool>);
        impl Drop for Dropped {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let settings = board_config::PublicRequestLimits::from_lookup(|key| match key {
            "PUBLIC_MAX_ACTIVE_REQUESTS" => Some("1".into()),
            "PUBLIC_HANDLER_TIMEOUT_MS" => Some("20".into()),
            _ => None,
        })
        .unwrap();
        let limits = Arc::new(Limits::new(settings));
        let state = AppState {
            pool: sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://unused:unused@127.0.0.1:1/absent")
                .unwrap(),
            origin: "http://127.0.0.1:3000".into(),
            production: false,
            limits: limits.clone(),
            media: None,
        };
        let dropped = Arc::new(AtomicBool::new(false));
        let witness = dropped.clone();
        let app = Router::new()
            .route(
                "/pending",
                get(move || {
                    let dropped = witness.clone();
                    async move {
                        let _guard = Dropped(dropped);
                        std::future::pending::<()>().await;
                    }
                }),
            )
            .layer(middleware::from_fn_with_state(state, protect));
        let response = tokio::time::timeout(
            Duration::from_secs(2),
            app.oneshot(Request::get("/pending").body(Body::empty()).unwrap()),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
        assert!(dropped.load(Ordering::SeqCst));
        assert_eq!(limits.active.available_permits(), 0);
        drop(response);
        assert_eq!(limits.active.available_permits(), 1);
    }
}
