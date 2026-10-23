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
    // A browser's standalone media document fetches its own approved URL.
    // Chromium creates that element with crossorigin=anonymous. Retain this
    // isolated media origin so its own read is same-origin, rather than allowing
    // CORS from every opaque/null origin. Scripts, frames and forms stay blocked.
    let video = matches!(
        response.status(),
        StatusCode::OK | StatusCode::PARTIAL_CONTENT | StatusCode::NOT_MODIFIED
    ) && response
        .headers()
        .get("content-type")
        .is_some_and(|value| value == "video/webm" || value == "video/mp4");
    let player_policy = (response.status() == StatusCode::OK)
        .then(|| {
            response
                .extensions()
                .get::<crate::swf_player::DocumentPolicy>()
                .cloned()
        })
        .flatten();
    let h = response.headers_mut();
    let policy = if video {
        "default-src 'none'; media-src 'self'; sandbox allow-same-origin; base-uri 'none'; frame-ancestors 'none'; form-action 'none'"
    } else {
        "default-src 'none'; sandbox; base-uri 'none'; frame-ancestors 'none'; form-action 'none'"
    };
    h.insert("content-security-policy", HeaderValue::from_static(policy));
    h.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    if let Some(player) = player_policy {
        h.insert("content-security-policy", player.0);
        h.remove("x-frame-options");
    } else {
        h.insert("x-frame-options", HeaderValue::from_static("DENY"));
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_successful_video_representations_allow_standalone_media() {
        for status in [
            StatusCode::OK,
            StatusCode::PARTIAL_CONTENT,
            StatusCode::NOT_MODIFIED,
            StatusCode::NOT_FOUND,
            StatusCode::SERVICE_UNAVAILABLE,
        ] {
            for mime in [
                "video/webm",
                "video/mp4",
                "image/png",
                "application/pdf",
                "text/html",
                "video/unknown",
            ] {
                let response = headers((status, [("content-type", mime)], "owned").into_response());
                let csp = response.headers()["content-security-policy"]
                    .to_str()
                    .unwrap();
                assert_eq!(
                    csp.contains("media-src 'self'"),
                    matches!(
                        status,
                        StatusCode::OK | StatusCode::PARTIAL_CONTENT | StatusCode::NOT_MODIFIED
                    ) && matches!(mime, "video/webm" | "video/mp4")
                );
                assert!(csp.contains("default-src 'none';"));
                assert_eq!(
                    csp.contains("sandbox allow-same-origin;"),
                    matches!(
                        status,
                        StatusCode::OK | StatusCode::PARTIAL_CONTENT | StatusCode::NOT_MODIFIED
                    ) && matches!(mime, "video/webm" | "video/mp4")
                );
                assert!(csp.contains("sandbox"));
                assert!(!csp.contains("allow-scripts"));
                assert_eq!(response.headers()["x-content-type-options"], "nosniff");
            }
        }
    }
}
