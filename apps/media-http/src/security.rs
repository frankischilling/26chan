use crate::AppState;
use axum::{
    body::{Body, HttpBody},
    extract::{Request, State},
    http::{HeaderValue, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::time::Duration;

pub fn error(status: StatusCode) -> Response {
    let message = match status {
        StatusCode::NOT_FOUND => "Media not found.",
        StatusCode::METHOD_NOT_ALLOWED => "Method not allowed.",
        StatusCode::BAD_REQUEST => "Invalid media request.",
        StatusCode::MISDIRECTED_REQUEST => "Incorrect media host.",
        _ => "Media unavailable.",
    };
    (status, message).into_response()
}

pub async fn protect(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let head = request.method() == Method::HEAD;
    let response = admit(&state, request, next).await;
    let mut response = headers(response);
    // Normalize errors and fallback responses too; GET route HEAD stripping
    // occurs inside the router and cannot cover outer middleware denials.
    if head {
        *response.body_mut() = Body::empty();
    }
    response
}

async fn admit(state: &AppState, request: Request, next: Next) -> Response {
    let mut hosts = request.headers().get_all("host").iter();
    if !hosts
        .next()
        .and_then(|h| h.to_str().ok())
        .is_some_and(|host| host.eq_ignore_ascii_case(&state.authority))
        || hosts.next().is_some()
        || request
            .uri()
            .authority()
            .is_some_and(|a| !a.as_str().eq_ignore_ascii_case(&state.authority))
    {
        return error(StatusCode::MISDIRECTED_REQUEST);
    }
    if request.uri().query().is_some()
        || !request.body().is_end_stream()
        || request.headers().contains_key("transfer-encoding")
        || request
            .headers()
            .get_all("content-length")
            .iter()
            .any(|h| h != "0")
        || request
            .headers()
            .get_all("if-none-match")
            .iter()
            .map(|h| h.as_bytes().len())
            .sum::<usize>()
            > 4096
    {
        return error(StatusCode::BAD_REQUEST);
    }
    let Ok(permit) = state.requests.clone().try_acquire_owned() else {
        return error(StatusCode::SERVICE_UNAVAILABLE);
    };
    let response = match tokio::time::timeout(Duration::from_secs(10), next.run(request)).await {
        Ok(response) => response,
        Err(_) => error(StatusCode::SERVICE_UNAVAILABLE),
    };
    board_http::hold_permit(response, permit)
}

fn headers(mut response: Response) -> Response {
    let h = response.headers_mut();
    h.insert("content-security-policy", HeaderValue::from_static("default-src 'none'; sandbox; base-uri 'none'; frame-ancestors 'none'; form-action 'none'"));
    h.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    h.insert("referrer-policy", HeaderValue::from_static("no-referrer"));
    h.insert(
        "cross-origin-resource-policy",
        HeaderValue::from_static("cross-origin"),
    );
    h.insert(
        "permissions-policy",
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
    h.entry("cache-control")
        .or_insert(HeaderValue::from_static("no-store"));
    response
}
