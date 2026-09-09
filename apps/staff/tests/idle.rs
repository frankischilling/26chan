#![cfg(feature = "database-tests")]

use axum::http::{HeaderMap, HeaderValue};
use board_staff::{AppError, AppState, Config, Limits, auth};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use webauthn_rs::prelude::*;

struct Fixture {
    state: Arc<AppState>,
    owner: PgPool,
    account: i64,
    credential: Vec<u8>,
}

async fn pool(key: &str) -> PgPool {
    PgPool::connect(
        &std::env::var(key).expect("database credential required for explicit database tests"),
    )
    .await
    .unwrap()
}

async fn fixture() -> Fixture {
    let owner = pool("MIGRATION_DATABASE_URL").await;
    let auth_pool = pool("AUTH_DATABASE_URL").await;
    let account: i64 = sqlx::query_scalar(
        "INSERT INTO staff_identity.accounts(role) VALUES ('moderator') RETURNING id",
    )
    .fetch_one(&owner)
    .await
    .unwrap();
    let credential = uuid::Uuid::new_v4().as_bytes().to_vec();
    sqlx::query(
        "INSERT INTO staff_identity.credentials(id,account_id,credential) VALUES ($1,$2,'{}'::jsonb)",
    )
    .bind(&credential)
    .bind(account)
    .execute(&owner)
    .await
    .unwrap();
    let origin = Url::parse("http://localhost:3001").unwrap();
    Fixture {
        state: Arc::new(AppState {
            config: Config {
                origin: "http://localhost:3001".into(),
                bind: "127.0.0.1:3001".parse().unwrap(),
                production: false,
                auth_database: String::new(),
                staff_database: String::new(),
                idle_timeout: Duration::from_secs(60),
            },
            auth: auth_pool,
            staff: pool("STAFF_DATABASE_URL").await,
            webauthn: WebauthnBuilder::new("localhost", &origin)
                .unwrap()
                .build()
                .unwrap(),
            limits: Limits::default(),
        }),
        owner,
        account,
        credential,
    }
}

async fn insert_session(fixture: &Fixture, activity_age_seconds: i64) -> String {
    let session = auth::token();
    sqlx::query("INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id,authenticated_at,expires_at,last_activity_at) VALUES ($1,$2,$3,$4,clock_timestamp()-interval '5 minutes',clock_timestamp()+interval '1 hour',clock_timestamp()-$5::bigint*interval '1 second')")
        .bind(auth::hash(&session))
        .bind(auth::hash(&auth::token()))
        .bind(fixture.account)
        .bind(&fixture.credential)
        .bind(activity_age_seconds)
        .execute(&fixture.owner)
        .await
        .unwrap();
    session
}

fn headers(session: &str) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        "cookie",
        HeaderValue::from_str(&format!("staff={session}")).unwrap(),
    );
    headers
}

async fn cleanup(fixture: &Fixture) {
    sqlx::query("DELETE FROM staff_identity.sessions WHERE account_id=$1")
        .bind(fixture.account)
        .execute(&fixture.owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM staff_identity.credentials WHERE account_id=$1")
        .bind(fixture.account)
        .execute(&fixture.owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM staff_identity.accounts WHERE id=$1")
        .bind(fixture.account)
        .execute(&fixture.owner)
        .await
        .unwrap();
}

async fn observe_blocked_auth_backend(transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>) {
    let locker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut **transaction)
        .await
        .unwrap();
    for _ in 0..20 {
        let blocked: Option<i32> = sqlx::query_scalar("SELECT pid FROM pg_stat_activity WHERE usename='board_auth' AND $1=ANY(pg_blocking_pids(pid)) LIMIT 1")
            .bind(locker_pid)
            .fetch_optional(&mut **transaction)
            .await
            .unwrap();
        if blocked.is_some() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("board_auth never reached the expected row lock");
}

async fn wait_for_idle_deadline(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    token_hash: &[u8],
) {
    for _ in 0..150 {
        let expired: bool = sqlx::query_scalar("SELECT clock_timestamp()>=last_activity_at+(60*interval '1 second') FROM staff_identity.sessions WHERE token_hash=$1")
            .bind(token_hash)
            .fetch_one(&mut **transaction)
            .await
            .unwrap();
        if expired {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("database clock did not cross the idle deadline in time");
}

#[tokio::test]
async fn idle_session_is_denied_without_refreshing_persisted_activity() {
    let fixture = fixture().await;
    let session = insert_session(&fixture, 61).await;
    let before: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "SELECT last_activity_at FROM staff_identity.sessions WHERE token_hash=$1",
    )
    .bind(auth::hash(&session))
    .fetch_one(&fixture.owner)
    .await
    .unwrap();
    let result = auth::session(&fixture.state, &headers(&session)).await;
    let after: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "SELECT last_activity_at FROM staff_identity.sessions WHERE token_hash=$1",
    )
    .bind(auth::hash(&session))
    .fetch_one(&fixture.owner)
    .await
    .unwrap();
    cleanup(&fixture).await;
    assert!(matches!(result, Err(AppError::Unauthorized)));
    assert_eq!(after, before);
}

#[tokio::test]
async fn absolute_expiry_denies_a_session_with_fresh_activity() {
    let fixture = fixture().await;
    let session = insert_session(&fixture, 1).await;
    sqlx::query("UPDATE staff_identity.sessions SET expires_at=clock_timestamp()-interval '1 second' WHERE token_hash=$1")
        .bind(auth::hash(&session))
        .execute(&fixture.owner)
        .await
        .unwrap();
    let before: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "SELECT last_activity_at FROM staff_identity.sessions WHERE token_hash=$1",
    )
    .bind(auth::hash(&session))
    .fetch_one(&fixture.owner)
    .await
    .unwrap();
    let result = auth::session(&fixture.state, &headers(&session)).await;
    let after: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "SELECT last_activity_at FROM staff_identity.sessions WHERE token_hash=$1",
    )
    .bind(auth::hash(&session))
    .fetch_one(&fixture.owner)
    .await
    .unwrap();
    cleanup(&fixture).await;
    assert!(matches!(result, Err(AppError::Unauthorized)));
    assert_eq!(after, before);
}

#[tokio::test]
async fn stale_authentication_remains_non_recent_when_activity_is_live() {
    let fixture = fixture().await;
    let session = insert_session(&fixture, 1).await;
    sqlx::query("UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp()-interval '11 minutes' WHERE token_hash=$1")
        .bind(auth::hash(&session))
        .execute(&fixture.owner)
        .await
        .unwrap();
    let authenticated = auth::session(&fixture.state, &headers(&session)).await;
    cleanup(&fixture).await;
    assert!(authenticated.is_ok_and(|session| !session.recent));
}

#[tokio::test]
async fn live_session_refreshes_only_its_activity_and_stops_after_revocation() {
    let fixture = fixture().await;
    let live = insert_session(&fixture, 1).await;
    let separate = insert_session(&fixture, 1).await;
    let before: (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>) = sqlx::query_as("SELECT authenticated_at,expires_at,last_activity_at FROM staff_identity.sessions WHERE token_hash=$1")
        .bind(auth::hash(&live))
        .fetch_one(&fixture.owner)
        .await
        .unwrap();
    let separate_before: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "SELECT last_activity_at FROM staff_identity.sessions WHERE token_hash=$1",
    )
    .bind(auth::hash(&separate))
    .fetch_one(&fixture.owner)
    .await
    .unwrap();
    let authenticated = auth::session(&fixture.state, &headers(&live)).await;
    let after: (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>) = sqlx::query_as("SELECT authenticated_at,expires_at,last_activity_at FROM staff_identity.sessions WHERE token_hash=$1")
        .bind(auth::hash(&live))
        .fetch_one(&fixture.owner)
        .await
        .unwrap();
    let separate_after: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "SELECT last_activity_at FROM staff_identity.sessions WHERE token_hash=$1",
    )
    .bind(auth::hash(&separate))
    .fetch_one(&fixture.owner)
    .await
    .unwrap();
    sqlx::query("UPDATE staff_identity.accounts SET revoked_at=clock_timestamp() WHERE id=$1")
        .bind(fixture.account)
        .execute(&fixture.owner)
        .await
        .unwrap();
    let before_denial: chrono::DateTime<chrono::Utc> = after.2;
    let denied = auth::session(&fixture.state, &headers(&live)).await;
    let after_denial: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "SELECT last_activity_at FROM staff_identity.sessions WHERE token_hash=$1",
    )
    .bind(auth::hash(&live))
    .fetch_one(&fixture.owner)
    .await
    .unwrap();
    cleanup(&fixture).await;
    assert!(authenticated.is_ok());
    assert_eq!(after.0, before.0);
    assert_eq!(after.1, before.1);
    assert!(after.2 > before.2);
    assert_eq!(separate_after, separate_before);
    assert!(matches!(denied, Err(AppError::Unauthorized)));
    assert_eq!(after_denial, before_denial);
}

#[tokio::test]
async fn expired_activity_cannot_be_revived_while_waiting_for_its_row_lock() {
    let fixture = fixture().await;
    let session = insert_session(&fixture, 1).await;
    let mut transaction = fixture.owner.begin().await.unwrap();
    let expired: chrono::DateTime<chrono::Utc> = sqlx::query_scalar("UPDATE staff_identity.sessions SET last_activity_at=clock_timestamp()-interval '61 seconds' WHERE token_hash=$1 RETURNING last_activity_at")
        .bind(auth::hash(&session))
        .fetch_one(&mut *transaction)
        .await
        .unwrap();
    let state = fixture.state.clone();
    let request_headers = headers(&session);
    let mut waiting = tokio::spawn(async move { auth::session(&state, &request_headers).await });
    assert!(
        tokio::time::timeout(Duration::from_millis(100), &mut waiting)
            .await
            .is_err()
    );
    transaction.commit().await.unwrap();
    let result = waiting.await.unwrap();
    let after: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "SELECT last_activity_at FROM staff_identity.sessions WHERE token_hash=$1",
    )
    .bind(auth::hash(&session))
    .fetch_one(&fixture.owner)
    .await
    .unwrap();
    cleanup(&fixture).await;
    assert!(matches!(result, Err(AppError::Unauthorized)));
    assert_eq!(after, expired);
}

#[tokio::test]
async fn activity_deadline_is_checked_after_a_healthy_row_lock_wait() {
    let fixture = fixture().await;
    let session = insert_session(&fixture, 59).await;
    let token_hash = auth::hash(&session);
    let before: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "SELECT last_activity_at FROM staff_identity.sessions WHERE token_hash=$1",
    )
    .bind(&token_hash)
    .fetch_one(&fixture.owner)
    .await
    .unwrap();
    let mut transaction = fixture.owner.begin().await.unwrap();
    sqlx::query("SELECT token_hash FROM staff_identity.sessions WHERE token_hash=$1 FOR UPDATE")
        .bind(&token_hash)
        .execute(&mut *transaction)
        .await
        .unwrap();
    let state = fixture.state.clone();
    let request_headers = headers(&session);
    let waiting = tokio::spawn(async move { auth::session(&state, &request_headers).await });
    observe_blocked_auth_backend(&mut transaction).await;
    wait_for_idle_deadline(&mut transaction, &token_hash).await;
    transaction.commit().await.unwrap();
    let result = waiting.await.unwrap();
    let after: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
        "SELECT last_activity_at FROM staff_identity.sessions WHERE token_hash=$1",
    )
    .bind(token_hash)
    .fetch_one(&fixture.owner)
    .await
    .unwrap();
    cleanup(&fixture).await;
    assert!(matches!(result, Err(AppError::Unauthorized)));
    assert_eq!(after, before);
}

#[tokio::test]
async fn activity_column_grant_is_limited_to_board_auth() {
    let auth = pool("AUTH_DATABASE_URL").await;
    let public = pool("TEST_PUBLIC_DATABASE_URL").await;
    let staff = pool("STAFF_DATABASE_URL").await;
    sqlx::query(
        "UPDATE staff_identity.sessions SET last_activity_at=clock_timestamp() WHERE false",
    )
    .execute(&auth)
    .await
    .unwrap();
    for (pool, statement) in [
        (
            &auth,
            "UPDATE staff_identity.sessions SET expires_at=clock_timestamp() WHERE false",
        ),
        (
            &public,
            "UPDATE staff_identity.sessions SET last_activity_at=clock_timestamp() WHERE false",
        ),
        (
            &staff,
            "UPDATE staff_identity.sessions SET last_activity_at=clock_timestamp() WHERE false",
        ),
    ] {
        let error = sqlx::query(statement).execute(pool).await.unwrap_err();
        assert_eq!(
            error
                .as_database_error()
                .and_then(|error| error.code())
                .as_deref(),
            Some("42501")
        );
    }
}
