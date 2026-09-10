#![cfg(feature = "database-tests")]

use board_store::{
    StoreError,
    monitoring::{MonitorReader, QueueSnapshot},
};
use sqlx::{Executor, PgPool};

#[tokio::test]
async fn aggregate_only_login_observes_queue_windows_and_rejects_unsafe_grants() {
    assert!(
        std::env::var("BOARD_TEST_CLUSTER")
            .is_ok_and(|path| path.starts_with("/tmp/board-postgres.")),
        "Requires the explicit owned development cluster marker"
    );
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let monitor_url = std::env::var("MONITOR_DATABASE_URL").unwrap();
    let fixture: String = sqlx::query_scalar("SELECT gen_random_uuid()::text")
        .fetch_one(&owner)
        .await
        .unwrap();
    let capacity: i32 =
        sqlx::query_scalar("SELECT capacity FROM media.queue_policy WHERE singleton")
            .fetch_one(&owner)
            .await
            .unwrap();
    let view_definition: String =
        sqlx::query_scalar("SELECT pg_get_viewdef('monitoring.media_queue'::regclass)")
            .fetch_one(&owner)
            .await
            .unwrap();
    let baseline: i64 = sqlx::query_scalar("SELECT count(*) FROM media.jobs")
        .fetch_one(&owner)
        .await
        .unwrap();
    assert_eq!(
        baseline, 0,
        "Use an empty owned disposable queue; never delete unrelated jobs"
    );
    let task_owner = owner.clone();
    let task_fixture = fixture.clone();
    let result =
        tokio::spawn(
            async move { exercise(task_owner, monitor_url, task_fixture, capacity).await },
        )
        .await;
    // Restore only changes made by this test even when an assertion panics.
    // This definition came from pg_get_viewdef on the owned, migrated fixture; no user input.
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE OR REPLACE VIEW monitoring.media_queue WITH(security_barrier=true) AS {view_definition}"))).execute(&owner).await.unwrap();
    owner.execute("REVOKE ALL ON media.jobs FROM board_monitor; REVOKE SELECT(filename) ON media.jobs FROM board_monitor; REVOKE ALL ON SCHEMA media FROM board_monitor; REVOKE CREATE ON SCHEMA monitoring FROM board_monitor; GRANT USAGE ON SCHEMA monitoring TO board_monitor; GRANT SELECT ON monitoring.media_queue TO board_monitor").await.unwrap();
    sqlx::query("INSERT INTO media.queue_policy(singleton, capacity) VALUES(true,$1) ON CONFLICT(singleton) DO UPDATE SET capacity=EXCLUDED.capacity").bind(capacity).execute(&owner).await.unwrap();
    sqlx::query("DELETE FROM media.jobs WHERE filename=$1")
        .bind(&fixture)
        .execute(&owner)
        .await
        .unwrap();
    owner.close().await;
    result.unwrap();
}

async fn exercise(owner: PgPool, monitor_url: String, fixture: String, capacity: i32) {
    let reader = MonitorReader::connect(&monitor_url).await.unwrap();
    let monitor = PgPool::connect(&monitor_url).await.unwrap();
    let empty = reader.snapshot().await.unwrap();
    assert_eq!(
        empty,
        QueueSnapshot {
            capacity: i64::from(capacity),
            receiving: 0,
            queued: 0,
            processing: 0,
            expired_receiving: 0,
            expired_queued: 0,
            expired_processing: 0,
            oldest_queued_seconds: 0,
            intake_failed: 0,
            abandoned: 0,
            processing_failed: 0,
            invalid_output: 0,
            retry_exhausted: 0
        }
    );
    let barrier: bool = sqlx::query_scalar("SELECT 'security_barrier=true'=ANY(reloptions) AND relowner=(SELECT oid FROM pg_roles WHERE rolname='board_migrator') FROM pg_class WHERE oid='monitoring.media_queue'::regclass").fetch_one(&owner).await.unwrap();
    assert!(barrier);
    let columns: Vec<(String,String)> = sqlx::query_as("SELECT attname::text, format_type(atttypid,atttypmod) FROM pg_attribute WHERE attrelid='monitoring.media_queue'::regclass AND attnum>0 AND NOT attisdropped ORDER BY attnum").fetch_all(&owner).await.unwrap();
    assert_eq!(
        columns
            .iter()
            .map(|(name, _)| name.as_str())
            .collect::<Vec<_>>(),
        [
            "capacity",
            "receiving",
            "queued",
            "processing",
            "expired_receiving",
            "expired_queued",
            "expired_processing",
            "oldest_queued_seconds",
            "intake_failed",
            "abandoned",
            "processing_failed",
            "invalid_output",
            "retry_exhausted"
        ]
    );
    assert!(columns.iter().all(|(_, kind)| kind == "bigint"));
    for query in [
        "SELECT filename FROM media.jobs LIMIT 0",
        "SELECT capacity FROM media.queue_policy LIMIT 0",
        "SELECT sha256 FROM media.assets LIMIT 0",
        "SELECT comment FROM content.posts LIMIT 0",
        "SELECT * FROM post_secrets.deletion LIMIT 0",
        "UPDATE media.jobs SET filename=filename WHERE false",
        "DELETE FROM media.jobs WHERE false",
    ] {
        let error = sqlx::query(query).execute(&monitor).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501"),
            "{query}"
        );
    }
    // Healthy owner controls distinguish genuine permission denial from a bad table/query.
    for query in [
        "SELECT filename FROM media.jobs LIMIT 0",
        "SELECT capacity FROM media.queue_policy LIMIT 0",
        "SELECT sha256 FROM media.assets LIMIT 0",
        "SELECT comment FROM content.posts LIMIT 0",
        "SELECT * FROM post_secrets.deletion LIMIT 0",
        "UPDATE media.jobs SET filename=filename WHERE false",
        "DELETE FROM media.jobs WHERE false",
    ] {
        sqlx::query(query).execute(&owner).await.unwrap();
    }
    for query in [
        "UPDATE monitoring.media_queue SET capacity=1",
        "DELETE FROM monitoring.media_queue",
        "INSERT INTO monitoring.media_queue(capacity) VALUES(1)",
    ] {
        let error = sqlx::query(query).execute(&monitor).await.unwrap_err();
        assert!(matches!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501" | "55000")
        ));
        // Even the owner cannot turn this aggregate into a base-table write.
        let error = sqlx::query(query).execute(&owner).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("55000")
        );
    }
    assert!(
        MonitorReader::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
            .await
            .is_err()
    );
    // Owner writes deterministic synthetic records; production constraints remain active.
    for state in ["receiving", "queued", "processing"] {
        for expired in [false, true] {
            sqlx::query("INSERT INTO media.jobs(id,filename,state,input_bytes,attempts,lease_token,expires_at,created_at) VALUES(replace(gen_random_uuid()::text,'-',''),$1,$2,1,CASE WHEN $2='processing' THEN 1 ELSE 0 END,CASE WHEN $2='processing' THEN repeat('a',32) ELSE NULL END,statement_timestamp()+CASE WHEN $3 THEN interval '-1 minute' ELSE interval '1 hour' END,statement_timestamp()-interval '2 minutes')")
                .bind(&fixture).bind(state).bind(expired).execute(&owner).await.unwrap();
        }
    }
    sqlx::query("INSERT INTO media.jobs(id,filename,state,input_bytes,attempts,lease_token,output_sha256,output_bytes) VALUES(replace(gen_random_uuid()::text,'-',''),$1,'published',1,1,repeat('b',32),repeat('c',64),1)").bind(&fixture).execute(&owner).await.unwrap();
    for reason in [
        "intake_failed",
        "abandoned",
        "processing_failed",
        "invalid_output",
        "retry_exhausted",
    ] {
        // Both sides of the 15-minute boundary, plus a future-dated row. The
        // ten-second margin accommodates hosted scheduling while preserving the window.
        for age in [
            "14 minutes 50 seconds",
            "15 minutes 10 seconds",
            "-1 minute",
        ] {
            sqlx::query("INSERT INTO media.jobs(id,filename,state,failure,updated_at) VALUES(replace(gen_random_uuid()::text,'-',''),$1,'failed',$2,statement_timestamp()-$3::interval)")
                .bind(&fixture).bind(reason).bind(age).execute(&owner).await.unwrap();
        }
    }
    let snapshot = reader.snapshot().await.unwrap();
    assert_eq!(
        (snapshot.receiving, snapshot.queued, snapshot.processing),
        (2, 2, 2)
    );
    assert_eq!(
        (
            snapshot.expired_receiving,
            snapshot.expired_queued,
            snapshot.expired_processing
        ),
        (1, 1, 1)
    );
    assert!((120..=130).contains(&snapshot.oldest_queued_seconds));
    assert_eq!(
        (
            snapshot.intake_failed,
            snapshot.abandoned,
            snapshot.processing_failed,
            snapshot.invalid_output,
            snapshot.retry_exhausted
        ),
        (1, 1, 1, 1, 1)
    );
    sqlx::query("UPDATE media.jobs SET updated_at=statement_timestamp()-interval '15 minutes 1 second' WHERE filename=$1 AND state='failed'").bind(&fixture).execute(&owner).await.unwrap();
    let retained = reader.snapshot().await.unwrap();
    assert_eq!(
        (
            retained.intake_failed,
            retained.abandoned,
            retained.processing_failed,
            retained.invalid_output,
            retained.retry_exhausted
        ),
        (0, 0, 0, 0, 0)
    );
    // Missing policy is unavailable, never fabricated capacity or a zero snapshot.
    owner
        .execute("DELETE FROM media.queue_policy WHERE singleton")
        .await
        .unwrap();
    assert!(reader.snapshot().await.is_err());
    sqlx::query("INSERT INTO media.queue_policy(capacity) VALUES($1)")
        .bind(capacity)
        .execute(&owner)
        .await
        .unwrap();
    let error = owner
        .execute("UPDATE media.queue_policy SET capacity=0")
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    assert_eq!(
        reader.snapshot().await.unwrap().capacity,
        i64::from(capacity)
    );
    let mut locked = owner.begin().await.unwrap();
    sqlx::query("LOCK TABLE media.jobs IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *locked)
        .await
        .unwrap();
    let started = std::time::Instant::now();
    assert!(reader.snapshot().await.is_err());
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "Blocked aggregate exceeded its deadline"
    );
    locked.rollback().await.unwrap();
    assert!(reader.snapshot().await.is_ok());
    for (grant, revoke) in [
        (
            "GRANT SELECT ON media.jobs TO board_monitor",
            "REVOKE SELECT ON media.jobs FROM board_monitor",
        ),
        (
            "GRANT SELECT(filename) ON media.jobs TO board_monitor",
            "REVOKE SELECT(filename) ON media.jobs FROM board_monitor",
        ),
        (
            "GRANT USAGE ON SCHEMA media TO board_monitor",
            "REVOKE USAGE ON SCHEMA media FROM board_monitor",
        ),
        (
            "GRANT CREATE ON SCHEMA monitoring TO board_monitor",
            "REVOKE CREATE ON SCHEMA monitoring FROM board_monitor",
        ),
    ] {
        owner.execute(grant).await.unwrap();
        assert!(
            matches!(
                MonitorReader::connect(&monitor_url).await,
                Err(StoreError::UnsafeRole)
            ),
            "{grant}"
        );
        owner.execute(revoke).await.unwrap();
        MonitorReader::connect(&monitor_url)
            .await
            .unwrap()
            .close()
            .await;
    }
    owner
        .execute("REVOKE SELECT ON monitoring.media_queue FROM board_monitor")
        .await
        .unwrap();
    assert!(reader.snapshot().await.is_err());
    assert!(MonitorReader::connect(&monitor_url).await.is_err());
    owner
        .execute("GRANT SELECT ON monitoring.media_queue TO board_monitor")
        .await
        .unwrap();
    owner
        .execute("REVOKE USAGE ON SCHEMA monitoring FROM board_monitor")
        .await
        .unwrap();
    assert!(reader.snapshot().await.is_err());
    assert!(MonitorReader::connect(&monitor_url).await.is_err());
    owner
        .execute("GRANT USAGE ON SCHEMA monitoring TO board_monitor")
        .await
        .unwrap();
    assert!(reader.snapshot().await.is_ok());
    owner.execute("CREATE OR REPLACE VIEW monitoring.media_queue WITH(security_barrier=true) AS SELECT 0::bigint AS capacity, 0::bigint AS receiving, 0::bigint AS queued, 0::bigint AS processing, 0::bigint AS expired_receiving, 0::bigint AS expired_queued, 0::bigint AS expired_processing, 0::bigint AS oldest_queued_seconds, 0::bigint AS intake_failed, 0::bigint AS abandoned, 0::bigint AS processing_failed, 0::bigint AS invalid_output, 0::bigint AS retry_exhausted").await.unwrap();
    assert!(matches!(
        reader.snapshot().await,
        Err(StoreError::Invalid(_))
    ));
    owner.execute("CREATE OR REPLACE VIEW monitoring.media_queue WITH(security_barrier=true) AS SELECT 1::bigint AS capacity, 0::bigint AS receiving, 0::bigint AS queued, 0::bigint AS processing, 0::bigint AS expired_receiving, 0::bigint AS expired_queued, 0::bigint AS expired_processing, 0::bigint AS oldest_queued_seconds, 0::bigint AS intake_failed, 0::bigint AS abandoned, 0::bigint AS processing_failed, 0::bigint AS invalid_output, 0::bigint AS retry_exhausted UNION ALL SELECT 1,0,0,0,0,0,0,0,0,0,0,0,0").await.unwrap();
    assert!(
        matches!(reader.snapshot().await, Err(StoreError::Invalid(_))),
        "Two individually valid rows are still a malformed aggregate"
    );
    reader.close().await;
    assert!(reader.snapshot().await.is_err());
    monitor.close().await;
}
