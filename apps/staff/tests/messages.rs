#![cfg(feature = "database-tests")]

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use board_staff::{AppState, Config, Limits, auth, router};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use tower::ServiceExt;
use webauthn_rs::prelude::*;

#[tokio::test]
async fn messages_require_live_authority_and_preserve_board_visibility_and_unread_state() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let auth_pool = PgPool::connect(&std::env::var("AUTH_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let staff_pool = PgPool::connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let origin = Url::parse("http://localhost:3001").unwrap();
    let state = Arc::new(AppState {
        config: Config {
            asn_database: None,
            browser_hint_key: None,
            media: None,
            proxy: None,
            poster_id_key: None,
            country_database: None,
            origin: "http://localhost:3001".into(),
            public_origin: "http://localhost:3000".into(),
            media_origin: "http://127.0.0.1:3002".into(),
            bind: "127.0.0.1:3001".parse().unwrap(),
            production: false,
            auth_database: String::new(),
            staff_database: String::new(),
            idle_timeout: Duration::from_secs(600),
            tripcode_key: None,
        },
        auth: auth_pool.clone(),
        staff: staff_pool.clone(),
        webauthn: WebauthnBuilder::new("localhost", &origin)
            .unwrap()
            .build()
            .unwrap(),
        limits: Limits::default(),
    });
    auth::check_identity(&auth_pool, "board_auth")
        .await
        .unwrap();
    auth::check_identity(&staff_pool, "board_staff")
        .await
        .unwrap();
    let token = auth::token();
    let csrf = auth::token();
    let credential = uuid::Uuid::new_v4().as_bytes().to_vec();
    let mut tx = owner.begin().await.unwrap();
    let account: i64 = sqlx::query_scalar("INSERT INTO staff_identity.accounts(role,allow_boards) VALUES('janitor',ARRAY['msgone']) RETURNING id")
        .fetch_one(&mut *tx).await.unwrap();
    sqlx::query(
        "INSERT INTO staff_identity.credentials(id,account_id,credential) VALUES($1,$2,'{}')",
    )
    .bind(&credential)
    .bind(account)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id) VALUES($1,$2,$3,$4)")
        .bind(auth::hash(&token)).bind(auth::hash(&csrf)).bind(account).bind(&credential)
        .execute(&mut *tx).await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let mut ids = Vec::new();
    for (index, (boards, content)) in [
        (vec![], "Owned general message"),
        (vec!["msgone"], "Owned allowed <script>content</script>"),
        (vec!["msgtwo"], "Owned hidden message"),
        (vec!["msgtwo", "msgone"], "Owned mixed-board message"),
    ]
    .into_iter()
    .enumerate()
    {
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO restrictions.staff_messages(boards,content,created_at) VALUES($1,$2,$3) RETURNING id",
        )
        .bind(boards)
        .bind(content)
        .bind(1_700_000_000_i64 + index as i64)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        ids.push(id);
    }
    tx.commit().await.unwrap();
    let outcome = tokio::spawn({
        let owner = owner.clone();
        let credential = credential.clone();
        let message_ids = ids.clone();
        async move {
            let app = router(state);
            let cookie = format!("staff={token}; staff-csrf={csrf}");
            let request = |path: &str, cookies: &str| Request::get(path).header("cookie", cookies).body(Body::empty()).unwrap();
            let anonymous = app.clone().oneshot(request("/messages", "")).await.unwrap();
            assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
            let response = app.clone().oneshot(request("/messages", &cookie)).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()["cache-control"], "private, no-store");
            let marker = response.headers()["set-cookie"].to_str().unwrap().to_owned();
            assert!(marker.starts_with("staff-messages="));
            assert!(marker.contains("HttpOnly; SameSite=Strict; Max-Age=31536000"));
            let policy = response.headers()["content-security-policy"].to_str().unwrap();
            assert!(policy.contains("font-src http://localhost:3001/staff-assets/reportqueue.woff2;"));
            assert!(!policy.contains("img-src 'self'"));
            let html = String::from_utf8(to_bytes(response.into_body(), 1_048_576).await.unwrap().to_vec()).unwrap();
            assert!(html.contains("Owned general message"));
            assert!(html.contains("Owned allowed"));
            assert!(html.contains("Owned mixed-board message"));
            assert!(!html.contains("Owned hidden message"));
            assert!(!html.contains("<script>content</script>"));
            assert!(!html.contains(&token));
            assert!(!html.contains(&csrf));
            if std::env::var("RUN_STAFF_MESSAGE_BROWSER").as_deref() == Ok("1") {
                let mut tx = owner.begin().await.unwrap();
                sqlx::query("SET LOCAL ROLE board_restriction_owner").execute(&mut *tx).await.unwrap();
                sqlx::query("UPDATE restrictions.staff_messages SET content='Owned allowed message' WHERE id=$1")
                    .bind(message_ids[1]).execute(&mut *tx).await.unwrap();
                tx.commit().await.unwrap();
                let listener = tokio::net::TcpListener::bind("127.0.0.1:3001").await.unwrap();
                let served = app.clone();
                let server = tokio::spawn(async move { axum::serve(listener, served).await.unwrap(); });
                let mut command = tokio::process::Command::new("node");
                command.current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
                    .arg("tests/browser/staff-messages-source.mjs")
                    .env("STAFF_MESSAGE_ORIGIN", "http://localhost:3001")
                    .env("STAFF_MESSAGE_COOKIE", &cookie)
                    .kill_on_drop(true);
                let result = tokio::time::timeout(Duration::from_secs(120), command.output()).await;
                server.abort();
                let _ = server.await;
                let output = result.expect("source message browser deadline").unwrap();
                println!("{}", String::from_utf8_lossy(&output.stdout));
                assert!(output.status.success(), "Source message browser failed: {}", String::from_utf8_lossy(&output.stderr));
            }
            let count = app.clone().oneshot(request("/messages/count", &format!("{cookie}; staff-messages=9223372036854775807"))).await.unwrap();
            assert_eq!(count.status(), StatusCode::OK);
            assert!(count.headers().get("set-cookie").is_none());
            let value: serde_json::Value = serde_json::from_slice(&to_bytes(count.into_body(), 1024).await.unwrap()).unwrap();
            assert_eq!(value, serde_json::json!({"unread":0}));
            for path in ["/messages?offset=-1", "/messages?offset=10001", "/messages?offset=0&offset=25", "/messages?board=msgtwo"] {
                assert_eq!(app.clone().oneshot(request(path, &cookie)).await.unwrap().status(), StatusCode::BAD_REQUEST);
            }
            sqlx::query("UPDATE staff_identity.accounts SET role='moderator' WHERE id=$1").bind(account).execute(&owner).await.unwrap();
            let response = app.clone().oneshot(request("/messages", &cookie)).await.unwrap();
            let html = String::from_utf8(to_bytes(response.into_body(), 1_048_576).await.unwrap().to_vec()).unwrap();
            assert!(html.contains("Owned hidden message"));
            sqlx::query("UPDATE staff_identity.sessions SET expires_at=clock_timestamp()-interval '1 second' WHERE account_id=$1").bind(account).execute(&owner).await.unwrap();
            assert_eq!(app.clone().oneshot(request("/messages", &cookie)).await.unwrap().status(), StatusCode::UNAUTHORIZED);
            sqlx::query("UPDATE staff_identity.sessions SET expires_at=clock_timestamp()+interval '1 hour' WHERE account_id=$1").bind(account).execute(&owner).await.unwrap();
            sqlx::query("DELETE FROM staff_identity.credentials WHERE id=$1").bind(&credential).execute(&owner).await.unwrap();
            assert_eq!(app.clone().oneshot(request("/messages", &cookie)).await.unwrap().status(), StatusCode::UNAUTHORIZED);
            for asset in ["reportqueue.woff2", "select-arrow.png", "select-arrow-light.png", "favicon-team.ico"] {
                let response = app.clone().oneshot(request(&format!("/staff-assets/{asset}"), "")).await.unwrap();
                assert_eq!(response.status(), StatusCode::OK);
                assert!(!to_bytes(response.into_body(), 100_000).await.unwrap().is_empty());
            }
            assert_eq!(app.oneshot(request("/staff-assets/unknown.png", "")).await.unwrap().status(), StatusCode::NOT_FOUND);
            let denied = sqlx::query("SELECT staff_identity.staff_message_page($1,$2,600,0,0)")
                .bind(auth::hash(&token)).bind(auth::hash(&csrf)).execute(&staff_pool).await.unwrap_err();
            assert_eq!(denied.as_database_error().unwrap().code().as_deref(), Some("42501"));
        }
    }).await;
    let mut cleanup = owner.begin().await.unwrap();
    sqlx::query("DELETE FROM staff_identity.sessions WHERE account_id=$1")
        .bind(account)
        .execute(&mut *cleanup)
        .await
        .unwrap();
    sqlx::query("DELETE FROM staff_identity.credentials WHERE account_id=$1")
        .bind(account)
        .execute(&mut *cleanup)
        .await
        .unwrap();
    sqlx::query("DELETE FROM staff_identity.accounts WHERE id=$1")
        .bind(account)
        .execute(&mut *cleanup)
        .await
        .unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *cleanup)
        .await
        .unwrap();
    sqlx::query("DELETE FROM restrictions.staff_messages WHERE id=ANY($1)")
        .bind(&ids)
        .execute(&mut *cleanup)
        .await
        .unwrap();
    cleanup.commit().await.unwrap();
    outcome.unwrap();
}
