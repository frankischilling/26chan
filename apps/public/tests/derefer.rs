use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use tower::ServiceExt;

fn app() -> Router {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused:synthetic@127.0.0.1:9/unavailable")
        .unwrap();
    board_public::router(pool, "https://board.example".into(), false)
}

fn path(destination: &str) -> String {
    format!(
        "/derefer?{}",
        url::form_urlencoded::Serializer::new(String::new())
            .append_pair("url", destination)
            .finish()
    )
}

#[tokio::test]
async fn actual_router_serves_escaped_two_second_page_without_database_or_redirect_header() {
    let response = app()
        .oneshot(
            Request::builder()
                .uri(path("https://Example.test/path?a=1&amp;b=2&quot;<script>"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(response.headers().get("location").is_none());
    assert!(response.headers().get("set-cookie").is_none());
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(response.headers()["referrer-policy"], "same-origin");
    let csp = response.headers()["content-security-policy"]
        .to_str()
        .unwrap();
    assert!(csp.contains("script-src 'none'") && !csp.contains("unsafe-inline"));
    assert!(
        !csp.contains("/static/derefer.css"),
        "stylesheet is not an image source"
    );
    let html = String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(html.contains("http-equiv=\"refresh\" content=\"2; URL=https://Example.test/path?a=1&#38;b=2&#34;&#60;script&#62;\""), "{html}");
    assert!(html.contains("Redirecting you to <i>example.test</i>..."));
    assert!(!html.contains("<script>"));
    let css = app()
        .oneshot(
            Request::builder()
                .uri("/static/derefer.css")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(css.status(), StatusCode::OK);
    assert_eq!(css.headers()["content-type"], "text/css; charset=utf-8");
}

#[tokio::test]
async fn destination_and_referrer_validation_cannot_be_bypassed_by_decoding_or_host_prefixes() {
    for destination in [
        "javascript:alert(1)",
        "data:text/html,test",
        "//example.test/",
        "https:example.test",
        "https://user:pass@example.test/",
        "https://example.test/\n",
        "https://example.test/\\next",
        "https://&#39;user@example.test/",
        "file:///test",
    ] {
        let response = app()
            .oneshot(
                Request::builder()
                    .uri(path(destination))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{destination}");
    }
    for (referer, expected) in [
        ("https://board.example/thread/1", StatusCode::OK),
        ("", StatusCode::OK),
        ("https://board.example.evil.test/", StatusCode::FORBIDDEN),
        ("https://board.example:444/", StatusCode::FORBIDDEN),
        ("http://board.example/", StatusCode::FORBIDDEN),
        ("https://user@board.example/", StatusCode::FORBIDDEN),
        ("null", StatusCode::FORBIDDEN),
    ] {
        let response = app()
            .oneshot(
                Request::builder()
                    .uri(path("https://example.test/path"))
                    .header("referer", referer)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), expected, "{referer}");
    }
    for empty in ["/derefer", "/derefer?url=", "/derefer?other=1"] {
        let response = app()
            .oneshot(Request::builder().uri(empty).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
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
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/derefer?url=https://one.test&url=https://two.test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    for invalid in [
        "/derefer?url=https%3A%2F%2Fexample.test%2F%",
        "/derefer?url=https%3A%2F%2Fexample.test%2F%FF",
        "/derefer?url=https%3A%2F%2Fexample.test%2F%C3%28",
    ] {
        let response = app()
            .oneshot(Request::builder().uri(invalid).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST, "{invalid}");
    }
    // The HTTP URI type rejects this before it can reach the router. Direct
    // handler tests separately verify its independent decoded/query caps.
    assert!(
        Request::builder()
            .uri(path(&format!(
                "https://example.test/{}",
                "x".repeat(192000)
            )))
            .body(Body::empty())
            .is_err()
    );
}
