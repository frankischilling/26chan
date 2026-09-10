#![cfg(feature = "database-tests")]

use board_store::{StoreError, media_intake::IntakeStore};
use board_store::{media::MediaQueue, media_assets::OutputMetadata};
use sqlx::{Connection, Executor, PgConnection, PgPool};
use std::sync::{Arc, Mutex};
use std::time::Duration;

static DATABASE_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct Fixture {
    store: IntakeStore,
    admin: PgPool,
    queue: MediaQueue,
    ids: Arc<Mutex<Vec<String>>>,
}

impl Fixture {
    async fn reserve(&self) -> board_store::media_intake::IntakeReservation {
        let r = self.store.reserve("synthetic.png").await.unwrap();
        self.ids.lock().unwrap().push(r.id.clone());
        r
    }

    async fn expire(&self, id: &str) {
        sqlx::query("UPDATE media.jobs SET expires_at = clock_timestamp() - interval '1 second' WHERE id = $1")
            .bind(id).execute(&self.admin).await.unwrap();
    }
}

async fn denied(connection: &mut PgConnection, statement: &'static str) {
    let error = connection.execute(statement).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("42501"),
        "{statement}"
    );
}

#[tokio::test]
async fn reservation_requires_read_committed_at_the_sql_boundary() {
    let _serial = DATABASE_TEST_LOCK.lock().await;
    let mut runtime = PgConnection::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
        .await
        .unwrap();
    for begin in [
        "BEGIN ISOLATION LEVEL READ UNCOMMITTED",
        "BEGIN ISOLATION LEVEL REPEATABLE READ",
        "BEGIN ISOLATION LEVEL SERIALIZABLE",
    ] {
        runtime.execute(begin).await.unwrap();
        let result = sqlx::query("SELECT id, capability FROM media_intake.reserve($1)")
            .bind("isolation-synthetic.png")
            .execute(&mut runtime)
            .await;
        // Roll back even when the missing guard unexpectedly admitted the call.
        runtime.execute("ROLLBACK").await.unwrap();
        let error = result.expect_err("Unsupported isolation must reject reservation");
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("22023"),
            "{begin}"
        );
    }
    runtime
        .execute("BEGIN ISOLATION LEVEL READ COMMITTED")
        .await
        .unwrap();
    let result = sqlx::query("SELECT id, capability FROM media_intake.reserve($1)")
        .bind("isolation-synthetic.png")
        .execute(&mut runtime)
        .await;
    runtime.execute("ROLLBACK").await.unwrap();
    assert_eq!(result.unwrap().rows_affected(), 1);
}

#[tokio::test]
async fn capabilities_scope_intake_without_processing_authority() {
    let _serial = DATABASE_TEST_LOCK.lock().await;
    let intake_url = std::env::var("INTAKE_DATABASE_URL").unwrap();
    let store = IntakeStore::connect(&intake_url).await.unwrap();
    let admin = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let ids = Arc::new(Mutex::new(Vec::new()));
    let f = Fixture {
        store,
        admin: admin.clone(),
        queue,
        ids: ids.clone(),
    };
    let result = tokio::spawn(async move {
        let store = &f.store;
        let reservation = f.reserve().await;
        assert!(matches!(
            store.status(&reservation.id, &"0".repeat(64)).await,
            Err(StoreError::NotFound)
        ));
        assert!(matches!(
            store.begin_upload(&reservation.id, &"0".repeat(64)).await,
            Err(StoreError::NotFound)
        ));
        store
            .begin_upload(&reservation.id, &reservation.capability)
            .await
            .unwrap();
        assert!(matches!(
            store
                .begin_upload(&reservation.id, &reservation.capability)
                .await,
            Err(StoreError::Conflict(_))
        ));
        store
            .finish_upload(&reservation.id, &reservation.capability, 32)
            .await
            .unwrap();
        assert_eq!(
            store
                .status(&reservation.id, &reservation.capability)
                .await
                .unwrap()
                .state,
            "queued"
        );
        let mut runtime = PgConnection::connect(&intake_url).await.unwrap();
        let error = runtime
            .execute(
                "SELECT id FROM media.jobs WHERE state = 'queued' FOR UPDATE SKIP LOCKED LIMIT 1",
            )
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501")
        );
        exercise(&f, &mut runtime).await;
        store.close().await.unwrap();
        assert!(matches!(store.ready().await, Err(StoreError::Database(_))));
    })
    .await;
    let owned = ids.lock().unwrap().clone();
    sqlx::query("DELETE FROM media.assets WHERE job_id = ANY($1)")
        .bind(&owned)
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("DELETE FROM media.jobs WHERE id = ANY($1)")
        .bind(&owned)
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    result.unwrap();
}

async fn exercise(f: &Fixture, runtime: &mut PgConnection) {
    let store = &f.store;
    store.ready().await.unwrap();
    assert!(matches!(
        IntakeStore::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap()).await,
        Err(StoreError::UnsafeRole)
    ));
    assert!(matches!(
        IntakeStore::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap()).await,
        Err(StoreError::UnsafeRole)
    ));
    for statement in [
        "SELECT * FROM content.boards",
        "SELECT * FROM post_secrets.deletion",
        "SELECT * FROM staff_identity.accounts",
        "SELECT * FROM deployment.settings",
        "SELECT * FROM media.jobs",
        "SELECT * FROM media.queue_policy",
        "SELECT * FROM media.assets",
        "SELECT * FROM media.approved_assets",
        "SELECT * FROM media_intake.handles",
        "SELECT * FROM monitoring.media_queue",
        "CREATE TABLE media_intake.denied (id integer)",
        "CREATE TABLE public.denied (id integer)",
        "CREATE TEMP TABLE denied (id integer)",
        "CREATE SCHEMA intake_denied",
        "CREATE FUNCTION media_intake.denied() RETURNS integer LANGUAGE sql AS 'SELECT 1'",
        "ALTER FUNCTION media_intake.ready() OWNER TO board_media_intake",
        "SET ROLE board_migrator",
        "SET ROLE board_media",
        "SET ROLE board_media_intake_owner",
    ] {
        denied(runtime, statement).await;
    }
    let mut owner = PgConnection::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    owner
        .execute("SET ROLE board_media_intake_owner")
        .await
        .unwrap();
    let ready: bool = sqlx::query_scalar("SELECT media_intake.ready()")
        .fetch_one(&mut owner)
        .await
        .unwrap();
    assert!(ready);
    for statement in [
        "SELECT filename FROM media.jobs",
        "SELECT lease_token FROM media.jobs",
        "SELECT attempts FROM media.jobs",
        "SELECT output_sha256 FROM media.jobs",
        "SELECT lease_token FROM media.assets",
        "SELECT * FROM content.boards",
        "UPDATE media.jobs SET attempts = 1 WHERE false",
        "UPDATE media.jobs SET lease_token = repeat('0',32) WHERE false",
        "UPDATE media.assets SET state = 'approved' WHERE false",
        "DELETE FROM media.jobs WHERE false",
        "CREATE TABLE media_intake.denied_owner (id integer)",
    ] {
        denied(&mut owner, statement).await;
    }

    // Metadata is validated by the database interface, including UTF-8 byte counts.
    for filename in [
        String::new(),
        "x".repeat(256),
        "é".repeat(128),
        "line\nname".into(),
        "\u{0085}".into(),
    ] {
        assert!(matches!(
            store.reserve(&filename).await,
            Err(StoreError::Invalid(_))
        ));
    }
    let boundary = store.reserve(&"x".repeat(255)).await.unwrap();
    f.ids.lock().unwrap().push(boundary.id.clone());
    assert_eq!(boundary.id.len(), 32);
    assert_eq!(boundary.capability.len(), 64);
    assert!(
        boundary
            .capability
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    let hashed: bool = sqlx::query_scalar("SELECT octet_length(capability_hash) = 32 AND capability_hash = sha256(convert_to($2,'UTF8')) FROM media_intake.handles WHERE job_id = $1")
        .bind(&boundary.id).bind(&boundary.capability).fetch_one(&f.admin).await.unwrap();
    assert!(hashed);
    for bad in [
        String::new(),
        "0".repeat(63),
        "A".repeat(64),
        "0".repeat(65),
        format!("{}\n", "0".repeat(63)),
    ] {
        assert!(matches!(
            store.status(&boundary.id, &bad).await,
            Err(StoreError::NotFound)
        ));
        assert!(matches!(
            store.begin_upload(&boundary.id, &bad).await,
            Err(StoreError::NotFound)
        ));
        assert!(matches!(
            store.finish_upload(&boundary.id, &bad, 32).await,
            Err(StoreError::NotFound)
        ));
        assert!(matches!(
            store.abort_upload(&boundary.id, &bad).await,
            Err(StoreError::NotFound)
        ));
    }
    for bad in [
        String::new(),
        "0".repeat(31),
        "A".repeat(32),
        "0".repeat(33),
    ] {
        assert!(matches!(
            store.status(&bad, &boundary.capability).await,
            Err(StoreError::NotFound)
        ));
    }
    assert!(matches!(
        store.status(&"0".repeat(32), &boundary.capability).await,
        Err(StoreError::NotFound)
    ));
    let missing = sqlx::query("SELECT * FROM media_intake.status($1, NULL)")
        .bind(&boundary.id)
        .execute(&mut *runtime)
        .await
        .unwrap_err();
    assert_eq!(
        missing.as_database_error().unwrap().code().as_deref(),
        Some("P0002")
    );
    let other = f.reserve().await;
    assert_ne!(other.capability, boundary.capability);
    assert!(matches!(
        store.begin_upload(&other.id, &boundary.capability).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store
            .finish_upload(&other.id, &boundary.capability, 32)
            .await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.abort_upload(&other.id, &boundary.capability).await,
        Err(StoreError::NotFound)
    ));
    assert_eq!(
        store
            .status(&other.id, &other.capability)
            .await
            .unwrap()
            .state,
        "receiving"
    );
    assert!(matches!(
        store
            .finish_upload(&boundary.id, &boundary.capability, 32)
            .await,
        Err(StoreError::Conflict(_))
    ));
    let (left, right) = tokio::join!(
        store.begin_upload(&boundary.id, &boundary.capability),
        store.begin_upload(&boundary.id, &boundary.capability)
    );
    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
    assert!(matches!(
        left.err().or(right.err()),
        Some(StoreError::Conflict(_))
    ));
    assert_eq!(
        store
            .status(&boundary.id, &boundary.capability)
            .await
            .unwrap()
            .state,
        "uploading"
    );
    for bytes in [0, 8_388_609, u64::MAX] {
        assert!(matches!(
            store
                .finish_upload(&boundary.id, &boundary.capability, bytes)
                .await,
            Err(StoreError::Invalid(_))
        ));
    }
    let (left, right) = tokio::join!(
        store.finish_upload(&boundary.id, &boundary.capability, 8_388_608),
        store.finish_upload(&boundary.id, &boundary.capability, 8_388_608)
    );
    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
    assert!(matches!(
        left.err().or(right.err()),
        Some(StoreError::Conflict(_))
    ));
    let status = store
        .status(&boundary.id, &boundary.capability)
        .await
        .unwrap();
    assert_eq!(status.input_bytes, Some(8_388_608));
    assert!(status.output_id.is_none());
    assert!(matches!(
        store.abort_upload(&boundary.id, &boundary.capability).await,
        Err(StoreError::Conflict(_))
    ));
    assert!(matches!(
        store.begin_upload(&boundary.id, &boundary.capability).await,
        Err(StoreError::Conflict(_))
    ));
    f.expire(&boundary.id).await;
    assert!(matches!(
        store.status(&boundary.id, &boundary.capability).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.abort_upload(&boundary.id, &boundary.capability).await,
        Err(StoreError::NotFound)
    ));

    store
        .begin_upload(&other.id, &other.capability)
        .await
        .unwrap();
    store
        .abort_upload(&other.id, &other.capability)
        .await
        .unwrap();
    assert_eq!(
        store
            .status(&other.id, &other.capability)
            .await
            .unwrap()
            .state,
        "failed"
    );
    assert!(matches!(
        store.begin_upload(&other.id, &other.capability).await,
        Err(StoreError::Conflict(_))
    ));
    assert!(matches!(
        store.abort_upload(&other.id, &other.capability).await,
        Err(StoreError::Conflict(_))
    ));
    let cancelled: bool = sqlx::query_scalar("SELECT h.upload_started_at IS NOT NULL AND j.failure = 'intake_failed' AND j.attempts = 0 AND j.lease_token IS NULL FROM media.jobs j JOIN media_intake.handles h ON h.job_id = j.id WHERE j.id = $1")
        .bind(&other.id).fetch_one(&f.admin).await.unwrap();
    assert!(cancelled);

    // Holding the job row forces the statement to recheck expiry after its wait.
    for finish in [false, true] {
        let r = f.reserve().await;
        if finish {
            store.begin_upload(&r.id, &r.capability).await.unwrap();
        }
        let mut lock = f.admin.begin().await.unwrap();
        sqlx::query("UPDATE media.jobs SET expires_at = clock_timestamp() + interval '350 milliseconds' WHERE id = $1")
            .bind(&r.id).execute(&mut *lock).await.unwrap();
        let worker = store.clone();
        let id = r.id.clone();
        let capability = r.capability.clone();
        let pending = tokio::spawn(async move {
            if finish {
                worker.finish_upload(&id, &capability, 1).await
            } else {
                worker.begin_upload(&id, &capability).await
            }
        });
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(
            !pending.is_finished(),
            "Mutation must wait for the held job row"
        );
        lock.commit().await.unwrap();
        assert!(matches!(pending.await.unwrap(), Err(StoreError::NotFound)));
        assert!(matches!(
            store.status(&r.id, &r.capability).await,
            Err(StoreError::NotFound)
        ));
    }

    let legacy = f.queue.reserve("operator-owned.png").await.unwrap();
    f.ids.lock().unwrap().push(legacy.id.clone());
    assert!(matches!(
        store.status(&legacy.id, &other.capability).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.begin_upload(&legacy.id, &other.capability).await,
        Err(StoreError::NotFound)
    ));
    f.queue.abort_intake(&legacy.id).await.unwrap();
    let unclaimed = f.reserve().await;
    store
        .abort_upload(&unclaimed.id, &unclaimed.capability)
        .await
        .unwrap();
    assert_eq!(
        store
            .status(&unclaimed.id, &unclaimed.capability)
            .await
            .unwrap()
            .state,
        "failed"
    );

    // Retire only this test's unfinished jobs before the healthy processing control.
    let owned = f.ids.lock().unwrap().clone();
    sqlx::query("UPDATE media.jobs SET state = 'failed', failure = 'abandoned', expires_at = NULL WHERE id = ANY($1) AND state IN ('receiving','queued')")
        .bind(&owned).execute(&f.admin).await.unwrap();
    let r = f.reserve().await;
    store.begin_upload(&r.id, &r.capability).await.unwrap();
    store.finish_upload(&r.id, &r.capability, 1).await.unwrap();
    let job = f.queue.claim().await.unwrap().unwrap();
    assert_eq!(job.id, r.id, "Requires an idle disposable queue");
    assert!(matches!(
        store.abort_upload(&r.id, &r.capability).await,
        Err(StoreError::Conflict(_))
    ));
    assert!(matches!(
        store.begin_upload(&r.id, &r.capability).await,
        Err(StoreError::Conflict(_))
    ));
    assert!(matches!(
        store.finish_upload(&r.id, &r.capability, 1).await,
        Err(StoreError::Conflict(_))
    ));
    let token = job.lease_token.unwrap();
    let asset = f
        .queue
        .prepare_output(
            &r.id,
            &token,
            &OutputMetadata {
                sha256: "a".repeat(64),
                bytes: 100,
                width: 10,
                height: 10,
            },
        )
        .await
        .unwrap();
    assert!(
        store
            .status(&r.id, &r.capability)
            .await
            .unwrap()
            .output_id
            .is_none()
    );
    let error = sqlx::query(
        "UPDATE media.assets SET state = 'approved', approved_at = clock_timestamp() WHERE id = $1",
    )
    .bind(&asset.id)
    .execute(&mut *runtime)
    .await
    .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
    f.queue
        .approve_output(&r.id, &token, &asset.id)
        .await
        .unwrap();
    let status = store.status(&r.id, &r.capability).await.unwrap();
    assert_eq!(status.state, "published");
    assert_eq!(status.output_id.as_deref(), Some(asset.id.as_str()));
    assert!(matches!(
        store.abort_upload(&r.id, &r.capability).await,
        Err(StoreError::Conflict(_))
    ));
    sqlx::query(
        "UPDATE media.jobs SET updated_at = clock_timestamp() - interval '2 days' WHERE id = $1",
    )
    .bind(&r.id)
    .execute(&f.admin)
    .await
    .unwrap();
    assert!(f.queue.forget_terminal(&r.id).await.unwrap());
    assert!(matches!(
        store.status(&r.id, &r.capability).await,
        Err(StoreError::NotFound)
    ));
    let handles: i64 =
        sqlx::query_scalar("SELECT count(*) FROM media_intake.handles WHERE job_id = $1")
            .bind(&r.id)
            .fetch_one(&f.admin)
            .await
            .unwrap();
    assert_eq!(handles, 0);

    // Capacity uses the real singleton policy and includes every unfinished state.
    let capacity: i32 =
        sqlx::query_scalar("SELECT capacity FROM media.queue_policy WHERE singleton")
            .fetch_one(&f.admin)
            .await
            .unwrap();
    let active: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM media.jobs WHERE state IN ('receiving','queued','processing')",
    )
    .fetch_one(&f.admin)
    .await
    .unwrap();
    assert_eq!(active, 0, "Requires an idle disposable queue");
    for _ in 0..capacity - 1 {
        f.reserve().await;
    }
    let (left, right) = tokio::join!(
        store.reserve("capacity-left.png"),
        store.reserve("capacity-right.png")
    );
    let mut winners = 0;
    for result in [left, right] {
        match result {
            Ok(r) => {
                f.ids.lock().unwrap().push(r.id);
                winners += 1;
            }
            Err(StoreError::Conflict(_)) => {}
            _ => panic!("Unexpected capacity result"),
        }
    }
    assert_eq!(winners, 1);
    assert!(matches!(
        store.reserve("full.png").await,
        Err(StoreError::Conflict(_))
    ));
    assert!(matches!(
        f.queue.reserve("full-operator.png").await,
        Err(StoreError::Conflict(_))
    ));

    // Missing execute authority makes both readiness and fresh startup fail closed.
    owner
        .execute(
            "REVOKE EXECUTE ON FUNCTION media_intake.status(text,text) FROM board_media_intake",
        )
        .await
        .unwrap();
    let ready = store.ready().await;
    let startup = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap()).await;
    owner
        .execute("GRANT EXECUTE ON FUNCTION media_intake.status(text,text) TO board_media_intake")
        .await
        .unwrap();
    assert!(matches!(ready, Err(StoreError::UnsafeRole)));
    assert!(matches!(startup, Err(StoreError::UnsafeRole)));
    store.ready().await.unwrap();

    // A changed owner column grant must also prevent startup and readiness.
    sqlx::query("GRANT SELECT(lease_token) ON media.jobs TO board_media_intake_owner")
        .execute(&f.admin)
        .await
        .unwrap();
    let ready = store.ready().await;
    let startup = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap()).await;
    sqlx::query("REVOKE SELECT(lease_token) ON media.jobs FROM board_media_intake_owner")
        .execute(&f.admin)
        .await
        .unwrap();
    assert!(matches!(ready, Err(StoreError::UnsafeRole)));
    assert!(matches!(startup, Err(StoreError::UnsafeRole)));
    store.ready().await.unwrap();
}
