#![cfg(feature = "database-tests")]

use board_store::post_media::FILE_DELETION_READINESS_SQL;
use sqlx::PgPool;

#[tokio::test]
async fn actual_file_action_roles_are_ready_and_schema_authority_drift_fails_closed() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    for (variable, expected) in [
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
        assert_eq!(actual, expected);
        assert!(
            sqlx::query_scalar::<_, bool>(FILE_DELETION_READINESS_SQL)
                .fetch_one(&runtime)
                .await
                .unwrap()
        );
        for statement in [
            "UPDATE content.posts SET file_deleted=true WHERE false",
            "DELETE FROM content.reports WHERE false",
            "SELECT post_secrets.retire_staff_file_report_membership('missing',1)",
            "SET ROLE board_attachment_owner",
            "SET ROLE board_report_admission_owner",
        ] {
            let error = sqlx::query(statement).execute(&runtime).await.unwrap_err();
            assert_eq!(
                error.as_database_error().unwrap().code().as_deref(),
                Some("42501"),
                "{expected}: {statement}"
            );
        }
        runtime.close().await;
    }
    // Transaction-local catalog faults only. No changed grants, functions or
    // defaults are committed, and no production content is created or touched.
    for fault in [
        "ALTER TABLE content.posts ALTER COLUMN file_deleted DROP NOT NULL",
        "ALTER TABLE content.posts ALTER COLUMN file_deleted SET DEFAULT true",
        "ALTER TABLE content.posts ALTER COLUMN file_deleted DROP DEFAULT",
        "REVOKE UPDATE(file_deleted) ON content.posts FROM board_attachment_owner",
        "GRANT UPDATE(file_deleted) ON content.posts TO board_public",
        "GRANT INSERT(file_deleted) ON content.posts TO board_staff",
        "GRANT UPDATE(file_deleted) ON content.posts TO board_media_read",
        "GRANT UPDATE ON content.posts TO board_attachment_owner",
        "GRANT DELETE ON content.reports TO board_attachment_owner",
        "GRANT DELETE ON content.reports TO board_staff",
        "REVOKE DELETE ON content.reports FROM board_report_admission_owner",
        "SET LOCAL ROLE board_attachment_owner; ALTER FUNCTION content.delete_post_attachment(text,bigint) SECURITY INVOKER; RESET ROLE",
        "SET LOCAL ROLE board_attachment_owner; ALTER FUNCTION content.staff_delete_post_attachment(text,bigint) SET search_path=public; RESET ROLE",
        "SET LOCAL ROLE board_attachment_owner; GRANT EXECUTE ON FUNCTION content.staff_delete_post_attachment(text,bigint) TO board_public; RESET ROLE",
        "SET LOCAL ROLE board_attachment_owner; GRANT EXECUTE ON FUNCTION content.delete_post_attachment(text,bigint) TO board_auth; RESET ROLE",
        "SET LOCAL ROLE board_attachment_owner; GRANT EXECUTE ON FUNCTION content.delete_post_attachment(text,bigint) TO PUBLIC; RESET ROLE",
        "SET LOCAL ROLE board_report_admission_owner; GRANT EXECUTE ON FUNCTION post_secrets.retire_staff_file_report_membership(text,bigint) TO board_staff; RESET ROLE",
        "SET LOCAL ROLE board_report_admission_owner; REVOKE EXECUTE ON FUNCTION post_secrets.retire_staff_file_report_membership(text,bigint) FROM board_attachment_owner; RESET ROLE",
        "ALTER VIEW content.visible_post_media RESET(security_barrier)",
        "ALTER VIEW content.staff_post_media RESET(security_barrier)",
        "ALTER VIEW media.approved_assets RESET(security_barrier)",
    ] {
        let mut tx = owner.begin().await.unwrap();
        sqlx::query("SET LOCAL lock_timeout='5s'")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert!(
            sqlx::query_scalar::<_, bool>(FILE_DELETION_READINESS_SQL)
                .fetch_one(&mut *tx)
                .await
                .unwrap()
        );
        sqlx::raw_sql(fault).execute(&mut *tx).await.unwrap();
        let healthy: bool = sqlx::query_scalar(FILE_DELETION_READINESS_SQL)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        tx.rollback().await.unwrap();
        assert!(!healthy, "Readiness accepted catalog fault: {fault}");
        assert!(
            sqlx::query_scalar::<_, bool>(FILE_DELETION_READINESS_SQL)
                .fetch_one(&owner)
                .await
                .unwrap()
        );
    }
    owner.close().await;
}
