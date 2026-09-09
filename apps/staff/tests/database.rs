#![cfg(feature = "database-tests")]
use sqlx::PgPool;

async fn pool(key: &str) -> PgPool {
    PgPool::connect(
        &std::env::var(key).expect("database credential required for explicit database tests"),
    )
    .await
    .unwrap()
}
async fn denied(pool: &PgPool, sql: &'static str) {
    let error = sqlx::query(sql)
        .execute(pool)
        .await
        .expect_err("operation unexpectedly permitted");
    assert_eq!(
        error.as_database_error().and_then(|e| e.code()).as_deref(),
        Some("42501")
    );
    assert_eq!(
        sqlx::query_scalar::<_, i32>("SELECT 1")
            .fetch_one(pool)
            .await
            .unwrap(),
        1,
        "healthy positive control"
    );
}
#[tokio::test]
async fn actual_runtime_logins_enforce_identity_boundaries() {
    let auth = pool("AUTH_DATABASE_URL").await;
    let staff = pool("STAFF_DATABASE_URL").await;
    let public = pool("TEST_PUBLIC_DATABASE_URL").await;
    board_staff::auth::check_identity(&auth, "board_auth")
        .await
        .unwrap();
    board_staff::auth::check_identity(&staff, "board_staff")
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT current_user::text")
            .fetch_one(&auth)
            .await
            .unwrap(),
        "board_auth"
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT current_user::text")
            .fetch_one(&staff)
            .await
            .unwrap(),
        "board_staff"
    );
    sqlx::query("SELECT id FROM staff_identity.accounts LIMIT 1")
        .execute(&auth)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM content.reports LIMIT 1")
        .execute(&staff)
        .await
        .unwrap();
    denied(
        &auth,
        "UPDATE staff_identity.accounts SET role='admin' WHERE false",
    )
    .await;
    denied(
        &auth,
        "INSERT INTO staff_identity.accounts(role) VALUES ('admin')",
    )
    .await;
    denied(&auth, "DELETE FROM staff_identity.credentials WHERE false").await;
    denied(
        &auth,
        "UPDATE staff_identity.credentials SET credential='{}'::jsonb WHERE false",
    )
    .await;
    denied(&auth,"INSERT INTO staff_identity.invitations(token_hash,account_id,expires_at) VALUES (decode(repeat('00',32),'hex'),1,clock_timestamp())").await;
    denied(&auth, "SELECT * FROM content.posts LIMIT 1").await;
    denied(&staff, "SELECT * FROM staff_identity.accounts LIMIT 1").await;
    denied(&staff, "DELETE FROM content.moderation_audit WHERE false").await;
    denied(
        &staff,
        "UPDATE content.moderation_audit SET action='close' WHERE false",
    )
    .await;
    denied(
        &staff,
        "UPDATE content.posts SET comment='changed' WHERE false",
    )
    .await;
    for p in [&auth, &staff, &public] {
        denied(p, "SELECT * FROM deployment.settings LIMIT 1").await;
        denied(p, "SET ROLE board_migrator").await;
        denied(p, "CREATE SCHEMA staff_test_forbidden").await;
    }
    denied(&public, "SELECT * FROM staff_identity.sessions LIMIT 1").await;
    denied(&public, "SELECT * FROM content.moderation_audit LIMIT 1").await;
    if let Ok(url) = std::env::var("MEDIA_DATABASE_URL") {
        let media = PgPool::connect(&url).await.unwrap();
        denied(&media, "SELECT * FROM staff_identity.accounts LIMIT 1").await;
        denied(&media, "SELECT * FROM content.moderation_audit LIMIT 1").await;
    }
    let failed=sqlx::query("SELECT staff_identity.enroll(decode(repeat('00',32),'hex'),decode('01','hex'),'{}'::jsonb)").execute(&auth).await;
    assert!(
        failed.is_err(),
        "enrollment without a live invitation must fail"
    );
}

#[tokio::test]
async fn moderation_persists_with_audit_and_rejects_cross_board_or_stale_authority() {
    use board_staff::{auth::Session, store::moderate};
    let owner = pool("MIGRATION_DATABASE_URL").await;
    let staff = pool("STAFF_DATABASE_URL").await;
    let board = format!("s{}", &uuid::Uuid::new_v4().simple().to_string()[..9]);
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_bytes,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES ($1,'Staff test','Synthetic',16000,100,100,100,10)").bind(&board).execute(&owner).await.unwrap();
    let id: i64 = sqlx::query_scalar("INSERT INTO content.threads(board) VALUES ($1) RETURNING id")
        .bind(&board)
        .fetch_one(&owner)
        .await
        .unwrap();
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES ($1,$2,$1,'Synthetic','Test','Harmless test comment')").bind(id).bind(&board).execute(&owner).await.unwrap();
    let report:i64=sqlx::query_scalar("INSERT INTO content.reports(board,post_id,reason) VALUES ($1,$2,'Harmless test report') RETURNING id").bind(&board).bind(id).fetch_one(&owner).await.unwrap();
    let mut session = Session {
        account_id: 42,
        role: "moderator".into(),
        csrf_hash: vec![],
        recent: true,
    };
    for action in ["close", "reopen", "sticky", "unsticky"] {
        moderate(&staff, &session, &board, id, action)
            .await
            .unwrap();
    }
    for action in ["resolve", "dismiss"] {
        moderate(&staff, &session, &board, report, action)
            .await
            .unwrap();
    }
    session.role = "unrecognized".into();
    assert!(
        moderate(&staff, &session, &board, id, "close")
            .await
            .is_err()
    );
    session.role = "moderator".into();
    session.recent = false;
    assert!(
        moderate(&staff, &session, &board, id, "close")
            .await
            .is_err()
    );
    session.recent = true;
    assert!(
        moderate(&staff, &session, "other", id, "close")
            .await
            .is_err()
    );
    moderate(&staff, &session, &board, id, "remove-thread")
        .await
        .unwrap();
    let state: (bool, bool, bool) =
        sqlx::query_as("SELECT closed,sticky,deleted FROM content.threads WHERE id=$1")
            .bind(id)
            .fetch_one(&owner)
            .await
            .unwrap();
    assert_eq!(state, (false, false, true));
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM content.moderation_audit WHERE board=$1")
            .bind(&board)
            .fetch_one(&owner)
            .await
            .unwrap();
    assert_eq!(count, 7);
    sqlx::query("DELETE FROM content.moderation_audit WHERE board=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.reports WHERE board=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.posts WHERE board=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.threads WHERE board=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.boards WHERE slug=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
}

#[tokio::test]
async fn credential_update_allows_backup_upgrade_but_not_key_replacement_or_downgrade() {
    let owner = pool("MIGRATION_DATABASE_URL").await;
    let auth = pool("AUTH_DATABASE_URL").await;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO staff_identity.accounts(role) VALUES ('moderator') RETURNING id",
    )
    .fetch_one(&owner)
    .await
    .unwrap();
    let key = uuid::Uuid::new_v4().as_bytes().to_vec();
    let previous = serde_json::json!({"cred":{"counter":0,"backup_eligible":false,"backup_state":false,"cred":{"synthetic":"test-only"}}});
    sqlx::query("INSERT INTO staff_identity.credentials(id,account_id,credential) VALUES ($1,$2,$3::text::jsonb)").bind(&key).bind(id).bind(previous.to_string()).execute(&owner).await.unwrap();
    let mut updated = previous.clone();
    updated["cred"]["backup_eligible"] = true.into();
    let result = sqlx::query_scalar::<_, bool>(
        "SELECT staff_identity.update_counter($1,$2::text::jsonb,$3::text::jsonb)",
    )
    .bind(&key)
    .bind(previous.to_string())
    .bind(updated.to_string())
    .fetch_one(&auth)
    .await;
    // Cleanup occurs before assertions, including when the regression is red.
    if result.as_ref().is_ok_and(|ok| *ok) {
        let downgrade =
            sqlx::query("SELECT staff_identity.update_counter($1,$2::text::jsonb,$3::text::jsonb)")
                .bind(&key)
                .bind(updated.to_string())
                .bind(previous.to_string())
                .execute(&auth)
                .await;
        let mut replacement = updated.clone();
        replacement["cred"]["cred"]["synthetic"] = "different".into();
        let changed_key =
            sqlx::query("SELECT staff_identity.update_counter($1,$2::text::jsonb,$3::text::jsonb)")
                .bind(&key)
                .bind(updated.to_string())
                .bind(replacement.to_string())
                .execute(&auth)
                .await;
        assert!(downgrade.is_err());
        assert!(changed_key.is_err());
    }
    sqlx::query("DELETE FROM staff_identity.credentials WHERE id=$1")
        .bind(&key)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM staff_identity.accounts WHERE id=$1")
        .bind(id)
        .execute(&owner)
        .await
        .unwrap();
    assert!(
        result.unwrap(),
        "legitimate backup eligibility upgrade should succeed"
    );
}
