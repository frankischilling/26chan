use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use tower::ServiceExt;

const ORIGIN: &str = "http://127.0.0.1:3000";

async fn request(
    app: &axum::Router,
    method: &str,
    path: &str,
    body: String,
) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("origin", ORIGIN)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn snapshot_path_is_read_only_strict_and_absent_from_the_api_listener() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused:synthetic@127.0.0.1:9/unavailable")
        .unwrap();
    let (web, api) = board_public::routers(pool, ORIGIN.into(), false);
    for app in [&web, &api] {
        for key in [
            "0-tail.json",
            "01-tail.json",
            "+1-tail.json",
            "-1-tail.json",
            "9223372036854775808-tail.json",
        ] {
            assert_eq!(
                request(app, "GET", &format!("/fixture/thread/{key}"), String::new())
                    .await
                    .status(),
                404
            );
        }
    }
    for method in ["POST", "PUT", "PATCH", "DELETE"] {
        assert_eq!(
            request(
                &web,
                method,
                "/_watch/fixture/thread/1/posts-tail",
                String::new()
            )
            .await
            .status(),
            405
        );
    }
    assert_eq!(
        request(
            &web,
            "GET",
            "/_watch/fixture/thread/01/posts-tail",
            String::new()
        )
        .await
        .status(),
        404
    );
    assert_eq!(
        request(
            &web,
            "GET",
            "/_watch/fixture/thread/1/posts-tail?after=0",
            String::new()
        )
        .await
        .status(),
        400
    );
    for method in ["POST", "PUT", "PATCH", "DELETE"] {
        let response = request(
            &web,
            method,
            "/_watch/fixture/thread/1/posts",
            String::new(),
        )
        .await;
        assert_eq!(response.status(), 405, "{method}");
        assert!(response.headers().get("set-cookie").is_none());
    }
    for key in [
        "0",
        "-1",
        "+1",
        "01",
        "1.json",
        "1.html",
        "9223372036854775808",
    ] {
        let response = request(
            &web,
            "GET",
            &format!("/_watch/fixture/thread/{key}/posts"),
            String::new(),
        )
        .await;
        assert_eq!(response.status(), 404, "{key}");
    }
    let response = request(
        &web,
        "GET",
        "/_watch/fixture/thread/1/posts?after=0",
        String::new(),
    )
    .await;
    assert_eq!(response.status(), 400);
    let response = request(&api, "GET", "/_watch/fixture/thread/1/posts", String::new()).await;
    assert_eq!(response.status(), 404);
    assert!(response.headers().get("set-cookie").is_none());
    assert!(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .len()
            < 4096
    );
}

#[cfg(feature = "database-tests")]
#[tokio::test]
async fn owned_posts_match_ssr_and_follow_reply_and_thread_deletion() {
    let pool = board_store::connect_public(
        &std::env::var("TEST_PUBLIC_DATABASE_URL").expect("disposable database required"),
    )
    .await
    .unwrap();
    let app = board_public::routers_with_options(
        pool.clone(),
        board_public::PublicRouterOptions {
            origin: ORIGIN.into(),
            production: false,
            media: None,
            limits: board_config::PublicRequestLimits::default(),
            proxy_uid: None,
            poster_id_key: Some(std::sync::Arc::new(
                board_domain::poster_id::PosterIdKey::parse(&"42".repeat(32)).unwrap(),
            )),
            tripcode_key: None,
            country_database: None,
        },
    )
    .0
    .layer(axum::middleware::from_fn(
        |mut request: axum::extract::Request, next: axum::middleware::Next| async move {
            request
                .extensions_mut()
                .insert(axum::extract::ConnectInfo(fixture_peer()));
            next.run(request).await
        },
    ));
    let password = "owned-updater-snapshot-password";
    let mut ids: Vec<String> = Vec::new();
    for resto in ["0".to_owned(), String::new()] {
        let parent = if resto.is_empty() {
            ids[0].clone()
        } else {
            resto
        };
        let response = request(&app, "POST", "/fixture/post", format!("resto={parent}&name=Snapshot&sub=Shared%20markup&com=%3Cscript%3Ealert%281%29%3C%2Fscript%3E&password={password}")).await;
        assert_eq!(response.status(), 303);
        let location = response.headers()["location"].to_str().unwrap();
        let id = location
            .rsplit_once("#p")
            .map(|(_, id)| id)
            .unwrap_or_else(|| location.rsplit('/').next().unwrap())
            .to_owned();
        ids.push(id);
    }
    let path = format!("/_watch/fixture/thread/{}/posts", ids[0]);
    for method in ["GET", "HEAD"] {
        let response = request(&app, method, &path, String::new()).await;
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["content-type"], "application/json");
        assert_eq!(
            response.headers()["cache-control"],
            "public, max-age=0, must-revalidate"
        );
        assert!(response.headers().contains_key("etag"));
        assert!(response.headers().contains_key("last-modified"));
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert!(response.headers().get("set-cookie").is_none());
        assert!(
            response
                .headers()
                .get("access-control-allow-origin")
                .is_none()
        );
        let csp = response.headers()["content-security-policy"]
            .to_str()
            .unwrap();
        assert!(csp.contains("script-src 'none'"));
        assert!(csp.contains("connect-src 'none'"));
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        if method == "HEAD" {
            assert!(bytes.is_empty());
            continue;
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["thread"], ids[0]);
        assert_eq!(value["replies"], 1);
        let response = request(
            &app,
            "GET",
            &format!("/fixture/thread/{}", ids[0]),
            String::new(),
        )
        .await;
        assert_eq!(response.status(), 200);
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
        for (post, id) in value["posts"].as_array().unwrap().iter().zip(&ids) {
            assert_eq!(post["no"], *id);
            let fragment = post["html"].as_str().unwrap().trim();
            assert!(
                html.contains(fragment),
                "snapshot must use the same post renderer as SSR"
            );
            assert!(!fragment.contains("<script>"));
            assert!(!fragment.contains(password));
            assert!(!fragment.contains("password_hash"));
        }
    }
    for (index, id) in ids.iter().enumerate().rev() {
        let response = request(
            &app,
            "POST",
            "/fixture/delete",
            format!("no={id}&password={password}"),
        )
        .await;
        assert_eq!(response.status(), 303);
        let response = request(&app, "GET", &path, String::new()).await;
        if index == 0 {
            assert_eq!(response.status(), 404);
        } else {
            assert_eq!(response.status(), 200);
            let value: serde_json::Value =
                serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes())
                    .unwrap();
            assert_eq!(value["replies"], 0);
            assert_eq!(value["posts"].as_array().unwrap().len(), 1);
        }
    }
    pool.close().await;
}

// Use isolated transport identities so the reply and thread deletion checks do
// not consume each other's persisted public deletion quota, including on reruns.
#[cfg(feature = "database-tests")]
fn fixture_peer() -> std::net::SocketAddr {
    static NEXT: std::sync::OnceLock<std::sync::atomic::AtomicU64> = std::sync::OnceLock::new();
    let nonce = NEXT
        .get_or_init(|| {
            std::sync::atomic::AtomicU64::new(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos() as u64,
            )
        })
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::net::SocketAddr::new(
        std::net::IpAddr::V6(std::net::Ipv6Addr::new(
            0x2001,
            0xdb8,
            7,
            0,
            (nonce >> 48) as u16,
            (nonce >> 32) as u16,
            (nonce >> 16) as u16,
            nonce as u16,
        )),
        12345,
    )
}
