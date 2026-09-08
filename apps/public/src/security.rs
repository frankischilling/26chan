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
    active: Semaphore,
    pub hashes: std::sync::Arc<Semaphore>,
    peers: Mutex<HashMap<IpAddr, (Instant, u32)>>,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            active: Semaphore::new(32),
            hashes: std::sync::Arc::new(Semaphore::new(4)),
            peers: Mutex::new(HashMap::new()),
        }
    }
}

pub async fn protect(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let response = protect_inner(&state, request, next).await;
    headers(response, state.production)
}

async fn protect_inner(state: &AppState, request: Request, next: Next) -> Response {
    let Ok(_permit) = state.limits.active.try_acquire() else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            "Server is busy. Try again shortly.",
        )
            .into_response();
    };
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
        if peers.len() >= 10_000 && !peers.contains_key(&peer) {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        let (_, used) = peers.entry(peer).or_insert((now, 0));
        if *used >= 30 {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                [("retry-after", "60")],
                "Too many submissions. Wait a minute.",
            )
                .into_response();
        }
        *used += 1;
    }
    match tokio::time::timeout(Duration::from_secs(10), next.run(request)).await {
        Ok(response) => response,
        Err(_) => (
            StatusCode::REQUEST_TIMEOUT,
            "Request timed out. Check the thread before retrying a post.",
        )
            .into_response(),
    }
}

fn headers(mut response: Response, production: bool) -> Response {
    let headers = response.headers_mut();
    headers.insert("content-security-policy", HeaderValue::from_static("default-src 'none'; style-src 'self'; img-src 'none'; script-src 'none'; form-action 'self'; base-uri 'none'; frame-ancestors 'none'; object-src 'none'"));
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
    if production {
        headers.insert(
            "strict-transport-security",
            HeaderValue::from_static("max-age=31536000"),
        );
    }
    response
}
