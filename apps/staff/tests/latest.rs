#![cfg(feature = "database-tests")]

use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use board_staff::{AppState, Config, Limits, auth, router};
use serde_json::Value;
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use tower::ServiceExt;
use webauthn_rs::prelude::*;

async fn pool(key: &str) -> PgPool {
    PgPool::connect(&std::env::var(key).unwrap()).await.unwrap()
}

async fn private_comment_context(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>, comment: &str) {
    let mut prepared = board_domain::wordfiltered_comment::prepare(
        comment,
        board_domain::comment_markup::MarkupPolicy::default(),
        board_domain::wordfilter::Profile::Global,
        None,
    )
    .unwrap();
    prepared.freeze_format("j");
    let payload = prepared
        .encode()
        .unwrap()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let search = board_domain::formatting::plain_text(&board_domain::filtered_formatting::lines(
        &prepared, "j",
    ));
    sqlx::query("SELECT set_config('board.wordfilter_payload',$1,true),set_config('board.wordfilter_search',$2,true)")
        .bind(payload).bind(search).execute(&mut **tx).await.unwrap();
}

#[tokio::test]
async fn latest_reports_private_post_numbers_only_to_live_staff_sessions() {
    let owner = pool("MIGRATION_DATABASE_URL").await;
    let auth_pool = pool("AUTH_DATABASE_URL").await;
    let staff_pool = pool("STAFF_DATABASE_URL").await;
    let origin = Url::parse("http://localhost:3001").unwrap();
    let state = Arc::new(AppState {
        config: Config {
            origin: "http://localhost:3001".into(),
            public_origin: "http://localhost:3000".into(),
            media_origin: "http://127.0.0.1:3002".into(),
            bind: "127.0.0.1:3001".parse().unwrap(),
            production: false,
            auth_database: String::new(),
            staff_database: String::new(),
            idle_timeout: Duration::from_secs(60),
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
    let mut tx = owner.begin().await.unwrap();
    let account: i64 = sqlx::query_scalar(
        "INSERT INTO staff_identity.accounts(role) VALUES('moderator') RETURNING id",
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    let credential = uuid::Uuid::new_v4().as_bytes().to_vec();
    sqlx::query(
        "INSERT INTO staff_identity.credentials(id,account_id,credential) VALUES($1,$2,'{}')",
    )
    .bind(&credential)
    .bind(account)
    .execute(&mut *tx)
    .await
    .unwrap();
    let token = auth::token();
    sqlx::query("INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id) VALUES($1,$2,$3,$4)")
        .bind(auth::hash(&token)).bind(auth::hash(&auth::token())).bind(account).bind(&credential)
        .execute(&mut *tx).await.unwrap();
    let thread: i64 =
        sqlx::query_scalar("INSERT INTO content.threads(board) VALUES('j') RETURNING id")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    private_comment_context(&mut tx, "Owned private polling fixture").await;
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,'j',$1,'Anonymous','','Owned private polling fixture')")
        .bind(thread).execute(&mut *tx).await.unwrap();
    private_comment_context(&mut tx, "Owned private reply").await;
    let reply: i64 = sqlx::query_scalar("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES('j',$1,'Anonymous','','Owned private reply') RETURNING id")
        .bind(thread).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();

    let result = tokio::spawn({
        let owner = owner.clone();
        async move {
            let app = router(state);
            for role in ["janitor", "moderator", "manager", "admin"] {
                sqlx::query("UPDATE staff_identity.accounts SET role=$2 WHERE id=$1")
                    .bind(account)
                    .bind(role)
                    .execute(&owner)
                    .await
                    .unwrap();
                for path in [
                    "/latest.php",
                    "/j/latest.php",
                    "/imgboard.php?mode=latest",
                    "/j/imgboard.php?mode=latest",
                ] {
                    let response = app
                        .clone()
                        .oneshot(
                            Request::get(path)
                                .header("cookie", format!("staff={token}"))
                                .body(Body::empty())
                                .unwrap(),
                        )
                        .await
                        .unwrap();
                    assert_eq!(response.status(), 200, "{role}: {path}");
                    assert_eq!(response.headers()["cache-control"], "private, no-store");
                    assert_eq!(response.headers()["content-type"], "application/json");
                    assert!(
                        response
                            .headers()
                            .get("access-control-allow-origin")
                            .is_none()
                    );
                    let value: Value = serde_json::from_slice(
                        &to_bytes(response.into_body(), 1024).await.unwrap(),
                    )
                    .unwrap();
                    assert_eq!(value, serde_json::json!({"no": reply}));
                }
            }
            sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1")
                .bind(reply)
                .execute(&owner)
                .await
                .unwrap();
            let response = app
                .clone()
                .oneshot(
                    Request::get("/latest.php")
                        .header("cookie", format!("staff={token}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            let value: Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 1024).await.unwrap())
                    .unwrap();
            assert_eq!(value, serde_json::json!({"no": thread}));
            sqlx::query(
                "UPDATE staff_identity.accounts SET revoked_at=clock_timestamp() WHERE id=$1",
            )
            .bind(account)
            .execute(&owner)
            .await
            .unwrap();
            let denied = app
                .clone()
                .oneshot(
                    Request::get("/latest.php")
                        .header("cookie", format!("staff={token}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(denied.status(), 401);
            let denied = app
                .oneshot(Request::get("/latest.php").body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(denied.status(), 401);

            let public =
                board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
                    .await
                    .unwrap();
            let visible: i64 =
                sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE thread_id=$1")
                    .bind(thread)
                    .fetch_one(&public)
                    .await
                    .unwrap();
            assert_eq!(visible, 0);
            public.close().await;
        }
    })
    .await;
    for statement in [
        "DELETE FROM content.posts WHERE thread_id=$1",
        "DELETE FROM content.threads WHERE id=$1",
    ] {
        sqlx::query(statement)
            .bind(thread)
            .execute(&owner)
            .await
            .unwrap();
    }
    for statement in [
        "DELETE FROM staff_identity.sessions WHERE account_id=$1",
        "DELETE FROM staff_identity.credentials WHERE account_id=$1",
        "DELETE FROM staff_identity.accounts WHERE id=$1",
    ] {
        sqlx::query(statement)
            .bind(account)
            .execute(&owner)
            .await
            .unwrap();
    }
    auth_pool.close().await;
    staff_pool.close().await;
    owner.close().await;
    result.unwrap();
}
