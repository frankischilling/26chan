#![cfg(feature = "database-tests")]

use board_store::{
    StoreError,
    media::{Failure, MediaQueue},
    media_assets::{MediaReader, OutputMetadata},
};
use sqlx::{Connection, Executor, PgConnection, PgPool};
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn durable_approval_schema_exists() {
    let admin = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let installed: bool = sqlx::query_scalar(
        "SELECT to_regclass('media.assets') IS NOT NULL AND to_regclass('media.approved_assets') IS NOT NULL",
    )
    .fetch_one(&admin)
    .await
    .unwrap();
    let barrier: bool = sqlx::query_scalar("SELECT 'security_barrier=true' = ANY(reloptions) FROM pg_class WHERE oid = 'media.approved_assets'::regclass")
        .fetch_one(&admin).await.unwrap();
    assert!(barrier);
    let columns: Vec<String> = sqlx::query_scalar("SELECT attname::text FROM pg_attribute WHERE attrelid = 'media.approved_assets'::regclass AND attnum > 0 AND NOT attisdropped ORDER BY attnum")
        .fetch_all(&admin).await.unwrap();
    assert_eq!(columns, ["id", "sha256", "bytes", "width", "height"]);
    let installed_migration: bool =
        sqlx::query_scalar("SELECT success FROM public._sqlx_migrations WHERE version = 8")
            .fetch_one(&admin)
            .await
            .unwrap();
    assert!(installed_migration);
    admin.close().await;
    assert!(
        installed,
        "durable approval table and view must be installed"
    );
}

fn metadata() -> OutputMetadata {
    OutputMetadata {
        sha256: "a".repeat(64),
        bytes: 100,
        width: 10,
        height: 20,
    }
}

// Owned IDs remain available to the outer task even if the exercise panics.
struct Fixture {
    queue: MediaQueue,
    admin: PgPool,
    owned: Arc<Mutex<Vec<String>>>,
    outputs: Arc<Mutex<Vec<String>>>,
}

impl Fixture {
    async fn claim(&self) -> (String, String) {
        let job = self.queue.reserve("approval-test").await.unwrap();
        self.owned.lock().unwrap().push(job.id.clone());
        self.queue.queue(&job.id, 1).await.unwrap();
        let claimed = self.queue.claim().await.unwrap().unwrap();
        assert_eq!(claimed.id, job.id, "Use an idle disposable media queue");
        (job.id, claimed.lease_token.unwrap())
    }

    async fn expire(&self, id: &str) {
        sqlx::query("UPDATE media.jobs SET expires_at = clock_timestamp() - interval '1 second' WHERE id = $1")
            .bind(id).execute(&self.admin).await.unwrap();
    }
}

#[tokio::test]
async fn approval_fences_leases_survives_job_cleanup_and_reconciles_abandoned_outputs() {
    let admin = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let owned = Arc::new(Mutex::new(Vec::new()));
    let outputs = Arc::new(Mutex::new(Vec::new()));
    let fixture = Fixture {
        queue,
        admin: admin.clone(),
        owned: owned.clone(),
        outputs: outputs.clone(),
    };
    let result = tokio::spawn(async move { exercise_approval(fixture).await }).await;
    let ids = owned.lock().unwrap().clone();
    let output_ids = outputs.lock().unwrap().clone();
    sqlx::query("DELETE FROM media.assets WHERE job_id = ANY($1) OR id = ANY($1) OR id = ANY($2)")
        .bind(&ids)
        .bind(&output_ids)
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("DELETE FROM media.jobs WHERE id = ANY($1)")
        .bind(&ids)
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    result.unwrap();
}

async fn exercise_approval(f: Fixture) {
    let reader = MediaReader::connect(&std::env::var("MEDIA_READ_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut writer = PgConnection::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let (job, token) = f.claim().await;
    let meta = metadata();

    // Invalid wire values fail before any database reservation can be created.
    for bad in [
        "A".repeat(32),
        "0".repeat(31),
        "0".repeat(33),
        format!("{}\n", "0".repeat(31)),
    ] {
        assert!(matches!(
            f.queue.prepare_output(&bad, &token, &meta).await,
            Err(StoreError::Invalid(_))
        ));
        assert!(matches!(
            f.queue.prepare_output(&job, &bad, &meta).await,
            Err(StoreError::Invalid(_))
        ));
        assert!(matches!(
            f.queue.approve_output(&job, &token, &bad).await,
            Err(StoreError::Invalid(_))
        ));
        assert!(matches!(
            reader.get(&bad).await,
            Err(StoreError::Invalid(_))
        ));
        assert!(matches!(
            f.queue.begin_output_deletion(&bad).await,
            Err(StoreError::Invalid(_))
        ));
        assert!(matches!(
            f.queue.forget_output(&bad).await,
            Err(StoreError::Invalid(_))
        ));
    }
    for invalid in [
        OutputMetadata {
            sha256: "A".repeat(64),
            ..metadata()
        },
        OutputMetadata {
            sha256: "a".repeat(63),
            ..metadata()
        },
        OutputMetadata {
            sha256: "a".repeat(65),
            ..metadata()
        },
        OutputMetadata {
            bytes: 0,
            ..metadata()
        },
        OutputMetadata {
            bytes: 5_242_881,
            ..metadata()
        },
        OutputMetadata {
            width: 0,
            ..metadata()
        },
        OutputMetadata {
            width: 1025,
            ..metadata()
        },
        OutputMetadata {
            height: 0,
            ..metadata()
        },
        OutputMetadata {
            height: 1025,
            ..metadata()
        },
    ] {
        assert!(matches!(
            f.queue.prepare_output(&job, &token, &invalid).await,
            Err(StoreError::Invalid(_))
        ));
        let error = sqlx::query("INSERT INTO media.assets (id, job_id, lease_token, sha256, bytes, width, height) VALUES (replace(gen_random_uuid()::text, '-', ''), $1, $2, $3, $4, $5, $6)")
            .bind(&job).bind(&token).bind(&invalid.sha256).bind(invalid.bytes).bind(invalid.width).bind(invalid.height)
            .execute(&mut writer).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
    }
    for column in ["id", "job_id", "lease_token"] {
        for bad in [
            "A".repeat(32),
            "0".repeat(31),
            "0".repeat(33),
            format!("{}\n", "0".repeat(32)),
        ] {
            let error = sqlx::query("INSERT INTO media.assets (id, job_id, lease_token, sha256, bytes, width, height) VALUES ($1, $2, $3, $4, 1, 1, 1)")
                .bind(if column == "id" { &bad } else { &job })
                .bind(if column == "job_id" { &bad } else { &job })
                .bind(if column == "lease_token" { &bad } else { &token })
                .bind(&meta.sha256)
                .execute(&mut writer)
                .await
                .unwrap_err();
            assert_eq!(
                error.as_database_error().unwrap().code().as_deref(),
                Some("23514")
            );
        }
    }

    let asset = f.queue.prepare_output(&job, &token, &meta).await.unwrap();
    f.outputs.lock().unwrap().push(asset.id.clone());
    assert_ne!(asset.id, job);
    assert_ne!(asset.id, token);
    assert_eq!(
        (
            asset.sha256.as_str(),
            asset.bytes,
            asset.width,
            asset.height
        ),
        (meta.sha256.as_str(), 100, 10, 20)
    );
    assert_eq!(
        f.queue.prepare_output(&job, &token, &meta).await.unwrap(),
        asset
    );
    assert!(matches!(
        reader.get(&asset.id).await,
        Err(StoreError::NotFound)
    ));
    assert!(
        !f.queue
            .output_cleanup_candidates()
            .await
            .unwrap()
            .contains(&asset.id)
    );
    assert!(!f.queue.begin_output_deletion(&asset.id).await.unwrap());
    assert!(!f.queue.forget_output(&asset.id).await.unwrap());
    for changed in [
        OutputMetadata {
            sha256: "b".repeat(64),
            ..metadata()
        },
        OutputMetadata {
            bytes: 101,
            ..metadata()
        },
        OutputMetadata {
            width: 11,
            ..metadata()
        },
        OutputMetadata {
            height: 21,
            ..metadata()
        },
    ] {
        assert!(
            f.queue
                .prepare_output(&job, &token, &changed)
                .await
                .is_err()
        );
    }
    for query in [
        "UPDATE media.assets SET id = repeat('f', 32) WHERE id = $1 RETURNING id",
        "UPDATE media.assets SET job_id = repeat('f', 32) WHERE id = $1 RETURNING id",
        "UPDATE media.assets SET lease_token = repeat('f', 32) WHERE id = $1 RETURNING id",
        "UPDATE media.assets SET sha256 = repeat('b', 64) WHERE id = $1 RETURNING id",
        "UPDATE media.assets SET bytes = 101 WHERE id = $1 RETURNING id",
        "UPDATE media.assets SET width = 11 WHERE id = $1 RETURNING id",
        "UPDATE media.assets SET height = 21 WHERE id = $1 RETURNING id",
    ] {
        let mutation: Result<Option<String>, _> = sqlx::query_scalar(query)
            .bind(&asset.id)
            .fetch_optional(&mut writer)
            .await;
        if let Ok(Some(id)) = &mutation {
            f.outputs.lock().unwrap().push(id.clone());
        }
        assert!(mutation.is_err());
    }
    assert!(
        f.queue
            .approve_output(&job, &"f".repeat(32), &asset.id)
            .await
            .is_err()
    );
    assert!(
        f.queue
            .approve_output(&"f".repeat(32), &token, &asset.id)
            .await
            .is_err()
    );
    assert!(
        f.queue
            .approve_output(&job, &token, &"f".repeat(32))
            .await
            .is_err()
    );

    // Both approvals must return one durable record, with one job receipt.
    let (a, b) = tokio::join!(
        f.queue.approve_output(&job, &token, &asset.id),
        f.queue.approve_output(&job, &token, &asset.id)
    );
    assert_eq!(a.unwrap(), asset);
    assert_eq!(b.unwrap(), asset);
    assert_eq!(reader.get(&asset.id).await.unwrap(), asset);
    let completed = f.queue.get(&job).await.unwrap();
    assert_eq!(completed.state, "published");
    assert_eq!(
        completed.output_sha256.as_deref(),
        Some(meta.sha256.as_str())
    );
    assert_eq!(completed.output_bytes, Some(100));
    assert!(!f.queue.begin_output_deletion(&asset.id).await.unwrap());
    assert!(!f.queue.forget_output(&asset.id).await.unwrap());
    for query in [
        "UPDATE media.assets SET state = 'deleting' WHERE id = $1",
        "UPDATE media.assets SET updated_at = clock_timestamp() WHERE id = $1",
        "DELETE FROM media.assets WHERE id = $1",
    ] {
        assert!(
            sqlx::query(query)
                .bind(&asset.id)
                .execute(&mut writer)
                .await
                .is_err()
        );
    }
    sqlx::query(
        "UPDATE media.jobs SET updated_at = clock_timestamp() - interval '2 days' WHERE id = $1",
    )
    .bind(&job)
    .execute(&f.admin)
    .await
    .unwrap();
    assert!(
        f.queue
            .cleanup_candidates()
            .await
            .unwrap()
            .iter()
            .any(|j| j.id == job)
    );
    assert!(f.queue.forget_terminal(&job).await.unwrap());
    assert_eq!(reader.get(&asset.id).await.unwrap(), asset);
    assert_eq!(
        f.queue.prepare_output(&job, &token, &meta).await.unwrap(),
        asset
    );
    assert_eq!(
        f.queue
            .approve_output(&job, &token, &asset.id)
            .await
            .unwrap(),
        asset
    );
    assert!(
        f.queue
            .prepare_output(
                &job,
                &token,
                &OutputMetadata {
                    bytes: 101,
                    ..metadata()
                }
            )
            .await
            .is_err()
    );

    // A reservation from an expired lease never follows its job into a new lease.
    let (job, old_token) = f.claim().await;
    let old = f
        .queue
        .prepare_output(&job, &old_token, &meta)
        .await
        .unwrap();
    f.expire(&job).await;
    assert!(
        f.queue
            .prepare_output(&job, &old_token, &meta)
            .await
            .is_err()
    );
    assert!(
        f.queue
            .approve_output(&job, &old_token, &old.id)
            .await
            .is_err()
    );
    assert!(
        f.queue
            .complete(&job, &old_token, &meta.sha256, 100)
            .await
            .is_err()
    );
    assert!(
        f.queue
            .output_cleanup_candidates()
            .await
            .unwrap()
            .contains(&old.id)
    );
    f.queue.expire().await.unwrap();
    let renewed = f.queue.claim().await.unwrap().unwrap();
    assert_eq!(renewed.id, job);
    let token = renewed.lease_token.unwrap();
    let new = f.queue.prepare_output(&job, &token, &meta).await.unwrap();
    assert_ne!(old.id, new.id);
    assert!(
        f.queue
            .approve_output(&job, &old_token, &old.id)
            .await
            .is_err()
    );
    assert!(
        f.queue
            .complete(&job, &old_token, &meta.sha256, 100)
            .await
            .is_err()
    );
    assert!(f.queue.begin_output_deletion(&old.id).await.unwrap());
    assert!(f.queue.begin_output_deletion(&old.id).await.unwrap());
    assert!(matches!(
        reader.get(&old.id).await,
        Err(StoreError::NotFound)
    ));
    assert!(
        f.queue
            .output_cleanup_candidates()
            .await
            .unwrap()
            .contains(&old.id)
    );
    assert!(
        f.queue
            .prepare_output(&job, &old_token, &meta)
            .await
            .is_err()
    );
    assert!(f.queue.forget_output(&old.id).await.unwrap());
    assert!(!f.queue.forget_output(&old.id).await.unwrap());
    assert!(!f.queue.begin_output_deletion(&old.id).await.unwrap());
    assert!(!f.queue.begin_output_deletion(&new.id).await.unwrap());
    f.queue
        .fail(&job, &token, Failure::Processing, false)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE media.jobs SET updated_at = clock_timestamp() - interval '2 days' WHERE id = $1",
    )
    .bind(&job)
    .execute(&f.admin)
    .await
    .unwrap();
    assert!(f.queue.forget_terminal(&job).await.unwrap());
    assert!(f.queue.begin_output_deletion(&new.id).await.unwrap());
    assert!(f.queue.forget_output(&new.id).await.unwrap());

    let (job, token) = f.claim().await;
    f.queue
        .complete(&job, &token, &meta.sha256, 100)
        .await
        .unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM media.assets WHERE job_id = $1")
        .bind(&job)
        .fetch_one(&f.admin)
        .await
        .unwrap();
    assert_eq!(count, 0, "Legacy completion cannot create approval");
    assert!(f.queue.prepare_output(&job, &token, &meta).await.is_err());

    // Start the operation while its lease is valid, then expire it during a row-lock wait.
    // Using a transaction timestamp for the final check would incorrectly permit publication.
    for approve in [false, true] {
        let (job, token) = f.claim().await;
        let asset = f.queue.prepare_output(&job, &token, &meta).await.unwrap();
        let mut blocker = f.admin.begin().await.unwrap();
        sqlx::query("UPDATE media.jobs SET expires_at = clock_timestamp() + interval '1 second' WHERE id = $1")
            .bind(&job).execute(&mut *blocker).await.unwrap();
        let waiting_queue = f.queue.clone();
        let waiting_job = job.clone();
        let waiting_asset = asset.id.clone();
        let waiting = tokio::spawn(async move {
            if approve {
                waiting_queue
                    .approve_output(&waiting_job, &token, &waiting_asset)
                    .await
            } else {
                waiting_queue
                    .prepare_output(&waiting_job, &token, &metadata())
                    .await
            }
        });
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        blocker.commit().await.unwrap();
        assert!(matches!(
            waiting.await.unwrap(),
            Err(StoreError::Conflict(_))
        ));
        assert_eq!(f.queue.get(&job).await.unwrap().state, "processing");
        assert!(matches!(
            reader.get(&asset.id).await,
            Err(StoreError::NotFound)
        ));
        assert!(f.queue.begin_output_deletion(&asset.id).await.unwrap());
        assert!(f.queue.forget_output(&asset.id).await.unwrap());
        // Keep the shared queue idle for the next fixture claim.
        sqlx::query("UPDATE media.jobs SET state = 'failed', failure = 'abandoned', lease_token = NULL, expires_at = NULL WHERE id = $1")
            .bind(&job).execute(&f.admin).await.unwrap();
    }

    // More than a batch of abandoned records is drained in stable order.
    let (job, token) = f.claim().await;
    let ids: Vec<String> = sqlx::query_scalar("INSERT INTO media.assets (id, job_id, lease_token, sha256, bytes, width, height) SELECT replace(gen_random_uuid()::text, '-', ''), $1, replace(gen_random_uuid()::text, '-', ''), repeat('c', 64), 5242880, 1024, 1024 FROM generate_series(1, 65) RETURNING id")
        .bind(&job).fetch_all(&f.admin).await.unwrap();
    let expected: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM media.assets WHERE id = ANY($1) ORDER BY created_at, id LIMIT 64",
    )
    .bind(&ids)
    .fetch_all(&f.admin)
    .await
    .unwrap();
    let candidates = f.queue.output_cleanup_candidates().await.unwrap();
    assert_eq!(candidates, expected);
    for id in candidates {
        assert!(f.queue.begin_output_deletion(&id).await.unwrap());
        assert!(f.queue.forget_output(&id).await.unwrap());
    }
    assert_eq!(f.queue.output_cleanup_candidates().await.unwrap().len(), 1);
    let minimum = OutputMetadata {
        bytes: 1,
        width: 1,
        height: 1,
        ..metadata()
    };
    let minimum_asset = f
        .queue
        .prepare_output(&job, &token, &minimum)
        .await
        .unwrap();
    assert_eq!(minimum_asset.bytes, 1);
    f.queue
        .approve_output(&job, &token, &minimum_asset.id)
        .await
        .unwrap();
}

#[tokio::test]
async fn reader_credentials_expose_only_approved_metadata() {
    let url = std::env::var("MEDIA_READ_DATABASE_URL").unwrap();
    let reader = MediaReader::connect(&url).await.unwrap();
    assert!(matches!(
        reader.get(&"0".repeat(32)).await,
        Err(StoreError::NotFound)
    ));
    let mut read = PgConnection::connect(&url).await.unwrap();
    read.execute("SELECT id, sha256, bytes, width, height FROM media.approved_assets LIMIT 1")
        .await
        .unwrap();
    for query in [
        "SELECT * FROM media.assets",
        "SELECT * FROM media.jobs",
        "SELECT lease_token FROM media.assets",
        "SELECT * FROM media.queue_policy",
        "SELECT * FROM content.posts",
        "SELECT * FROM post_secrets.deletion",
        "SELECT * FROM staff_identity.accounts",
        "SELECT * FROM deployment.settings",
        "UPDATE media.approved_assets SET bytes = 1",
        "DELETE FROM media.approved_assets",
        "INSERT INTO media.approved_assets VALUES (repeat('0', 32), repeat('a', 64), 1, 1, 1)",
        "UPDATE media.assets SET state = 'approved'",
        "DELETE FROM media.assets",
        "INSERT INTO media.assets (id) VALUES (repeat('0', 32))",
        "UPDATE media.jobs SET state = 'failed'",
        "CREATE TABLE media.reader_must_not_create (id int)",
        "CREATE SCHEMA reader_must_not_create",
        "SET ROLE board_media",
        "SET ROLE board_migrator",
    ] {
        let error = read.execute(query).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501"),
            "{query}"
        );
    }
    for variable in [
        "MEDIA_DATABASE_URL",
        "TEST_PUBLIC_DATABASE_URL",
        "AUTH_DATABASE_URL",
        "STAFF_DATABASE_URL",
        "MIGRATION_DATABASE_URL",
    ] {
        let url = std::env::var(variable).unwrap_or_else(|_| panic!("{variable} is required"));
        assert!(
            matches!(
                MediaReader::connect(&url).await,
                Err(StoreError::UnsafeRole)
            ),
            "{variable}"
        );
        if variable != "MIGRATION_DATABASE_URL" {
            let mut other = PgConnection::connect(&url).await.unwrap();
            let error = other
                .execute("SELECT * FROM media.approved_assets")
                .await
                .unwrap_err();
            assert_eq!(
                error.as_database_error().unwrap().code().as_deref(),
                Some("42501"),
                "{variable}"
            );
        }
    }
}
