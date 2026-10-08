#![cfg(feature = "database-tests")]

use board_store::{
    StoreError,
    media::MediaQueue,
    media_intake::{IntakeReservation, IntakeStore, PairedInputDescriptor},
};
use sqlx::{Connection, Executor, PgConnection, PgPool};
use std::time::Duration;

fn descriptor(image: u64, replay: Option<u64>) -> PairedInputDescriptor {
    PairedInputDescriptor {
        bytes: image + replay.unwrap_or(0) + 56,
        sha256: "a".repeat(64),
        image_bytes: image,
        image_sha256: "b".repeat(64),
        replay_bytes: replay,
        replay_sha256: replay.map(|_| "c".repeat(64)),
    }
}

struct Fixture {
    store: IntakeStore,
    admin: PgPool,
    media: PgPool,
    queue: MediaQueue,
    ids: Vec<String>,
}
impl Fixture {
    async fn new() -> Self {
        Self {
            store: IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
                .await
                .unwrap(),
            admin: PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
                .await
                .unwrap(),
            media: PgPool::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
                .await
                .unwrap(),
            queue: MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
                .await
                .unwrap(),
            ids: Vec::new(),
        }
    }
    async fn reserve(&mut self) -> IntakeReservation {
        let r = self.store.reserve_pair("drawing.png").await.unwrap();
        self.ids.push(r.id.clone());
        r
    }
    async fn cleanup(self) {
        sqlx::query("DELETE FROM media.assets WHERE job_id = ANY($1)")
            .bind(&self.ids)
            .execute(&self.admin)
            .await
            .unwrap();
        sqlx::query("DELETE FROM media.jobs WHERE id = ANY($1)")
            .bind(&self.ids)
            .execute(&self.admin)
            .await
            .unwrap();
        self.store.close().await.unwrap();
        self.admin.close().await;
        self.media.close().await;
    }
}

async fn rejected(f: &Fixture, id: &str, assignment: &'static str) {
    // Only fixed SQL fragments from this test enter the format string; IDs stay bound.
    let sql = format!("UPDATE media.jobs SET {assignment} WHERE id = $1");
    let result = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
        .bind(id)
        .execute(&f.media)
        .await;
    let err = result.expect_err("Direct media SQL must not bypass paired intake");
    let code = err.as_database_error().and_then(|e| e.code()).unwrap();
    assert!(matches!(code.as_ref(), "42501" | "23514"), "{sql}: {err}");
}

#[tokio::test]
async fn paired_receipts_retry_boundaries_and_legacy_exclusion() {
    let mut f = Fixture::new().await;
    f.store.ready().await.unwrap();
    for replay in [None, Some(1), Some(8_388_608)] {
        let r = f.reserve().await;
        let d = descriptor(8_388_608, replay);
        let pristine: bool = sqlx::query_scalar("SELECT input_kind = 'paired-v2' AND input_bytes IS NULL AND input_sha256 IS NULL AND input_image_bytes IS NULL AND input_image_sha256 IS NULL AND input_replay_bytes IS NULL AND input_replay_sha256 IS NULL FROM media.jobs WHERE id = $1").bind(&r.id).fetch_one(&f.admin).await.unwrap();
        assert!(pristine);
        assert!(matches!(
            f.store.begin_upload(&r.id, &r.capability).await,
            Err(StoreError::Conflict(_))
        ));
        assert!(matches!(
            f.store.finish_upload(&r.id, &r.capability, 1).await,
            Err(StoreError::Conflict(_))
        ));
        assert!(matches!(
            f.store.finish_pair_upload(&r.id, &r.capability, &d).await,
            Err(StoreError::Conflict(_))
        ));
        f.store
            .begin_pair_upload(&r.id, &r.capability)
            .await
            .unwrap();
        assert!(matches!(
            f.store.begin_pair_upload(&r.id, &r.capability).await,
            Err(StoreError::Conflict(_))
        ));
        let (a, b) = tokio::join!(
            f.store.finish_pair_upload(&r.id, &r.capability, &d),
            f.store.finish_pair_upload(&r.id, &r.capability, &d)
        );
        a.unwrap();
        b.unwrap();
        let before: String = sqlx::query_scalar(
            "SELECT expires_at::text || '/' || updated_at::text FROM media.jobs WHERE id = $1",
        )
        .bind(&r.id)
        .fetch_one(&f.admin)
        .await
        .unwrap();
        // Treat the committed first response as unknown and reconcile only by an exact retry.
        f.store
            .finish_pair_upload(&r.id, &r.capability, &d)
            .await
            .unwrap();
        let after: String = sqlx::query_scalar(
            "SELECT expires_at::text || '/' || updated_at::text FROM media.jobs WHERE id = $1",
        )
        .bind(&r.id)
        .fetch_one(&f.admin)
        .await
        .unwrap();
        assert_eq!(
            before, after,
            "Exact retry must not extend expiry or rewrite receipt"
        );
        let exact: bool = sqlx::query_scalar("SELECT input_bytes = $2 AND input_sha256 = $3 AND input_image_bytes = $4 AND input_image_sha256 = $5 AND input_replay_bytes IS NOT DISTINCT FROM $6 AND input_replay_sha256 IS NOT DISTINCT FROM $7 FROM media.jobs WHERE id = $1")
            .bind(&r.id).bind(d.bytes as i64).bind(&d.sha256).bind(d.image_bytes as i64).bind(&d.image_sha256).bind(d.replay_bytes.map(|n| n as i64)).bind(&d.replay_sha256).fetch_one(&f.admin).await.unwrap();
        assert!(exact);
        let mut changed = descriptor(8_388_608, replay);
        changed.sha256 = "d".repeat(64);
        assert!(matches!(
            f.store
                .finish_pair_upload(&r.id, &r.capability, &changed)
                .await,
            Err(StoreError::Conflict(_))
        ));
        assert_eq!(
            f.store.status(&r.id, &r.capability).await.unwrap().state,
            "queued"
        );
        assert!(
            f.queue.claim().await.unwrap().is_none(),
            "Legacy claim must skip paired receipts"
        );
    }
    let legacy = f.store.reserve("legacy.png").await.unwrap();
    f.ids.push(legacy.id.clone());
    assert!(matches!(
        f.store
            .begin_pair_upload(&legacy.id, &legacy.capability)
            .await,
        Err(StoreError::Conflict(_))
    ));
    f.store
        .begin_upload(&legacy.id, &legacy.capability)
        .await
        .unwrap();
    assert!(matches!(
        f.store
            .finish_pair_upload(&legacy.id, &legacy.capability, &descriptor(1, None))
            .await,
        Err(StoreError::Conflict(_))
    ));
    f.store
        .finish_upload(&legacy.id, &legacy.capability, 8_388_608)
        .await
        .unwrap();
    let claimed = f.queue.claim().await.unwrap().unwrap();
    assert_eq!(claimed.id, legacy.id);
    let legacy_shape: bool = sqlx::query_scalar("SELECT input_kind = 'image-v1' AND input_sha256 IS NULL AND input_image_bytes IS NULL AND input_image_sha256 IS NULL AND input_replay_bytes IS NULL AND input_replay_sha256 IS NULL FROM media.jobs WHERE id = $1").bind(&legacy.id).fetch_one(&f.admin).await.unwrap();
    assert!(legacy_shape);
    f.cleanup().await;
}

#[tokio::test]
async fn paired_authentication_precedes_metadata_and_rejects_partial_descriptors() {
    let mut f = Fixture::new().await;
    let r = f.reserve().await;
    f.store
        .begin_pair_upload(&r.id, &r.capability)
        .await
        .unwrap();
    let mut invalid = vec![
        descriptor(0, None),
        descriptor(8_388_609, None),
        descriptor(1, Some(0)),
        descriptor(1, Some(8_388_609)),
    ];
    let mut d = descriptor(1, None);
    d.bytes = u64::MAX;
    invalid.push(d);
    let mut d = descriptor(1, None);
    d.image_bytes = u64::MAX;
    invalid.push(d);
    let mut d = descriptor(1, Some(1));
    d.replay_bytes = Some(u64::MAX);
    invalid.push(d);
    let mut d = descriptor(1, None);
    d.bytes += 1;
    invalid.push(d);
    for hash in ["", "A", "\n", "\0", "z"] {
        let mut d = descriptor(1, None);
        d.sha256 = hash.repeat(64);
        invalid.push(d);
        let mut d = descriptor(1, None);
        d.image_sha256 = hash.repeat(64);
        invalid.push(d);
        let mut d = descriptor(1, Some(1));
        d.replay_sha256 = Some(hash.repeat(64));
        invalid.push(d);
    }
    let mut d = descriptor(1, None);
    d.replay_sha256 = Some("c".repeat(64));
    invalid.push(d);
    let mut d = descriptor(1, Some(1));
    d.replay_sha256 = None;
    invalid.push(d);
    for d in invalid {
        assert!(matches!(
            f.store.finish_pair_upload(&r.id, &"0".repeat(64), &d).await,
            Err(StoreError::NotFound)
        ));
        assert!(matches!(
            f.store.finish_pair_upload(&r.id, &r.capability, &d).await,
            Err(StoreError::Invalid(_))
        ));
    }
    let mut runtime = PgConnection::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
        .await
        .unwrap();
    for cap in [None, Some("wrong")] {
        let error = sqlx::query(
            "SELECT media_intake.finish_pair_upload($1,$2,NULL,NULL,NULL,NULL,NULL,NULL)",
        )
        .bind(&r.id)
        .bind(cap)
        .execute(&mut runtime)
        .await
        .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("P0002")
        );
    }
    for fields in [
        "NULL,NULL,NULL,NULL,NULL,NULL",
        "57,NULL,1,repeat('b',64),NULL,NULL",
        "57,repeat('a',64),NULL,repeat('b',64),NULL,NULL",
        "57,repeat('a',64),1,NULL,NULL,NULL",
        "58,repeat('a',64),1,repeat('b',64),1,NULL",
        "57,repeat('a',64),1,repeat('b',64),NULL,repeat('c',64)",
    ] {
        let sql = format!("SELECT media_intake.finish_pair_upload($1,$2,{fields})");
        let error = sqlx::query(sqlx::AssertSqlSafe(sql.as_str()))
            .bind(&r.id)
            .bind(&r.capability)
            .execute(&mut runtime)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("22023")
        );
    }
    f.store.abort_upload(&r.id, &r.capability).await.unwrap();
    assert_eq!(
        f.store.status(&r.id, &r.capability).await.unwrap().state,
        "failed"
    );
    assert!(matches!(
        f.store
            .finish_pair_upload(&r.id, &r.capability, &descriptor(1, None))
            .await,
        Err(StoreError::Conflict(_))
    ));
    f.cleanup().await;
}

#[tokio::test]
async fn paired_direct_sql_cannot_forge_promote_or_mutate_receipts() {
    let mut f = Fixture::new().await;
    let r = f.reserve().await;
    for assignment in [
        "input_kind = 'image-v1'",
        "input_sha256 = repeat('a',64)",
        "input_bytes = 57",
        "input_image_bytes = 1",
        "input_image_sha256 = repeat('b',64)",
        "input_replay_bytes = 1",
        "input_replay_sha256 = repeat('c',64)",
        "state = 'queued', input_bytes = 57, input_sha256 = repeat('a',64), input_image_bytes = 1, input_image_sha256 = repeat('b',64)",
    ] {
        rejected(&f, &r.id, assignment).await;
    }
    let error = sqlx::query("INSERT INTO media.jobs(id,filename,input_kind,expires_at) VALUES (replace(gen_random_uuid()::text,'-',''),'forged.png','paired-v2',clock_timestamp()+interval '5 minutes')").execute(&f.media).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
    let legacy = f.queue.reserve("legacy.png").await.unwrap();
    f.ids.push(legacy.id.clone());
    rejected(&f, &legacy.id, "input_kind = 'paired-v2'").await;
    rejected(&f, &legacy.id, "input_sha256 = repeat('a',64)").await;
    f.store
        .begin_pair_upload(&r.id, &r.capability)
        .await
        .unwrap();
    f.store
        .finish_pair_upload(&r.id, &r.capability, &descriptor(1, Some(1)))
        .await
        .unwrap();
    for assignment in [
        "input_bytes = 59, input_image_bytes = 2",
        "input_sha256 = repeat('d',64)",
        "input_image_sha256 = repeat('d',64)",
        "input_replay_sha256 = repeat('d',64)",
        "input_replay_bytes = NULL, input_replay_sha256 = NULL, input_bytes = 57",
        "state = 'processing', attempts = 1, lease_token = repeat('a',32), expires_at = clock_timestamp()+interval '30 seconds'",
        "state = 'published', attempts = 1, lease_token = repeat('a',32), output_bytes = 1, output_sha256 = repeat('a',64), expires_at = NULL",
    ] {
        rejected(&f, &r.id, assignment).await;
    }
    for state in ["pending", "approved"] {
        let error = sqlx::query("INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at) VALUES (replace(gen_random_uuid()::text,'-',''),$1,repeat('a',32),repeat('a',64),1,1,1,$2,CASE WHEN $2 = 'approved' THEN clock_timestamp() END)").bind(&r.id).bind(state).execute(&f.media).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501")
        );
    }
    // Expiry is a supported terminal transition; it must retain the actual-byte receipt.
    sqlx::query(
        "UPDATE media.jobs SET expires_at = clock_timestamp()-interval '1 second' WHERE id = $1",
    )
    .bind(&r.id)
    .execute(&f.admin)
    .await
    .unwrap();
    assert!(matches!(
        f.store
            .finish_pair_upload(&r.id, &r.capability, &descriptor(1, Some(1)))
            .await,
        Err(StoreError::NotFound)
    ));
    assert_eq!(f.queue.expire().await.unwrap(), 1);
    let retained: bool = sqlx::query_scalar("SELECT state = 'failed' AND input_bytes = 58 AND input_image_bytes = 1 AND input_replay_bytes = 1 FROM media.jobs WHERE id = $1").bind(&r.id).fetch_one(&f.admin).await.unwrap();
    assert!(retained);
    rejected(
        &f,
        &r.id,
        "state = 'queued', failure = NULL, expires_at = clock_timestamp()+interval '1 hour'",
    )
    .await;
    f.cleanup().await;
}

#[tokio::test]
async fn paired_waiters_recheck_expiry_after_job_and_handle_locks() {
    let mut f = Fixture::new().await;
    for finish in [false, true] {
        for handle_lock in [false, true] {
            let r = f.reserve().await;
            if finish {
                f.store
                    .begin_pair_upload(&r.id, &r.capability)
                    .await
                    .unwrap();
            }
            if handle_lock {
                sqlx::query("UPDATE media.jobs SET expires_at = clock_timestamp()+interval '1 second' WHERE id = $1").bind(&r.id).execute(&f.admin).await.unwrap();
            }
            let mut tx = f.admin.begin().await.unwrap();
            let sql = if handle_lock {
                "SELECT job_id FROM media_intake.handles WHERE job_id = $1 FOR UPDATE"
            } else {
                "SELECT id FROM media.jobs WHERE id = $1 FOR UPDATE"
            };
            sqlx::query(sql)
                .bind(&r.id)
                .execute(&mut *tx)
                .await
                .unwrap();
            let xid: String = sqlx::query_scalar("SELECT txid_current()::text")
                .fetch_one(&mut *tx)
                .await
                .unwrap();
            let store = f.store.clone();
            let id = r.id.clone();
            let cap = r.capability.clone();
            let pending = tokio::spawn(async move {
                if finish {
                    store
                        .finish_pair_upload(&id, &cap, &descriptor(1, None))
                        .await
                } else {
                    store.begin_pair_upload(&id, &cap).await
                }
            });
            tokio::time::timeout(Duration::from_secs(3),async {
                loop {
                    let blocked: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_locks WHERE locktype = 'transactionid' AND transactionid = $1::xid AND NOT granted)").bind(&xid).fetch_one(&f.admin).await.unwrap();
                    if blocked { break; }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }).await.expect("Paired mutation must wait for the held row");
            assert!(!pending.is_finished());
            if handle_lock {
                sqlx::query("SELECT pg_sleep(1.1)")
                    .execute(&mut *tx)
                    .await
                    .unwrap();
            } else {
                sqlx::query("UPDATE media.jobs SET expires_at = clock_timestamp()-interval '1 second' WHERE id = $1").bind(&r.id).execute(&mut *tx).await.unwrap();
            }
            tx.commit().await.unwrap();
            let result = pending.await.unwrap();
            assert!(
                matches!(result, Err(StoreError::NotFound)),
                "Expiry must be checked after all waits: {result:?}"
            );
        }
    }
    f.cleanup().await;
}

#[tokio::test]
async fn paired_function_privileges_and_readiness_are_exact() {
    let f = Fixture::new().await;
    let mut intake = PgConnection::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
        .await
        .unwrap();
    for begin in [
        "BEGIN ISOLATION LEVEL REPEATABLE READ",
        "BEGIN ISOLATION LEVEL SERIALIZABLE",
        "BEGIN ISOLATION LEVEL READ UNCOMMITTED",
    ] {
        intake.execute(begin).await.unwrap();
        let result = intake
            .execute("SELECT * FROM media_intake.reserve_pair('drawing.png')")
            .await;
        intake.execute("ROLLBACK").await.unwrap();
        assert_eq!(
            result
                .unwrap_err()
                .as_database_error()
                .unwrap()
                .code()
                .as_deref(),
            Some("22023")
        );
    }
    let mut owner = PgConnection::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    owner
        .execute("SET ROLE board_media_intake_owner")
        .await
        .unwrap();
    for signature in [
        "reserve_pair(text)",
        "begin_pair_upload(text,text)",
        "finish_pair_upload(text,text,bigint,text,bigint,text,bigint,text)",
    ] {
        let sql =
            format!("REVOKE EXECUTE ON FUNCTION media_intake.{signature} FROM board_media_intake");
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .execute(&mut owner)
            .await
            .unwrap();
        let ready = f.store.ready().await;
        let startup = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap()).await;
        let sql =
            format!("GRANT EXECUTE ON FUNCTION media_intake.{signature} TO board_media_intake");
        sqlx::query(sqlx::AssertSqlSafe(sql))
            .execute(&mut owner)
            .await
            .unwrap();
        assert!(matches!(ready, Err(StoreError::UnsafeRole)));
        assert!(matches!(startup, Err(StoreError::UnsafeRole)));
        f.store.ready().await.unwrap();
    }
    for statement in [
        "SELECT input_kind FROM media.jobs",
        "UPDATE media.jobs SET input_kind='paired-v2' WHERE false",
        "SELECT * FROM media_intake.handles",
        "SET ROLE board_media_intake_owner",
    ] {
        let error = intake.execute(statement).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501")
        );
    }
    f.cleanup().await;
}

#[tokio::test]
async fn paired_committed_receipt_survives_cancelled_sql_response() {
    let mut f = Fixture::new().await;
    let r = f.reserve().await;
    f.store
        .begin_pair_upload(&r.id, &r.capability)
        .await
        .unwrap();
    let mut connection = PgConnection::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
        .await
        .unwrap();
    // The transaction commits before the final statement completes. Drop the
    // waiting client's future only after another session witnesses that commit.
    // No SQL outcome is returned to the caller being reconciled.
    let sql = format!(
        "BEGIN; SELECT media_intake.finish_pair_upload('{}','{}',58,repeat('a',64),1,repeat('b',64),1,repeat('c',64)); COMMIT; SELECT pg_sleep(4)",
        r.id, r.capability
    );
    let pending = tokio::spawn(async move {
        sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
            .execute(&mut connection)
            .await
    });
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let queued: bool =
                sqlx::query_scalar("SELECT state = 'queued' FROM media.jobs WHERE id = $1")
                    .bind(&r.id)
                    .fetch_one(&f.admin)
                    .await
                    .unwrap();
            if queued {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("Independent session must witness the committed descriptor");
    assert!(!pending.is_finished());
    pending.abort();
    assert!(pending.await.unwrap_err().is_cancelled());
    f.store
        .finish_pair_upload(&r.id, &r.capability, &descriptor(1, Some(1)))
        .await
        .unwrap();
    assert_eq!(
        f.store.status(&r.id, &r.capability).await.unwrap().state,
        "queued"
    );
    let mut changed = descriptor(1, Some(1));
    changed.image_sha256 = "d".repeat(64);
    assert!(matches!(
        f.store
            .finish_pair_upload(&r.id, &r.capability, &changed)
            .await,
        Err(StoreError::Conflict(_))
    ));
    f.cleanup().await;
}
