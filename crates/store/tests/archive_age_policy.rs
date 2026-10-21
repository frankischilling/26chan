#![cfg(feature = "database-tests")]
mod support;

use board_store::{NewPost, StoreError};
use sqlx::PgPool;

async fn seed(owner: &PgPool, board: &str, count: i64) -> Vec<i64> {
    let mut ids: Vec<i64> = sqlx::query_scalar("WITH inserted AS (INSERT INTO content.threads(board,archived_at,archive_expires_at,bumped_at) SELECT $1,CASE WHEN n=1 THEN statement_timestamp()-interval '73 hours' ELSE statement_timestamp() END,statement_timestamp()+interval '5 days',CASE WHEN n=1 THEN statement_timestamp()-interval '73 hours' ELSE statement_timestamp() END FROM generate_series(1,$2::bigint) n RETURNING id) INSERT INTO content.posts(id,board,thread_id,name,subject,comment) SELECT id,$1,id,'Anonymous','Owned archive','Synthetic stored archive' FROM inserted RETURNING id")
        .bind(board).bind(count).fetch_all(owner).await.unwrap();
    ids.sort_unstable();
    ids
}

async fn fixture() -> (PgPool, PgPool, String) {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT 'as'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(captcha_enabled,slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,archive_retention_seconds,archive_limit,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,user_thread_limit) VALUES(false,$1,'Archive storage','Owned synthetic',2000,1000,300,1,1,864000,1,0,0,0,100)")
        .bind(&board).execute(&owner).await.unwrap();
    (owner, public, board)
}

async fn cleanup(owner: &PgPool, public: &PgPool, board: &str) {
    for query in [
        "DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(query).bind(board).execute(owner).await.unwrap();
    }
    public.close().await;
    owner.close().await;
}

#[tokio::test]
async fn retention_changes_recompute_existing_archives_without_restoring_deleted_rows() {
    let (owner, public, board) = fixture().await;
    let (o, p, b) = (owner.clone(), public.clone(), board.clone());
    let result = tokio::spawn(async move {
        let ids = seed(&o, &b, 3).await;
        sqlx::query("UPDATE content.threads SET archived_at=statement_timestamp()-interval '2 hours',archive_expires_at=statement_timestamp()+interval '1 hour' WHERE board=$1")
            .bind(&b).execute(&o).await.unwrap();
        sqlx::query("UPDATE content.threads SET deleted=true WHERE board=$1 AND id=$2")
            .bind(&b).bind(ids[2]).execute(&o).await.unwrap();
        let deleted_deadline: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar("SELECT archive_expires_at FROM content.threads WHERE board=$1 AND id=$2")
            .bind(&b).bind(ids[2]).fetch_one(&o).await.unwrap();
        sqlx::query("UPDATE content.boards SET archive_retention_seconds=14400 WHERE slug=$1")
            .bind(&b).execute(&o).await.unwrap();
        let correct: bool = sqlx::query_scalar("SELECT bool_and(archive_expires_at=archived_at+interval '4 hours') FROM content.threads WHERE board=$1 AND NOT deleted")
            .bind(&b).fetch_one(&o).await.unwrap();
        assert!(correct, "current board age must replace saved creation-time expiry");
        let mut fixed = o.begin().await.unwrap();
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *fixed).await.unwrap();
        let error = sqlx::query("UPDATE content.boards SET archive_retention_seconds=7200 WHERE slug=$1")
            .bind(&b).execute(&mut *fixed).await.unwrap_err();
        assert_eq!(error.as_database_error().and_then(|e| e.code()).as_deref(), Some("22023"));
        fixed.rollback().await.unwrap();
        let denied = sqlx::query("UPDATE content.boards SET archive_retention_seconds=7200 WHERE slug=$1")
            .bind(&b).execute(&p).await.unwrap_err();
        assert_eq!(denied.as_database_error().and_then(|e| e.code()).as_deref(), Some("42501"));
        let mut check = o.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_thread_transfer_owner").execute(&mut *check).await.unwrap();
        let ready: bool = sqlx::query_scalar("SELECT content.text_thread_transfer_ready()")
            .fetch_one(&mut *check).await.unwrap();
        check.commit().await.unwrap();
        assert!(ready, "archive policy migration must preserve sealed transfer readiness");

        assert_eq!(board_store::archive_snapshot(&p, &b).await.unwrap().entries.len(), 2);
        sqlx::query("UPDATE content.boards SET archive_retention_seconds=3600 WHERE slug=$1")
            .bind(&b).execute(&o).await.unwrap();
        assert!(board_store::archive_snapshot(&p, &b).await.unwrap().entries.is_empty());
        sqlx::query("UPDATE content.boards SET archive_retention_seconds=-1 WHERE slug=$1")
            .bind(&b).execute(&o).await.unwrap();
        assert_eq!(board_store::archive_snapshot(&p, &b).await.unwrap().entries.len(), 2);
        let archived = board_store::thread(&p, &b, ids[0]).await.unwrap();
        assert!(archived.archived_at.is_some());
        assert!(archived.archive_expires_at.is_none(), "unlimited deadlines decode without Chrono overflow");
        let unlimited: bool = sqlx::query_scalar("SELECT bool_and(archive_expires_at='infinity'::timestamptz) FROM content.threads WHERE board=$1 AND NOT deleted")
            .bind(&b).fetch_one(&o).await.unwrap();
        assert!(unlimited);
        assert!(board_store::report_target(&p, &b, ids[0]).await.is_ok());
        sqlx::query("UPDATE content.boards SET staff_only=true WHERE slug=$1")
            .bind(&b).execute(&o).await.unwrap();
        assert!(matches!(board_store::archive_snapshot(&p, &b).await, Err(StoreError::NotFound)));
        assert!(matches!(board_store::thread(&p, &b, ids[0]).await, Err(StoreError::NotFound)));
        assert!(matches!(board_store::report_target(&p, &b, ids[0]).await, Err(StoreError::NotFound)));
        sqlx::query("UPDATE content.boards SET staff_only=false WHERE slug=$1")
            .bind(&b).execute(&o).await.unwrap();
        sqlx::query("UPDATE content.boards SET archive_retention_seconds=0 WHERE slug=$1")
            .bind(&b).execute(&o).await.unwrap();
        assert!(matches!(board_store::archive_snapshot(&p, &b).await, Err(StoreError::NotFound)));
        sqlx::query("UPDATE content.boards SET archive_retention_seconds=18000 WHERE slug=$1")
            .bind(&b).execute(&o).await.unwrap();
        assert_eq!(board_store::archive_snapshot(&p, &b).await.unwrap().entries.len(), 2);
        let after: (bool, Option<chrono::DateTime<chrono::Utc>>) = sqlx::query_as("SELECT deleted,archive_expires_at FROM content.threads WHERE board=$1 AND id=$2")
            .bind(&b).bind(ids[2]).fetch_one(&o).await.unwrap();
        assert_eq!(after, (true, deleted_deadline));
    }).await;
    cleanup(&owner, &public, &board).await;
    result.unwrap();
}

#[tokio::test]
async fn unlimited_policy_preserves_rollover_autoarchive_and_disabled_storage() {
    let (owner, public, board) = fixture().await;
    let (o, p, b) = (owner.clone(), public.clone(), board.clone());
    let result = tokio::spawn(async move {
        sqlx::query("UPDATE content.boards SET archive_retention_seconds=-1,autoarchive_cap=2,reply_limit=2,bump_limit=2 WHERE slug=$1")
            .bind(&b).execute(&o).await.unwrap();
        let post = || NewPost { name: "Anonymous".into(), subject: "Owned unlimited archive".into(),
            comment: support::distinct_comment("Synthetic archive policy"), deletion_hash: "owned-fixture".into(), sage: false };
        let first = support::create_post(&p, &b, 0, &post()).await.unwrap();
        let second = support::create_post(&p, &b, 0, &post()).await.unwrap();
        let archived = board_store::thread(&p, &b, first).await.unwrap();
        assert!(archived.archived_at.is_some());
        assert!(archived.archive_expires_at.is_none());
        support::create_post(&p, &b, second, &post()).await.unwrap();
        support::create_post(&p, &b, second, &post()).await.unwrap();
        let archived = board_store::thread(&p, &b, second).await.unwrap();
        assert!(archived.archived_at.is_some());
        assert!(archived.archive_expires_at.is_none());
        assert_eq!(archived.reply_count, 2);
        assert_eq!(board_store::archive_snapshot(&p, &b).await.unwrap().entries.len(), 2);
        sqlx::query("UPDATE content.boards SET archive_retention_seconds=0 WHERE slug=$1")
            .bind(&b).execute(&o).await.unwrap();
        support::create_post(&p, &b, 0, &post()).await.unwrap();
        assert!(matches!(board_store::archive_snapshot(&p, &b).await, Err(StoreError::NotFound)));
        sqlx::query("UPDATE content.boards SET archive_retention_seconds=3600 WHERE slug=$1")
            .bind(&b).execute(&o).await.unwrap();
        assert_eq!(board_store::archive_snapshot(&p, &b).await.unwrap().entries.len(), 2);
        assert!(board_store::thread(&p, &b, second).await.unwrap().archive_expires_at.is_some());
    }).await;
    cleanup(&owner, &public, &board).await;
    result.unwrap();
}

#[tokio::test]
async fn waiting_policy_update_reconciles_an_archive_committed_before_its_board_lock() {
    let (owner, public, board) = fixture().await;
    let (o, b) = (owner.clone(), board.clone());
    let result = tokio::spawn(async move {
        let id: i64 = sqlx::query_scalar("WITH inserted AS (INSERT INTO content.threads(board) VALUES($1) RETURNING id) INSERT INTO content.posts(id,board,thread_id,name,subject,comment) SELECT id,$1,id,'Anonymous','Owned lock order','Synthetic archive policy lock' FROM inserted RETURNING id")
            .bind(&b).fetch_one(&o).await.unwrap();
        let mut writer = o.begin().await.unwrap();
        let writer_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *writer).await.unwrap();
        sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
            .bind(&b).fetch_one(&mut *writer).await.unwrap();
        let mut operator = o.acquire().await.unwrap();
        let operator_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *operator).await.unwrap();
        let update_board = b.clone();
        let update = tokio::spawn(async move {
            sqlx::query("UPDATE content.boards SET archive_retention_seconds=7200 WHERE slug=$1")
                .bind(update_board).execute(&mut *operator).await
        });
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            loop {
                let blocked: bool = sqlx::query_scalar("SELECT $2=ANY(pg_blocking_pids($1))")
                    .bind(operator_pid).bind(writer_pid).fetch_one(&o).await.unwrap();
                if blocked { break; }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }).await.expect("operator must wait on the owned board lock");
        sqlx::query("UPDATE content.threads SET archived_at=statement_timestamp(),archive_expires_at=statement_timestamp()+interval '10 days' WHERE board=$1 AND id=$2")
            .bind(&b).bind(id).execute(&mut *writer).await.unwrap();
        writer.commit().await.unwrap();
        update.await.unwrap().unwrap();
        let correct: bool = sqlx::query_scalar("SELECT archive_expires_at=archived_at+interval '2 hours' FROM content.threads WHERE board=$1 AND id=$2")
            .bind(&b).bind(id).fetch_one(&o).await.unwrap();
        assert!(correct, "the current-age update must include the newly committed archive");
    }).await;
    cleanup(&owner, &public, &board).await;
    result.unwrap();
}
