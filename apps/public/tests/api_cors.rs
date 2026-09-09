#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::Body,
    http::{Method, Request, StatusCode},
};
use http_body_util::BodyExt;
use std::time::Duration;
use tokio::sync::mpsc;
use tower::ServiceExt;

const BOARD_ORIGIN: &str = "https://boards.example.test";

async fn offline_api() -> Router {
    offline_api_with(false).await
}

async fn offline_api_with(production: bool) -> Router {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://board_public:unused@127.0.0.1:1/absent")
        .unwrap();
    pool.close().await;
    board_public::routers(pool, BOARD_ORIGIN.into(), production).1
}

async fn request(
    app: &Router,
    method: Method,
    path: &str,
    headers: &[(&str, &str)],
) -> axum::response::Response {
    let mut builder = Request::builder().method(method).uri(path);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    app.clone()
        .oneshot(builder.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn assert_empty(response: axum::response::Response) {
    assert!(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .is_empty()
    );
}

#[tokio::test]
async fn exact_origin_gets_cors_and_denied_or_absent_origins_still_vary() {
    let app = offline_api().await;
    for (origin, allowed) in [
        (Some(BOARD_ORIGIN), true),
        (Some("https://other.example.test"), false),
        (None, false),
    ] {
        let headers = origin
            .map(|value| vec![("origin", value)])
            .unwrap_or_default();
        let response = request(&app, Method::GET, "/boards.json", &headers).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(response.headers()["vary"], "Origin");
        assert_eq!(
            response.headers()["access-control-expose-headers"],
            "ETag, Last-Modified"
        );
        assert_eq!(
            response
                .headers()
                .get("access-control-allow-origin")
                .is_some(),
            allowed
        );
        assert!(
            response
                .headers()
                .get("access-control-allow-credentials")
                .is_none()
        );
    }
}

#[tokio::test]
async fn preflight_allows_only_read_methods_and_conditional_request_headers() {
    let app = offline_api().await;
    let allowed = request(
        &app,
        Method::OPTIONS,
        "/boards.json",
        &[
            ("origin", BOARD_ORIGIN),
            ("access-control-request-method", "GET"),
            (
                "access-control-request-headers",
                "If-None-Match, IF-MODIFIED-SINCE",
            ),
        ],
    )
    .await;
    assert_eq!(allowed.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        allowed.headers()["access-control-allow-origin"],
        BOARD_ORIGIN
    );
    assert_eq!(
        allowed.headers()["access-control-allow-methods"],
        "GET, HEAD, OPTIONS"
    );
    assert_eq!(
        allowed.headers()["access-control-allow-headers"],
        "If-None-Match, If-Modified-Since"
    );
    assert_eq!(allowed.headers()["allow"], "GET, HEAD, OPTIONS");
    assert_eq!(allowed.headers()["cache-control"], "no-store");
    assert_empty(allowed).await;

    let options = request(
        &app,
        Method::OPTIONS,
        "/boards.json",
        &[("origin", BOARD_ORIGIN)],
    )
    .await;
    assert_eq!(options.status(), StatusCode::NO_CONTENT);
    assert_eq!(options.headers()["allow"], "GET, HEAD, OPTIONS");
    assert_eq!(
        options.headers()["access-control-allow-origin"],
        BOARD_ORIGIN
    );
    assert_empty(options).await;

    for headers in [
        vec![
            ("origin", BOARD_ORIGIN),
            ("access-control-request-method", "POST"),
        ],
        vec![
            ("origin", BOARD_ORIGIN),
            ("access-control-request-method", "GET"),
            ("access-control-request-headers", "Authorization"),
        ],
        vec![
            ("origin", "https://other.example.test"),
            ("access-control-request-method", "GET"),
        ],
        vec![
            ("origin", BOARD_ORIGIN),
            ("access-control-request-method", "get"),
        ],
    ] {
        let response = request(&app, Method::OPTIONS, "/boards.json", &headers).await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response.headers()["vary"], "Origin");
        assert!(
            response
                .headers()
                .get("access-control-allow-origin")
                .is_none()
        );
        assert_empty(response).await;
    }
}

#[tokio::test]
async fn api_protection_errors_are_safe_json_and_keep_security_headers() {
    let app = offline_api_with(true).await;
    let response = request(&app, Method::POST, "/demo/post", &[]).await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    assert_eq!(response.headers()["content-type"], "application/json");
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert_eq!(response.headers()["x-frame-options"], "DENY");
    assert_eq!(response.headers()["referrer-policy"], "same-origin");
    assert_eq!(
        response.headers()["permissions-policy"],
        "camera=(), microphone=(), geolocation=()"
    );
    assert_eq!(
        response.headers()["strict-transport-security"],
        "max-age=31536000"
    );
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).unwrap()["error"],
        "Request is forbidden."
    );

    let preflight = request(
        &app,
        Method::OPTIONS,
        "/boards.json",
        &[
            ("origin", BOARD_ORIGIN),
            ("access-control-request-method", "GET"),
        ],
    )
    .await;
    assert_eq!(preflight.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        preflight.headers()["strict-transport-security"],
        "max-age=31536000"
    );
    assert_eq!(preflight.headers()["x-content-type-options"], "nosniff");
    assert_empty(preflight).await;
}

#[tokio::test]
async fn public_and_api_share_admission_and_options_cannot_bypass_busy_protection() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (accepted, mut connections) = mpsc::channel(32);
    let acceptor = tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((stream, _)) = listener.accept().await {
            held.push(stream);
            if accepted.send(()).await.is_err() {
                break;
            }
        }
    });
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(32)
        .connect_lazy(&format!("postgres://board_public:unused@{address}/absent"))
        .unwrap();
    let (public, api) = board_public::routers(pool.clone(), BOARD_ORIGIN.into(), false);
    let mut requests = Vec::new();
    for _ in 0..31 {
        let app = public.clone();
        requests.push(tokio::spawn(async move {
            app.oneshot(
                Request::builder()
                    .uri("/boards.json")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
        }));
    }
    let app = api.clone();
    requests.push(tokio::spawn(async move {
        app.oneshot(
            Request::builder()
                .uri("/boards.json")
                .header("origin", BOARD_ORIGIN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
    }));
    for _ in 0..32 {
        tokio::time::timeout(Duration::from_secs(2), connections.recv())
            .await
            .expect("request reached the controlled PostgreSQL listener")
            .expect("acceptor remains available");
    }

    let busy_get = request(
        &api,
        Method::GET,
        "/boards.json",
        &[("origin", BOARD_ORIGIN)],
    )
    .await;
    assert_eq!(busy_get.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(busy_get.headers()["content-type"], "application/json");
    assert_eq!(
        busy_get.headers()["access-control-allow-origin"],
        BOARD_ORIGIN
    );

    let busy_options = request(
        &api,
        Method::OPTIONS,
        "/boards.json",
        &[
            ("origin", BOARD_ORIGIN),
            ("access-control-request-method", "GET"),
        ],
    )
    .await;
    assert_eq!(busy_options.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(busy_options.headers()["x-content-type-options"], "nosniff");
    assert_eq!(
        busy_options.headers()["access-control-allow-origin"],
        BOARD_ORIGIN
    );
    assert_empty(busy_options).await;

    for request in requests {
        request.abort();
    }
    acceptor.abort();
    pool.close().await;
}

#[tokio::test]
async fn preflight_never_grants_unimplemented_or_ambiguous_requests() {
    let app = offline_api().await;
    for path in ["/demo/post", "/staff/admin/accounts", "/demo/archive.json"] {
        let response = request(
            &app,
            Method::OPTIONS,
            path,
            &[
                ("origin", BOARD_ORIGIN),
                ("access-control-request-method", "GET"),
            ],
        )
        .await;
        assert_ne!(response.status(), StatusCode::NO_CONTENT, "{path}");
        assert!(
            response
                .headers()
                .get("access-control-allow-origin")
                .is_none()
        );
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert_eq!(response.headers()["cache-control"], "no-store");
    }

    let duplicate_method = app
        .clone()
        .oneshot(
            Request::builder()
                .method(Method::OPTIONS)
                .uri("/boards.json")
                .header("origin", BOARD_ORIGIN)
                .header("access-control-request-method", "GET")
                .header("access-control-request-method", "HEAD")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(duplicate_method.status(), StatusCode::BAD_REQUEST);
    assert!(
        duplicate_method
            .headers()
            .get("access-control-allow-origin")
            .is_none()
    );

    let duplicate_origin = app
        .oneshot(
            Request::builder()
                .uri("/boards.json")
                .header("origin", BOARD_ORIGIN)
                .header("origin", "https://other.example.test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(duplicate_origin.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        duplicate_origin
            .headers()
            .get("access-control-allow-origin")
            .is_none()
    );
}

#[tokio::test]
async fn api_listener_has_no_html_or_write_routes_and_head_errors_have_no_body() {
    let app = offline_api().await;
    for path in [
        "/",
        "/demo/",
        "/demo/post/1",
        "/staff/admin/accounts",
        "/demo/archive.json",
    ] {
        let response = request(&app, Method::GET, path, &[]).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
    let write = request(
        &app,
        Method::POST,
        "/demo/post",
        &[("origin", BOARD_ORIGIN)],
    )
    .await;
    assert_eq!(write.status(), StatusCode::METHOD_NOT_ALLOWED);
    let head = request(
        &app,
        Method::HEAD,
        "/boards.json",
        &[("origin", BOARD_ORIGIN)],
    )
    .await;
    assert_eq!(head.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(head.headers()["access-control-allow-origin"], BOARD_ORIGIN);
    assert_empty(head).await;
}

#[tokio::test]
async fn database_backed_api_exposes_validators_on_success_and_not_modified() {
    let url =
        std::env::var("TEST_PUBLIC_DATABASE_URL").expect("TEST_PUBLIC_DATABASE_URL is required");
    let pool = board_store::connect_public(&url).await.unwrap();
    let app = board_public::routers(pool.clone(), BOARD_ORIGIN.into(), false).1;
    let initial = request(
        &app,
        Method::GET,
        "/boards.json",
        &[("origin", BOARD_ORIGIN)],
    )
    .await;
    assert_eq!(initial.status(), StatusCode::OK);
    assert_eq!(initial.headers()["content-type"], "application/json");
    assert_eq!(
        initial.headers()["access-control-allow-origin"],
        BOARD_ORIGIN
    );
    assert_eq!(
        initial.headers()["access-control-expose-headers"],
        "ETag, Last-Modified"
    );
    assert_eq!(initial.headers()["vary"], "Origin");
    let etag = initial.headers()["etag"].to_str().unwrap().to_owned();
    let conditional = request(
        &app,
        Method::GET,
        "/boards.json",
        &[("origin", BOARD_ORIGIN), ("if-none-match", &etag)],
    )
    .await;
    assert_eq!(conditional.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(conditional.headers()["etag"], etag);
    assert_eq!(
        conditional.headers()["access-control-allow-origin"],
        BOARD_ORIGIN
    );
    assert_empty(conditional).await;
    for path in [
        "/demo/thread/1000001.json",
        "/demo/threads.json",
        "/demo/catalog.json",
        "/demo/1.json",
    ] {
        let response = request(&app, Method::GET, path, &[("origin", BOARD_ORIGIN)]).await;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(response.headers()["content-type"], "application/json");
    }
    pool.close().await;
}
