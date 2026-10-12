#![cfg(feature = "database-tests")]

use sqlx::{Connection, PgPool};

const SNAPSHOT: &str = "INSERT INTO content.moderation_audit(account_id,board,target_id,action,
 snapshot_version,snapshot_name,snapshot_subject,snapshot_comment,snapshot_comment_format,
 snapshot_staff_authorized_limits,snapshot_wordfiltered,snapshot_image_spoiler,
 snapshot_filename,snapshot_drawing_time_seconds,snapshot_drawing_source_post_id,
 created_at,snapshot_expires_at)
 VALUES(42,$1,7,'spoiler',2,'Private name','Private subject','Private comment',0,false,false,true,
 'private.png',12,6,'2100-01-01Z','2200-01-01Z') RETURNING id";

#[tokio::test]
async fn deadlines_are_server_owned_inclusive_bounded_atomic_and_content_only() {
    let pool = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let seed: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&pool)
        .await
        .unwrap();
    let board = format!("mr{seed}");
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Owned retention test','Synthetic',1000,100,100,100,10)")
        .bind(&board).execute(&pool).await.unwrap();
    let outcome = tokio::spawn({
        let pool = pool.clone(); let board = board.clone();
        async move {
            let ready: bool = sqlx::query_scalar("SELECT content.moderation_retention_ready()").fetch_one(&pool).await.unwrap();
            assert!(ready);
            let before: chrono::DateTime<chrono::Utc> = sqlx::query_scalar("SELECT clock_timestamp()").fetch_one(&pool).await.unwrap();
            let id: i64 = sqlx::query_scalar(SNAPSHOT).bind(&board).fetch_one(&pool).await.unwrap();
            let bounded: bool = sqlx::query_scalar("SELECT snapshot_expires_at BETWEEN $2::timestamptz+interval '720 hours' AND clock_timestamp()+interval '720 hours' AND created_at='2100-01-01Z'::timestamptz FROM content.moderation_audit WHERE id=$1")
                .bind(id).bind(before).fetch_one(&pool).await.unwrap();
            assert!(bounded, "caller-supplied timestamp/deadline must not extend retention");
            for role in ["board_public","board_staff","board_auth","board_media","board_media_read","board_media_intake","board_monitor"] {
                let allowed: bool = sqlx::query_scalar("SELECT has_function_privilege($1,'content.expire_moderation_snapshots()','EXECUTE') OR pg_has_role($1,'board_moderation_retention_owner','MEMBER')")
                    .bind(role).fetch_one(&pool).await.unwrap();
                assert!(!allowed, "{role}");
            }
            let client_private: bool = sqlx::query_scalar("SELECT has_any_column_privilege('board_moderation_retention','content.moderation_audit','SELECT,INSERT,UPDATE,REFERENCES') OR has_table_privilege('board_moderation_retention','content.moderation_audit','DELETE,TRUNCATE,TRIGGER')")
                .fetch_one(&pool).await.unwrap();
            assert!(!client_private);
            let public = PgPool::connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap()).await.unwrap();
            let denied = sqlx::query("SELECT content.expire_moderation_snapshots()").execute(&public).await.unwrap_err();
            assert_eq!(denied.as_database_error().unwrap().code().as_deref(),Some("42501"));
            public.close().await;

            let future: i64 = sqlx::query_scalar(SNAPSHOT).bind(&board).fetch_one(&pool).await.unwrap();
            let mut conn = pool.acquire().await.unwrap();
            let mut tx = conn.begin().await.unwrap();
            sqlx::query("UPDATE content.moderation_audit SET snapshot_expires_at=transaction_timestamp()+CASE WHEN id=$1 THEN interval '0' ELSE interval '1 microsecond' END WHERE id=ANY($2)")
                .bind(id).bind([id,future].as_slice()).execute(&mut *tx).await.unwrap();
            let metadata: serde_json::Value = sqlx::query_scalar("SELECT to_jsonb(a)-ARRAY['snapshot_version','snapshot_name','snapshot_trip','snapshot_capcode','snapshot_subject','snapshot_comment','snapshot_comment_format','snapshot_staff_authorized_limits','snapshot_wordfiltered','snapshot_image_spoiler','snapshot_filename','snapshot_dice_result','snapshot_fortune_text','snapshot_fortune_color','snapshot_drawing_time_seconds','snapshot_drawing_source_post_id'] FROM content.moderation_audit a WHERE id=$1")
                .bind(id).fetch_one(&mut *tx).await.unwrap();
            let count: i64 = sqlx::query_scalar("SELECT content.expire_moderation_snapshots()").fetch_one(&mut *tx).await.unwrap();
            assert_eq!(count,1,"equality expires; even one microsecond in the future is retained");
            let cleared: serde_json::Value = sqlx::query_scalar("SELECT to_jsonb(a) FROM content.moderation_audit a WHERE id=$1")
                .bind(id).fetch_one(&mut *tx).await.unwrap();
            for (name,value) in cleared.as_object().unwrap() {
                if name.starts_with("snapshot_") && name!="snapshot_expires_at" { assert!(value.is_null(),"{name}"); }
                else { assert_eq!(metadata[name],*value,"metadata {name}"); }
            }
            let retained: bool = sqlx::query_scalar("SELECT snapshot_comment='Private comment' FROM content.moderation_audit WHERE id=$1")
                .bind(future).fetch_one(&mut *tx).await.unwrap();
            assert!(retained);
            tx.rollback().await.unwrap();
            let retained: bool = sqlx::query_scalar("SELECT snapshot_comment='Private comment' FROM content.moderation_audit WHERE id=$1")
                .bind(id).fetch_one(&pool).await.unwrap();
            assert!(retained,"expiry rolls back atomically");

            // Only the owned records are aged. Ordinary audit metadata survives.
            sqlx::query("INSERT INTO content.moderation_audit(account_id,board,target_id,action) VALUES(42,$1,7,'remove-post')")
                .bind(&board).execute(&pool).await.unwrap();
            for _ in 0..68 { sqlx::query(SNAPSHOT).bind(&board).execute(&pool).await.unwrap(); }
            sqlx::query("UPDATE content.moderation_audit SET snapshot_expires_at=clock_timestamp()-interval '1 second' WHERE board=$1 AND snapshot_version IS NOT NULL")
                .bind(&board).execute(&pool).await.unwrap();
            for expected in [64_i64,6,0] {
                let count: i64 = sqlx::query_scalar("SELECT content.expire_moderation_snapshots()").fetch_one(&pool).await.unwrap();
                assert_eq!(count,expected);
            }
            let (records,payloads): (i64,i64) = sqlx::query_as("SELECT count(*),count(snapshot_version) FROM content.moderation_audit WHERE board=$1")
                .bind(&board).fetch_one(&pool).await.unwrap();
            assert_eq!((records,payloads),(71,0));
            // A busy row must not block another worker or be counted as erased.
            let locked: i64 = sqlx::query_scalar(SNAPSHOT).bind(&board).fetch_one(&pool).await.unwrap();
            let unlocked: i64 = sqlx::query_scalar(SNAPSHOT).bind(&board).fetch_one(&pool).await.unwrap();
            sqlx::query("UPDATE content.moderation_audit SET snapshot_expires_at=clock_timestamp()-interval '1 second' WHERE id=ANY($1)")
                .bind([locked,unlocked].as_slice()).execute(&pool).await.unwrap();
            let mut holder = pool.begin().await.unwrap();
            sqlx::query("SELECT id FROM content.moderation_audit WHERE id=$1 FOR UPDATE")
                .bind(locked).execute(&mut *holder).await.unwrap();
            let count: i64 = tokio::time::timeout(std::time::Duration::from_secs(2),
                sqlx::query_scalar("SELECT content.expire_moderation_snapshots()").fetch_one(&pool))
                .await.expect("locked snapshots must be skipped").unwrap();
            assert_eq!(count,1);
            let retained: bool = sqlx::query_scalar("SELECT snapshot_comment='Private comment' FROM content.moderation_audit WHERE id=$1")
                .bind(locked).fetch_one(&mut *holder).await.unwrap();
            assert!(retained);
            holder.rollback().await.unwrap();
            let count: i64 = sqlx::query_scalar("SELECT content.expire_moderation_snapshots()").fetch_one(&pool).await.unwrap();
            assert_eq!(count,1,"the next run expires the released row");
            let mut tx = pool.begin().await.unwrap();
            sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ").execute(&mut *tx).await.unwrap();
            let error = sqlx::query("SELECT content.expire_moderation_snapshots()").execute(&mut *tx).await.unwrap_err();
            assert_eq!(error.as_database_error().unwrap().code().as_deref(),Some("22023"));
            tx.rollback().await.unwrap();
        }
    }).await;
    sqlx::query("DELETE FROM content.moderation_audit WHERE board=$1")
        .bind(&board)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.boards WHERE slug=$1")
        .bind(&board)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    outcome.unwrap();
}
