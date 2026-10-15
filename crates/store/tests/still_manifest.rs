#![cfg(feature = "database-tests")]

#[tokio::test]
async fn source_gif_manifest_keeps_separate_limits_and_private_context() {
    let pool = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut tx = pool.begin().await.unwrap();
    let result = sqlx::raw_sql(include_str!("fixtures/source_gif_manifest.sql"))
        .execute(&mut *tx)
        .await;
    tx.rollback().await.unwrap();
    pool.close().await;
    result.unwrap();
}

#[tokio::test]
async fn still_manifest_is_bounded_immutable_and_lease_fenced() {
    let pool = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut tx = pool.begin().await.unwrap();
    let result = sqlx::raw_sql(include_str!("fixtures/still_manifest.sql"))
        .execute(&mut *tx)
        .await;
    tx.rollback().await.unwrap();
    pool.close().await;
    result.unwrap();
}

#[tokio::test]
async fn still_reservation_retries_compare_every_companion_and_reject_expired_leases() {
    use board_store::{
        media::MediaQueue,
        media_assets::{
            MediaFormat, OutputMetadata, OutputVariants, SourceProfile, SourceProvenance,
            StillOutput,
        },
        media_intake::IntakeStore,
    };
    let admin = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let intake = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let upload = intake
        .reserve_image("synthetic.jpg", "p", 0, true)
        .await
        .unwrap();
    let id = upload.id.clone();
    let owner = admin.clone();
    let result = tokio::spawn(async move {
        intake
            .begin_upload(&upload.id, &upload.capability)
            .await
            .unwrap();
        intake
            .finish_upload(&upload.id, &upload.capability, 100)
            .await
            .unwrap();
        let job = queue.claim().await.unwrap().unwrap();
        assert_eq!(job.id, upload.id, "Use an idle disposable media queue");
        let token = job.lease_token.as_deref().unwrap();
        queue.extend_still_lease(&job.id, token).await.unwrap();
        let output = StillOutput {
            format: MediaFormat::Jpeg,
            primary: OutputMetadata {
                sha256: "a".repeat(64),
                bytes: 200,
                width: 2000,
                height: 1000,
            },
            variants: OutputVariants {
                md5: "07".repeat(16),
                thumbnail: OutputMetadata {
                    sha256: "b".repeat(64),
                    bytes: 100,
                    width: 250,
                    height: 125,
                },
            },
            mobile: Some(OutputMetadata {
                sha256: "c".repeat(64),
                bytes: 150,
                width: 1024,
                height: 512,
            }),
            source: SourceProvenance {
                input_sha256: "d".repeat(64),
                input_bytes: 100,
                profile: SourceProfile::StillV6,
                retained_bytes: 200,
                md5: [7; 16],
            },
        };
        let asset = queue
            .prepare_still_output(&job.id, token, &output)
            .await
            .unwrap();
        assert_eq!(asset.output_format, MediaFormat::Jpeg);
        assert_eq!(
            queue
                .prepare_still_output(&job.id, token, &output)
                .await
                .unwrap(),
            asset
        );
        for mutate in [
            |o: &mut StillOutput| o.mobile = None,
            |o: &mut StillOutput| o.mobile.as_mut().unwrap().sha256 = "e".repeat(64),
            |o: &mut StillOutput| o.mobile.as_mut().unwrap().height = 511,
            |o: &mut StillOutput| o.variants.thumbnail.sha256 = "f".repeat(64),
            |o: &mut StillOutput| o.format = MediaFormat::Png,
            |o: &mut StillOutput| o.source.input_sha256 = "1".repeat(64),
        ] {
            let mut changed = output.clone();
            mutate(&mut changed);
            assert!(
                queue
                    .prepare_still_output(&job.id, token, &changed)
                    .await
                    .is_err()
            );
        }
        assert!(
            queue
                .prepare_output_with_provenance(
                    &job.id,
                    token,
                    &output.primary,
                    Some(&output.variants),
                    Some(&output.source)
                )
                .await
                .is_err()
        );
        sqlx::query(
            "UPDATE media.jobs SET expires_at=clock_timestamp()-interval '1 second' WHERE id=$1",
        )
        .bind(&job.id)
        .execute(&owner)
        .await
        .unwrap();
        assert!(
            queue
                .prepare_still_output(&job.id, token, &output)
                .await
                .is_err()
        );
        assert!(
            queue
                .approve_output(&job.id, token, &asset.id)
                .await
                .is_err()
        );
    })
    .await;
    sqlx::query("DELETE FROM media.assets WHERE job_id=$1")
        .bind(&id)
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("DELETE FROM media.jobs WHERE id=$1")
        .bind(&id)
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    result.unwrap();
}

#[tokio::test]
async fn final_post_checks_source_image_context_without_trusting_the_upload_identity() {
    let pool = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut tx = pool.begin().await.unwrap();
    let result = sqlx::raw_sql(include_str!("fixtures/still_posting_context.sql"))
        .execute(&mut *tx)
        .await;
    tx.rollback().await.unwrap();
    pool.close().await;
    result.unwrap();
}
