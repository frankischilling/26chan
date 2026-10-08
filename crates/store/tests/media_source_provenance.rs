#![cfg(feature = "database-tests")]

use board_store::{
    StoreError,
    media::MediaQueue,
    media_assets::{MediaReader, OutputMetadata, SourceProfile, SourceProvenance},
};
use sqlx::PgPool;
use std::sync::{Arc, Mutex};

fn metadata() -> OutputMetadata {
    OutputMetadata {
        sha256: "a".repeat(64),
        bytes: 80,
        width: 10,
        height: 10,
    }
}

fn provenance() -> SourceProvenance {
    SourceProvenance {
        input_sha256: "b".repeat(64),
        input_bytes: 100,
        profile: SourceProfile::PngV1,
        retained_bytes: 70,
        md5: [7; 16],
    }
}

struct Fixture {
    admin: PgPool,
    writer: PgPool,
    queue: MediaQueue,
    jobs: Arc<Mutex<Vec<String>>>,
}

impl Fixture {
    async fn claim(&self) -> (String, String) {
        let job = self
            .queue
            .reserve("source-provenance-test.png")
            .await
            .unwrap();
        self.jobs.lock().unwrap().push(job.id.clone());
        self.queue.queue(&job.id, 100).await.unwrap();
        let claimed = self.queue.claim().await.unwrap().unwrap();
        assert_eq!(claimed.id, job.id, "Use an idle disposable media queue");
        (job.id, claimed.lease_token.unwrap())
    }

    async fn expire(&self, job: &str) {
        sqlx::query(
            "UPDATE media.jobs SET expires_at=clock_timestamp()-interval '1 second' WHERE id=$1",
        )
        .bind(job)
        .execute(&self.admin)
        .await
        .unwrap();
    }
}

#[tokio::test]
async fn source_provenance_is_private_immutable_and_exactly_retryable() {
    let admin = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let writer = PgPool::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let jobs = Arc::new(Mutex::new(Vec::new()));
    let f = Fixture {
        admin: admin.clone(),
        writer,
        queue,
        jobs: jobs.clone(),
    };
    let result = tokio::spawn(async move { exercise(f).await }).await;
    let jobs = jobs.lock().unwrap().clone();
    sqlx::query("DELETE FROM media.assets WHERE job_id=ANY($1)")
        .bind(&jobs)
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("DELETE FROM media.jobs WHERE id=ANY($1)")
        .bind(&jobs)
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    result.unwrap();
}

async fn exercise(f: Fixture) {
    let reader_url = std::env::var("MEDIA_READ_DATABASE_URL").unwrap();
    let reader = MediaReader::connect(&reader_url).await.unwrap();
    let reader_pool = PgPool::connect(&reader_url).await.unwrap();
    let role: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&f.writer)
        .await
        .unwrap();
    assert_eq!(role, "board_media");
    let role: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&f.admin)
        .await
        .unwrap();
    assert_eq!(role, "board_migrator");
    assert!(
        sqlx::query("SELECT source_input_sha256 FROM media.assets")
            .execute(&reader_pool)
            .await
            .is_err()
    );
    let leaked: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_attribute WHERE attrelid IN ('media.approved_assets'::regclass,'media.approved_post_assets'::regclass) AND attname LIKE 'source_%' AND NOT attisdropped")
        .fetch_one(&f.admin).await.unwrap();
    assert_eq!(leaked, 0);

    let (job, token) = f.claim().await;
    let meta = metadata();
    let source = provenance();
    for invalid in [
        SourceProvenance {
            input_sha256: "B".repeat(64),
            ..provenance()
        },
        SourceProvenance {
            input_sha256: "b".repeat(63),
            ..provenance()
        },
        SourceProvenance {
            input_sha256: format!("{}\n", "b".repeat(63)),
            ..provenance()
        },
        SourceProvenance {
            input_bytes: 19,
            ..provenance()
        },
        SourceProvenance {
            input_bytes: 8_388_609,
            ..provenance()
        },
        SourceProvenance {
            retained_bytes: 19,
            ..provenance()
        },
        SourceProvenance {
            retained_bytes: 101,
            ..provenance()
        },
    ] {
        assert!(matches!(
            f.queue
                .prepare_output_with_provenance(&job, &token, &meta, None, Some(&invalid))
                .await,
            Err(StoreError::Invalid(_))
        ));
    }
    let substituted = SourceProvenance {
        input_bytes: 101,
        ..provenance()
    };
    assert!(matches!(
        f.queue
            .prepare_output_with_provenance(&job, &token, &meta, None, Some(&substituted))
            .await,
        Err(StoreError::Conflict(_))
    ));

    // Direct restricted-role inserts cannot evade the complete tuple check.
    // Each insertion rolls back, leaving the same current job for the next case.
    for (hash, bytes, profile, retained, md5) in [
        (
            Some("B".repeat(64)),
            Some(100),
            Some("png-v1"),
            Some(70),
            Some(vec![7; 16]),
        ),
        (
            Some("b".repeat(63)),
            Some(100),
            Some("png-v1"),
            Some(70),
            Some(vec![7; 16]),
        ),
        (
            Some(format!("{}\n", "b".repeat(64))),
            Some(100),
            Some("png-v1"),
            Some(70),
            Some(vec![7; 16]),
        ),
        (
            Some("b".repeat(64)),
            Some(19),
            Some("png-v1"),
            Some(19),
            Some(vec![7; 16]),
        ),
        (
            Some("b".repeat(64)),
            Some(8_388_609),
            Some("png-v1"),
            Some(70),
            Some(vec![7; 16]),
        ),
        (
            Some("b".repeat(64)),
            Some(100),
            Some("png-v2"),
            Some(70),
            Some(vec![7; 16]),
        ),
        (
            Some("b".repeat(64)),
            Some(100),
            Some("png-v1"),
            Some(19),
            Some(vec![7; 16]),
        ),
        (
            Some("b".repeat(64)),
            Some(100),
            Some("png-v1"),
            Some(101),
            Some(vec![7; 16]),
        ),
        (
            Some("b".repeat(64)),
            Some(100),
            Some("png-v1"),
            Some(70),
            Some(vec![7; 15]),
        ),
        (
            Some("b".repeat(64)),
            Some(100),
            Some("png-v1"),
            Some(70),
            Some(vec![7; 17]),
        ),
        (None, Some(100), Some("png-v1"), Some(70), Some(vec![7; 16])),
        (
            Some("b".repeat(64)),
            None,
            Some("png-v1"),
            Some(70),
            Some(vec![7; 16]),
        ),
        (
            Some("b".repeat(64)),
            Some(100),
            None,
            Some(70),
            Some(vec![7; 16]),
        ),
        (
            Some("b".repeat(64)),
            Some(100),
            Some("png-v1"),
            None,
            Some(vec![7; 16]),
        ),
        (
            Some("b".repeat(64)),
            Some(100),
            Some("png-v1"),
            Some(70),
            None,
        ),
    ] {
        let mut tx = f.writer.begin().await.unwrap();
        let result = sqlx::query("INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,source_input_sha256,source_input_bytes,source_profile,source_retained_bytes,source_md5) VALUES (replace(gen_random_uuid()::text,'-',''),$1,$2,$3,80,10,10,$4,$5,$6,$7,$8)")
            .bind(&job).bind(&token).bind(&meta.sha256).bind(hash).bind(bytes.map(i64::from))
            .bind(profile).bind(retained.map(i64::from)).bind(md5).execute(&mut *tx).await;
        assert!(result.is_err());
        tx.rollback().await.unwrap();
    }
    // Both legal endpoint lengths and retained-byte bounds are accepted.
    for (input, retained) in [(20_i64, 20_i64), (8_388_608, 20), (8_388_608, 8_388_608)] {
        let mut tx = f.writer.begin().await.unwrap();
        sqlx::query("UPDATE media.jobs SET input_bytes=$2 WHERE id=$1")
            .bind(&job)
            .bind(input)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,source_input_sha256,source_input_bytes,source_profile,source_retained_bytes,source_md5) VALUES (replace(gen_random_uuid()::text,'-',''),$1,$2,$3,80,10,10,$4,$5,'png-v1',$6,$7)")
            .bind(&job).bind(&token).bind(&meta.sha256).bind(&source.input_sha256)
            .bind(input).bind(retained).bind(source.md5.as_slice()).execute(&mut *tx).await.unwrap();
        tx.rollback().await.unwrap();
    }
    // Prove the lower bound is a tuple constraint, independently of binding.
    let mut tx = f.writer.begin().await.unwrap();
    sqlx::query("UPDATE media.jobs SET input_bytes=19 WHERE id=$1")
        .bind(&job)
        .execute(&mut *tx)
        .await
        .unwrap();
    let error = sqlx::query("INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,source_input_sha256,source_input_bytes,source_profile,source_retained_bytes,source_md5) VALUES (replace(gen_random_uuid()::text,'-',''),$1,$2,$3,80,10,10,$4,19,'png-v1',19,$5)")
        .bind(&job).bind(&token).bind(&meta.sha256).bind(&source.input_sha256)
        .bind(source.md5.as_slice()).execute(&mut *tx).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    tx.rollback().await.unwrap();

    for (id, lease, bytes, state, approved_at) in [
        (job.as_str(), token.as_str(), 101_i64, "pending", "NULL"),
        (
            job.as_str(),
            "00000000000000000000000000000000",
            100,
            "pending",
            "NULL",
        ),
        (
            "00000000000000000000000000000000",
            token.as_str(),
            100,
            "pending",
            "NULL",
        ),
        (
            job.as_str(),
            token.as_str(),
            100,
            "approved",
            "clock_timestamp()",
        ),
        (job.as_str(), token.as_str(), 100, "deleting", "NULL"),
    ] {
        let error = sqlx::query("INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at,source_input_sha256,source_input_bytes,source_profile,source_retained_bytes,source_md5) VALUES (replace(gen_random_uuid()::text,'-',''),$1,$2,$3,80,10,10,$4,CASE WHEN $8 THEN clock_timestamp() ELSE NULL END,$5,$6,'png-v1',70,$7)")
            .bind(id).bind(lease).bind(&meta.sha256).bind(state).bind(&source.input_sha256)
            .bind(bytes).bind(source.md5.as_slice()).bind(approved_at == "clock_timestamp()")
            .execute(&f.writer).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501")
        );
    }

    let asset = f
        .queue
        .prepare_output_with_provenance(&job, &token, &meta, None, Some(&source))
        .await
        .unwrap();
    assert_eq!(
        f.queue
            .prepare_output_with_provenance(&job, &token, &meta, None, Some(&source))
            .await
            .unwrap(),
        asset
    );
    sqlx::query("UPDATE media.jobs SET input_bytes=101 WHERE id=$1")
        .bind(&job)
        .execute(&f.admin)
        .await
        .unwrap();
    assert!(matches!(
        f.queue
            .prepare_output_with_provenance(&job, &token, &meta, None, Some(&source))
            .await,
        Err(StoreError::Conflict(_))
    ));
    sqlx::query("UPDATE media.jobs SET input_bytes=100,lease_token=repeat('0',32) WHERE id=$1")
        .bind(&job)
        .execute(&f.admin)
        .await
        .unwrap();
    assert!(matches!(
        f.queue
            .prepare_output_with_provenance(&job, &token, &meta, None, Some(&source))
            .await,
        Err(StoreError::Conflict(_))
    ));
    sqlx::query("UPDATE media.jobs SET lease_token=$2 WHERE id=$1")
        .bind(&job)
        .bind(&token)
        .execute(&f.admin)
        .await
        .unwrap();
    for pool in [&f.writer, &f.admin] {
        for statement in [
            "UPDATE media.assets SET source_input_sha256=repeat('c',64) WHERE id=$1",
            "UPDATE media.assets SET source_input_bytes=101 WHERE id=$1",
            "UPDATE media.assets SET source_profile='png-v2' WHERE id=$1",
            "UPDATE media.assets SET source_retained_bytes=71 WHERE id=$1",
            "UPDATE media.assets SET source_md5=decode(repeat('08',16),'hex') WHERE id=$1",
            "UPDATE media.assets SET source_input_sha256=NULL,source_input_bytes=NULL,source_profile=NULL,source_retained_bytes=NULL,source_md5=NULL WHERE id=$1",
        ] {
            let error = sqlx::query(statement)
                .bind(&asset.id)
                .execute(pool)
                .await
                .unwrap_err();
            assert_eq!(
                error.as_database_error().unwrap().code().as_deref(),
                Some("42501")
            );
        }
    }
    // Pending and approved retries compare every representable field and None.
    for approved in [false, true] {
        if approved {
            f.queue
                .approve_output(&job, &token, &asset.id)
                .await
                .unwrap();
            sqlx::query("DELETE FROM media.jobs WHERE id=$1")
                .bind(&job)
                .execute(&f.admin)
                .await
                .unwrap();
        }
        assert_eq!(
            f.queue
                .prepare_output_with_provenance(&job, &token, &meta, None, Some(&source))
                .await
                .unwrap(),
            asset
        );
        for changed in [
            SourceProvenance {
                input_sha256: "c".repeat(64),
                ..provenance()
            },
            SourceProvenance {
                input_bytes: 101,
                ..provenance()
            },
            SourceProvenance {
                retained_bytes: 71,
                ..provenance()
            },
            SourceProvenance {
                md5: [8; 16],
                ..provenance()
            },
        ] {
            assert!(matches!(
                f.queue
                    .prepare_output_with_provenance(&job, &token, &meta, None, Some(&changed))
                    .await,
                Err(StoreError::Conflict(_))
            ));
        }
        assert!(matches!(
            f.queue.prepare_output(&job, &token, &meta).await,
            Err(StoreError::Conflict(_))
        ));
        let stored: (String, i64, String, i64, Vec<u8>) = sqlx::query_as("SELECT source_input_sha256,source_input_bytes,source_profile,source_retained_bytes,source_md5 FROM media.assets WHERE id=$1")
            .bind(&asset.id).fetch_one(&f.writer).await.unwrap();
        assert_eq!(
            stored,
            (
                source.input_sha256.clone(),
                source.input_bytes,
                "png-v1".into(),
                source.retained_bytes,
                source.md5.to_vec()
            )
        );
    }
    assert_eq!(reader.get(&asset.id).await.unwrap(), asset);

    // Historical None remains usable and cannot be backfilled, even by owner.
    let (legacy_job, legacy_token) = f.claim().await;
    let legacy = f
        .queue
        .prepare_output(&legacy_job, &legacy_token, &meta)
        .await
        .unwrap();
    for approved in [false, true] {
        if approved {
            f.queue
                .approve_output(&legacy_job, &legacy_token, &legacy.id)
                .await
                .unwrap();
        }
        assert!(matches!(
            f.queue
                .prepare_output_with_provenance(
                    &legacy_job,
                    &legacy_token,
                    &meta,
                    None,
                    Some(&source)
                )
                .await,
            Err(StoreError::Conflict(_))
        ));
        let error = sqlx::query("UPDATE media.assets SET source_input_sha256=$2,source_input_bytes=100,source_profile='png-v1',source_retained_bytes=70,source_md5=$3 WHERE id=$1")
            .bind(&legacy.id).bind(&source.input_sha256).bind(source.md5.as_slice()).execute(&f.admin).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501")
        );
    }
    sqlx::query("DELETE FROM media.jobs WHERE id=$1")
        .bind(&legacy_job)
        .execute(&f.admin)
        .await
        .unwrap();
    assert_eq!(
        f.queue
            .prepare_output(&legacy_job, &legacy_token, &meta)
            .await
            .unwrap(),
        legacy
    );

    let (expired_job, expired_token) = f.claim().await;
    f.queue
        .prepare_output_with_provenance(&expired_job, &expired_token, &meta, None, Some(&source))
        .await
        .unwrap();
    f.expire(&expired_job).await;
    assert!(matches!(
        f.queue
            .prepare_output_with_provenance(
                &expired_job,
                &expired_token,
                &meta,
                None,
                Some(&source)
            )
            .await,
        Err(StoreError::Conflict(_))
    ));
    let (new_job, new_token) = f.claim().await;
    f.expire(&new_job).await;
    assert!(matches!(
        f.queue
            .prepare_output_with_provenance(&new_job, &new_token, &meta, None, Some(&source))
            .await,
        Err(StoreError::Conflict(_))
    ));
    for (id, token, state, approved_at) in [
        (new_job.as_str(), new_token.as_str(), "pending", "NULL"),
        (
            "00000000000000000000000000000000",
            new_token.as_str(),
            "pending",
            "NULL",
        ),
        (
            new_job.as_str(),
            "00000000000000000000000000000000",
            "pending",
            "NULL",
        ),
        (
            new_job.as_str(),
            new_token.as_str(),
            "approved",
            "clock_timestamp()",
        ),
    ] {
        let error = sqlx::query("INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at,source_input_sha256,source_input_bytes,source_profile,source_retained_bytes,source_md5) VALUES (replace(gen_random_uuid()::text,'-',''),$1,$2,$3,80,10,10,$4,CASE WHEN $7 THEN clock_timestamp() ELSE NULL END,$5,100,'png-v1',70,$6)")
            .bind(id).bind(token).bind(&meta.sha256).bind(state).bind(&source.input_sha256)
            .bind(source.md5.as_slice()).bind(approved_at == "clock_timestamp()")
            .execute(&f.writer).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501")
        );
    }
    reader.close().await;
    reader_pool.close().await;
}
