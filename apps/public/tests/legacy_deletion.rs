#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use tower::ServiceExt;

const ORIGIN: &str = "http://127.0.0.1:3000";
const PASSWORD: &str = "legacy-deletion-password";

fn form(
    uri: &str,
    fields: &[(&str, &str)],
    origin: Option<&str>,
    multipart: bool,
) -> Request<Body> {
    let mut request = Request::post(uri);
    if let Some(origin) = origin {
        request = request.header("origin", origin);
    }
    let body = if multipart {
        request = request.header("content-type", "multipart/form-data; boundary=owned");
        let mut body = String::new();
        for (name, value) in fields {
            body.push_str(&format!(
                "--owned\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            ));
        }
        body.push_str("--owned--\r\n");
        body
    } else {
        request = request.header("content-type", "application/x-www-form-urlencoded");
        url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(fields.iter().copied())
            .finish()
    };
    request.body(Body::from(body)).unwrap()
}

async fn thread_status(app: &Router, id: &str) -> StatusCode {
    app.clone()
        .oneshot(
            Request::get(format!("/fixture/thread/{id}.json"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
        .status()
}

#[tokio::test]
async fn legacy_post_and_delete_share_password_origin_and_board_authorization() {
    let database =
        std::env::var("TEST_PUBLIC_DATABASE_URL").expect("TEST_PUBLIC_DATABASE_URL is required");
    let pool = board_store::connect_public(&database).await.unwrap();
    let admin = sqlx::PgPool::connect(
        &std::env::var("MIGRATION_DATABASE_URL").expect("MIGRATION_DATABASE_URL is required"),
    )
    .await
    .unwrap();
    let app = fixture_routers(
        pool.clone(),
        ORIGIN.into(),
        false,
        None,
        board_config::PublicRequestLimits::default(),
    )
    .0;
    for multipart in [false, true] {
        let response = app
            .clone()
            .oneshot(form(
                "/fixture/imgboard.php",
                &[
                    ("mode", "regist"),
                    ("sub", "Owned legacy deletion"),
                    ("com", "A synthetic post for the legacy deletion route."),
                    ("pwd", PASSWORD),
                ],
                Some(ORIGIN),
                multipart,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response.headers()["location"].to_str().unwrap();
        let id = location
            .rsplit('/')
            .next()
            .unwrap()
            .split('#')
            .next()
            .unwrap()
            .to_owned();
        assert_eq!(thread_status(&app, &id).await, StatusCode::OK);
        let popup = app
            .clone()
            .oneshot(
                Request::get(format!("/fixture/imgboard.php?mode=report&no={id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(popup.status(), StatusCode::OK);
        assert_eq!(popup.headers()["x-frame-options"], "DENY");
        let popup = popup.into_body().collect().await.unwrap().to_bytes();
        let popup = std::str::from_utf8(&popup).unwrap();
        assert!(popup.contains("action=\"/fixture/report\""));
        assert!(popup.contains(&format!("name=\"no\" value=\"{id}\"")));
        assert!(popup.contains("name=\"reason\""));
        let report = app
            .clone()
            .oneshot(form(
                "/fixture/report",
                &[
                    ("no", id.as_str()),
                    ("reason", "Owned legacy report <literal>"),
                ],
                Some(ORIGIN),
                false,
            ))
            .await
            .unwrap();
        assert_eq!(report.status(), StatusCode::OK);
        let reason: String = sqlx::query_scalar(
            "SELECT reason FROM content.reports WHERE board='fixture' AND post_id=$1",
        )
        .bind(id.parse::<i64>().unwrap())
        .fetch_one(&admin)
        .await
        .unwrap();
        assert_eq!(reason, "Owned legacy report <literal>");
        let wrong_popup = app
            .clone()
            .oneshot(
                Request::get(format!("/demo/imgboard.php?mode=report&no={id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(wrong_popup.status(), StatusCode::NOT_FOUND);
        let fields = [
            ("mode", "usrdel"),
            (id.as_str(), "delete"),
            ("pwd", PASSWORD),
        ];
        for origin in [None, Some("null"), Some("https://untrusted.example")] {
            let response = app
                .clone()
                .oneshot(form("/fixture/imgboard.php", &fields, origin, multipart))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
        }
        let original_state = "SELECT jsonb_build_object('post',to_jsonb(p),'thread',to_jsonb(t)) FROM content.posts p JOIN content.threads t ON t.id=p.thread_id AND t.board=p.board WHERE p.board='fixture' AND p.id=$1";
        let before: serde_json::Value = sqlx::query_scalar(original_state)
            .bind(id.parse::<i64>().unwrap())
            .fetch_one(&admin)
            .await
            .unwrap();
        let response = app
            .clone()
            .oneshot(form("/demo/imgboard.php", &fields, Some(ORIGIN), multipart))
            .await
            .unwrap();
        // The target is missing on this accessible board: source single-delete
        // returns Updating index, without granting authority over another board.
        assert_eq!(response.status(), StatusCode::OK);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("Updating index"));
        assert!(html.contains("href=\"/demo/\""));
        assert_eq!(thread_status(&app, &id).await, StatusCode::OK);
        let after: serde_json::Value = sqlx::query_scalar(original_state)
            .bind(id.parse::<i64>().unwrap())
            .fetch_one(&admin)
            .await
            .unwrap();
        assert_eq!(
            before, after,
            "Wrong-board deletion must leave the original post and thread unchanged"
        );
        let mut wrong = fields;
        wrong[2].1 = "wrong-password";
        let response = app
            .clone()
            .oneshot(form(
                "/fixture/imgboard.php",
                &wrong,
                Some(ORIGIN),
                multipart,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(thread_status(&app, &id).await, StatusCode::OK);

        let response = app
            .clone()
            .oneshot(form(
                "/fixture/imgboard.php?mode=usrdel",
                &fields[1..],
                Some(ORIGIN),
                multipart,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response.headers()["cache-control"]
                .to_str()
                .unwrap()
                .contains("no-store")
        );
        let body = response.into_body().collect().await.unwrap().to_bytes();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("Updating index"));
        assert!(html.contains("href=\"/fixture/\""));
        assert!(!html.contains(PASSWORD));
        assert_eq!(thread_status(&app, &id).await, StatusCode::NOT_FOUND);
        let popup = app
            .clone()
            .oneshot(
                Request::get(format!("/fixture/imgboard.php?mode=report&no={id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(popup.status(), StatusCode::NOT_FOUND);
        sqlx::query("DELETE FROM content.reports WHERE board='fixture' AND post_id=$1")
            .bind(id.parse::<i64>().unwrap())
            .execute(&admin)
            .await
            .unwrap();
    }
    pool.close().await;
    admin.close().await;
}

// Fixture transport identities are isolated so unrelated authorization cases do
// not consume each other's shared public deletion quota.
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
            2,
            0,
            (nonce >> 48) as u16,
            (nonce >> 32) as u16,
            (nonce >> 16) as u16,
            nonce as u16,
        )),
        12345,
    )
}

fn fixture_routers(
    pool: sqlx::PgPool,
    origin: String,
    production: bool,
    media: Option<board_config::PublicMediaSettings>,
    limits: board_config::PublicRequestLimits,
) -> (axum::Router, axum::Router) {
    let (web, api) = board_public::routers_with_options(
        pool,
        board_public::PublicRouterOptions {
            origin,
            production,
            media,
            limits,
            proxy_uid: None,
            poster_id_key: Some(std::sync::Arc::new(
                board_domain::poster_id::PosterIdKey::parse(&"42".repeat(32)).unwrap(),
            )),
            tripcode_key: None,
            country_database: None,
        },
    );

    let transport = axum::middleware::from_fn(
        move |mut request: axum::extract::Request, next: axum::middleware::Next| async move {
            if request
                .extensions()
                .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
                .is_none()
            {
                request
                    .extensions_mut()
                    .insert(axum::extract::ConnectInfo(fixture_peer()));
            }
            next.run(request).await
        },
    );
    (web.layer(transport.clone()), api.layer(transport))
}
