use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use tower::ServiceExt;

#[tokio::test]
async fn work_safe_defaults_and_fixed_background_assets_are_verified() {
    use sha2::{Digest, Sha256};
    let app = app(false);
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/settings/theme?worksafe=true")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        body(response)
            .await
            .contains("value=\"yotsuba-b\" selected")
    );
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/settings/theme?worksafe=true")
                .header("cookie", "board-theme=photon; board-theme-ws=tomorrow")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(body(response).await.contains("value=\"tomorrow\" selected"));
    for (name, hash) in [
        (
            "fade.png",
            "5f7a2be79027d3a5c7207de3e7efe510bcc4a66f105e174d1000cbffd6e4a274",
        ),
        (
            "fade-blue.png",
            "1c64b2cff8257de0f2939755da675632e2946432ad96244d03c0df4cfa8e57e0",
        ),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/static/themes/{name}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "image/png");
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert_eq!(
            response.headers()["cache-control"],
            "public, max-age=0, must-revalidate"
        );
        assert!(response.headers().get("set-cookie").is_none());
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(format!("{:x}", Sha256::digest(&bytes)), hash);
        let head = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("HEAD")
                    .uri(format!("/static/themes/{name}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(head.status(), StatusCode::OK);
        assert!(body(head).await.is_empty());
    }
}

fn app(production: bool) -> Router {
    // The actual public router can serve preferences without contacting a DB.
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused:synthetic@127.0.0.1:9/unavailable")
        .unwrap();
    board_public::router(pool, "https://board.example".into(), production)
}

#[tokio::test]
async fn work_safe_and_other_board_preferences_remain_independent() {
    for production in [false, true] {
        let app = app(production);
        let prefix = if production { "__Host-" } else { "" };
        let mut cookies = Vec::new();
        for (worksafe, theme, suffix) in [(false, "photon", ""), (true, "tomorrow", "-ws")] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri("/settings/theme")
                        .header("origin", "https://board.example")
                        .header("content-type", "application/x-www-form-urlencoded")
                        .body(Body::from(format!(
                            "theme={theme}&worksafe={worksafe}&return_to=%2F"
                        )))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::SEE_OTHER);
            let cookie = response.headers()["set-cookie"].to_str().unwrap();
            assert!(cookie.starts_with(&format!("{prefix}board-theme{suffix}={theme};")));
            assert_eq!(cookie.ends_with("; Secure"), production);
            assert!(!cookie.contains("Domain="));
            cookies.push(cookie.split(';').next().unwrap().to_owned());
        }
        let cookies = cookies.join("; ");
        for (worksafe, theme) in [(false, "photon"), (true, "tomorrow")] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/settings/theme?worksafe={worksafe}"))
                        .header("cookie", &cookies)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let page = body(response).await;
            assert!(page.contains(&format!("value=\"{theme}\" selected")));
            assert!(page.contains(&format!("name=\"worksafe\" value=\"{worksafe}\"")));
        }
        // Duplicating one group does not poison the other group's preference.
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/settings/theme?worksafe=true")
                    .header("cookie", format!("{cookies}; {prefix}board-theme=burichan"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(body(response).await.contains("value=\"tomorrow\" selected"));
    }
}

async fn body(response: axum::response::Response) -> String {
    String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap()
}

fn request(theme: &str, origin: Option<&str>, fetch: Option<&str>) -> Request<Body> {
    let mut request = Request::builder()
        .method("POST")
        .uri("/settings/theme")
        .header("content-type", "application/x-www-form-urlencoded");
    if let Some(origin) = origin {
        request = request.header("origin", origin);
    }
    if let Some(fetch) = fetch {
        request = request.header("sec-fetch-site", fetch);
    }
    request
        .body(Body::from(format!(
            "theme={theme}&return_to=%2Ftest%2Fthread%2F123"
        )))
        .unwrap()
}

#[tokio::test]
async fn persisted_styles_are_private_finite_and_have_no_database_dependency() {
    for production in [false, true] {
        let app = app(production);
        for theme in [
            "yotsuba",
            "yotsuba-b",
            "futaba",
            "burichan",
            "photon",
            "tomorrow",
        ] {
            let response = app
                .clone()
                .oneshot(request(
                    theme,
                    Some("https://board.example"),
                    Some("same-origin"),
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::SEE_OTHER);
            assert_eq!(response.headers()["location"], "/test/thread/123");
            assert_eq!(response.headers()["cache-control"], "private, no-store");
            let cookie = response.headers()["set-cookie"]
                .to_str()
                .unwrap()
                .to_owned();
            assert!(cookie.contains("; Path=/; Max-Age=31536000; HttpOnly; SameSite=Lax"));
            assert!(!cookie.contains("Domain="));
            assert_eq!(cookie.ends_with("; Secure"), production);
            assert_eq!(cookie.starts_with("__Host-"), production);
            let value = cookie.split(';').next().unwrap();
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/settings/theme")
                        .header("cookie", value)
                        .header(
                            "referer",
                            "https://board.example/test/catalog?ignored=secret",
                        )
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert!(
                response.headers()["content-security-policy"]
                    .to_str()
                    .unwrap()
                    .contains("script-src 'none'")
            );
            assert!(response.headers().get("set-cookie").is_none());
            let page = body(response).await;
            assert!(page.contains(&format!("value=\"{theme}\" selected")));
            assert!(page.contains("name=\"return_to\" value=\"/test/catalog\""));
            assert!(!page.contains("ignored=secret"));
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .uri("/static/theme.css")
                        .header("cookie", value)
                        .header("if-none-match", "*")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response.headers()["content-type"],
                "text/css; charset=utf-8"
            );
            assert_eq!(response.headers()["cache-control"], "private, no-store");
            assert_eq!(response.headers()["vary"], "Cookie");
            assert!(response.headers().get("etag").is_none());
            let css = body(response).await;
            assert!(css.contains("url('/static/themes/fade.png')"));
            assert!(!css.contains("https://"));
            if theme == "tomorrow" {
                assert!(css.contains("--scheme: dark"));
            }
            if theme == "futaba" || theme == "burichan" {
                assert!(css.contains("Times New Roman"));
            }
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("HEAD")
                        .uri("/static/theme.css")
                        .header("cookie", value)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert!(body(response).await.is_empty());
        }
    }
}

#[tokio::test]
async fn style_updates_reject_cross_origin_unknown_fields_and_oversized_bodies() {
    let app = app(true);
    for (origin, fetch) in [
        (None, None),
        (Some("null"), None),
        (Some("https://other.example"), None),
        (Some("https://board.example"), Some("cross-site")),
        (Some("https://board.example"), Some("same-site")),
    ] {
        let response = app
            .clone()
            .oneshot(request("tomorrow", origin, fetch))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert!(response.headers().get("set-cookie").is_none());
    }
    for input in [
        "missing",
        "%3Cscript%3E",
        "tomorrow%0D%0ASet-Cookie%3Aevil%3D1",
        "tomorrow&extra=1",
        "tomorrow&theme=photon",
    ] {
        let response = app
            .clone()
            .oneshot(request(
                input,
                Some("https://board.example"),
                Some("same-origin"),
            ))
            .await
            .unwrap();
        assert!(response.status().is_client_error());
        assert!(response.headers().get("set-cookie").is_none());
    }
    let response = app
        .clone()
        .oneshot(request(
            &"x".repeat(1025),
            Some("https://board.example"),
            Some("same-origin"),
        ))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert!(response.headers().get("set-cookie").is_none());
    let response = app
        .oneshot(request(
            "tomorrow",
            Some("https://board.example"),
            Some("same-origin"),
        ))
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::SEE_OTHER,
        "healthy allowed-origin control"
    );
}

#[tokio::test]
async fn references_cannot_redirect_or_import_themes_from_other_origins() {
    let app = app(false);
    for referer in [
        "https://other.example/test/",
        "https://board.example@other.example/test/",
        "https://board.example/test/upload/status",
        "https://board.example/test/%2fother",
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/settings/theme")
                    .header("referer", referer)
                    .header("cookie", "board-theme=tomorrow; board-theme=photon")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let page = body(response).await;
        assert!(page.contains("name=\"return_to\" value=\"/\""));
        assert!(page.contains("value=\"yotsuba\" selected"));
    }
    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/settings/theme")
                .header("origin", "https://board.example")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from("theme=photon&return_to=%2F%2Fevil.example%2F"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.headers()["location"], "/");
}
