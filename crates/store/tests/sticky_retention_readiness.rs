#![cfg(feature = "database-tests")]

use board_store::sticky_retention::READINESS_SQL;
use sqlx::PgPool;

#[tokio::test]
async fn retirement_readiness_rejects_missing_disabled_or_broadened_dependencies() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    for (variable, role) in [
        ("TEST_PUBLIC_DATABASE_URL", "board_public"),
        ("STAFF_DATABASE_URL", "board_staff"),
    ] {
        let runtime = PgPool::connect(&std::env::var(variable).unwrap())
            .await
            .unwrap();
        let actual: String = sqlx::query_scalar("SELECT current_user::text")
            .fetch_one(&runtime)
            .await
            .unwrap();
        assert_eq!(actual, role);
        assert!(
            sqlx::query_scalar::<_, bool>(READINESS_SQL)
                .fetch_one(&runtime)
                .await
                .unwrap()
        );
        runtime.close().await;
    }
    // Each schema fault exists only in this owner transaction. No fault is
    // committed, and no private credential row is read or modified. The same
    // catalog query checks the explicit runtime grants in both service roles.
    for fault in [
        "ALTER TABLE content.posts DISABLE TRIGGER retire_pruned_reply_credentials",
        "ALTER TRIGGER retire_pruned_reply_credentials ON content.posts RENAME TO owned_missing_retirement",
        "SET LOCAL ROLE board_posting_cooldown_owner; ALTER FUNCTION post_secrets.retire_pruned_reply_credentials() SECURITY INVOKER; RESET ROLE",
        "SET LOCAL ROLE board_posting_cooldown_owner; ALTER FUNCTION post_secrets.retire_pruned_reply_credentials() SET search_path=public; RESET ROLE",
        "SET LOCAL ROLE board_posting_cooldown_owner; GRANT EXECUTE ON FUNCTION post_secrets.retire_pruned_reply_credentials() TO PUBLIC; RESET ROLE",
        "SET LOCAL ROLE board_posting_cooldown_owner; GRANT EXECUTE ON FUNCTION post_secrets.retire_pruned_reply_credentials() TO board_public; RESET ROLE",
        "SET LOCAL ROLE board_posting_cooldown_owner; REVOKE EXECUTE ON FUNCTION post_secrets.retire_pruned_reply_credentials() FROM board_posting_cooldown_owner; RESET ROLE",
        "REVOKE DELETE ON post_secrets.anonymous_posts FROM board_posting_cooldown_owner",
        "REVOKE SELECT(post_id) ON post_secrets.anonymous_posts FROM board_posting_cooldown_owner",
        "GRANT SELECT(password_proof) ON post_secrets.anonymous_posts TO board_posting_cooldown_owner",
        "GRANT SELECT(password_proof) ON post_secrets.anonymous_posts TO board_public",
        "GRANT INSERT(post_id) ON post_secrets.anonymous_posts TO board_staff",
        "GRANT UPDATE(token_hash) ON post_secrets.anonymous_posts TO board_auth",
        "GRANT DELETE ON post_secrets.anonymous_posts TO board_public",
        "GRANT DELETE ON post_secrets.anonymous_posts TO board_media_read",
        "GRANT DELETE ON post_secrets.deletion TO board_media_intake",
        "REVOKE SELECT(undead) ON content.threads FROM board_posting_cooldown_owner",
        "REVOKE SELECT(reply_limit) ON content.boards FROM board_posting_cooldown_owner",
    ] {
        let mut tx = owner.begin().await.unwrap();
        sqlx::query("SET LOCAL lock_timeout='5s'")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(
            sqlx::query_scalar::<_, bool>(READINESS_SQL)
                .fetch_one(&mut *tx)
                .await
                .unwrap()
        );
        sqlx::raw_sql(fault).execute(&mut *tx).await.unwrap();
        let healthy: bool = sqlx::query_scalar(READINESS_SQL)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        tx.rollback().await.unwrap();
        assert!(!healthy, "schema fault remained ready: {fault}");
        assert!(
            sqlx::query_scalar::<_, bool>(READINESS_SQL)
                .fetch_one(&owner)
                .await
                .unwrap()
        );
    }
    owner.close().await;
}
