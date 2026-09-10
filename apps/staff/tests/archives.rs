#![cfg(feature = "database-tests")]

use board_staff::{AppError, auth::Session, store::moderate};
use sqlx::PgPool;

#[tokio::test]
async fn archived_threads_cannot_be_reopened_but_removal_is_audited() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let staff = PgPool::connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let slug = format!("s{}", &uuid::Uuid::new_v4().simple().to_string()[..9]);
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,archive_retention_seconds) VALUES ($1,'Archive moderation','Synthetic',100,20,10,1,1,3600)").bind(&slug).execute(&owner).await.unwrap();
    let id:i64=sqlx::query_scalar("INSERT INTO content.threads(board,archived_at,archive_expires_at) VALUES ($1,clock_timestamp(),clock_timestamp()+interval '1 hour') RETURNING id").bind(&slug).fetch_one(&owner).await.unwrap();
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES ($1,$2,$1,'Anonymous','Synthetic','Archive moderation fixture')").bind(id).bind(&slug).execute(&owner).await.unwrap();
    sqlx::query(
        "INSERT INTO content.reports(board,post_id,reason) VALUES($1,$2,'Owned archive report')",
    )
    .bind(&slug)
    .bind(id)
    .execute(&owner)
    .await
    .unwrap();
    let test_staff = staff.clone();
    let test_owner = owner.clone();
    let test_slug = slug.clone();
    let result = tokio::spawn(async move {
        let session = Session {
            account_id: 42,
            role: "moderator".into(),
            csrf_hash: vec![],
            recent: true,
        };
        assert!(
            board_staff::store::reports(&test_staff)
                .await
                .unwrap()
                .iter()
                .find(|report| report.board == test_slug)
                .unwrap()
                .closed
        );
        for action in ["reopen", "sticky"] {
            assert!(
                matches!(
                    moderate(&test_staff, &session, &test_slug, id, action).await,
                    Err(AppError::Invalid)
                ),
                "archived action {action}"
            );
        }
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM content.moderation_audit WHERE board=$1")
                .bind(&test_slug)
                .fetch_one(&test_owner)
                .await
                .unwrap();
        assert_eq!(count, 0);
        moderate(&test_staff, &session, &test_slug, id, "remove-thread")
            .await
            .unwrap();
        let removed: bool = sqlx::query_scalar("SELECT deleted FROM content.threads WHERE id=$1")
            .bind(id)
            .fetch_one(&test_owner)
            .await
            .unwrap();
        assert!(removed);
        let action: String =
            sqlx::query_scalar("SELECT action FROM content.moderation_audit WHERE board=$1")
                .bind(&test_slug)
                .fetch_one(&test_owner)
                .await
                .unwrap();
        assert_eq!(action, "remove-thread");
    })
    .await;
    sqlx::query("DELETE FROM content.moderation_audit WHERE board=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.reports WHERE board=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.posts WHERE board=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.threads WHERE board=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.boards WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    staff.close().await;
    owner.close().await;
    result.unwrap();
}
