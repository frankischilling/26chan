#![cfg(feature = "database-tests")]
use board_store::{
    media::MediaQueue,
    media_intake::{IntakeStore, PairedInputDescriptor},
};
use sqlx::PgPool;
static QUEUE_TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn drawing_enrollment_is_explicit_immutable_and_separate_from_candidate_claims() {
    let _serial = QUEUE_TEST.lock().await;
    let intake = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let media = PgPool::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let admin = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let drawing = intake.reserve_drawing("tegaki.png").await.unwrap();
    let candidate = intake.reserve_pair("candidate.png").await.unwrap();
    for reservation in [&drawing, &candidate] {
        intake
            .begin_pair_upload(&reservation.id, &reservation.capability)
            .await
            .unwrap();
        intake
            .finish_pair_upload(
                &reservation.id,
                &reservation.capability,
                &PairedInputDescriptor {
                    bytes: 60,
                    sha256: "a".repeat(64),
                    image_bytes: 3,
                    image_sha256: "b".repeat(64),
                    replay_bytes: Some(1),
                    replay_sha256: Some("c".repeat(64)),
                },
            )
            .await
            .unwrap();
    }
    for pool in [&media, &admin] {
        assert!(
            sqlx::query("UPDATE media.jobs SET drawing_publication=true WHERE id=$1")
                .bind(&candidate.id)
                .execute(pool)
                .await
                .is_err()
        );
        assert!(
            sqlx::query("UPDATE media.jobs SET drawing_publication=false WHERE id=$1")
                .bind(&drawing.id)
                .execute(pool)
                .await
                .is_err()
        );
    }
    assert!(queue.claim().await.unwrap().is_none());
    let lease = queue.claim_paired_candidate().await.unwrap().unwrap();
    assert_eq!(lease.id(), candidate.id);
    assert!(queue.claim_paired_candidate().await.unwrap().is_none());
    let claimed: String = sqlx::query_scalar("SELECT id FROM media.claim_drawing()")
        .fetch_one(&media)
        .await
        .unwrap();
    assert_eq!(claimed, drawing.id);
    let again: Option<String> = sqlx::query_scalar("SELECT id FROM media.claim_drawing()")
        .fetch_optional(&media)
        .await
        .unwrap();
    assert!(again.is_none());
    queue.finish_paired_candidate(lease, true).await.unwrap();
    for id in [&drawing.id, &candidate.id] {
        sqlx::query("DELETE FROM media.jobs WHERE id=$1")
            .bind(id)
            .execute(&admin)
            .await
            .unwrap();
    }
    intake.close().await.unwrap();
    drop(queue);
    media.close().await;
    admin.close().await;
}

#[tokio::test]
async fn replay_reader_has_no_raw_metadata_or_write_authority() {
    use board_store::{StoreError, media_assets::MediaReader};
    let url = std::env::var("MEDIA_READ_DATABASE_URL").unwrap();
    let reader = MediaReader::connect(&url).await.unwrap();
    reader.ready().await.unwrap();
    for (board, tim) in [
        ("i", 1),
        ("i", 0),
        ("../i", 1),
        ("i", 9_007_199_254_740_992),
    ] {
        assert!(matches!(
            reader.get_post_replay(board, tim).await,
            Err(StoreError::NotFound)
        ));
    }
    let raw = PgPool::connect(&url).await.unwrap();
    let failure = sqlx::query("SELECT * FROM media.replay_assets")
        .fetch_all(&raw)
        .await
        .unwrap_err();
    assert_eq!(
        failure.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
    let failure = sqlx::query("DELETE FROM media.approved_post_replays")
        .execute(&raw)
        .await
        .unwrap_err();
    assert!(matches!(
        failure.as_database_error().unwrap().code().as_deref(),
        Some("42501" | "55000")
    ));
    let writes: bool = sqlx::query_scalar("SELECT has_table_privilege(current_user,'media.approved_post_replays','INSERT,UPDATE,DELETE')")
        .fetch_one(&raw).await.unwrap();
    assert!(!writes);
    raw.close().await;
    reader.close().await;
}

#[tokio::test]
async fn drawing_publication_binds_one_current_lease_and_immutable_replay_companion() {
    let _serial = QUEUE_TEST.lock().await;
    let intake = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let media = PgPool::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let admin = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let reservation = intake.reserve_drawing("tegaki.png").await.unwrap();
    intake
        .begin_pair_upload(&reservation.id, &reservation.capability)
        .await
        .unwrap();
    intake
        .finish_pair_upload(
            &reservation.id,
            &reservation.capability,
            &PairedInputDescriptor {
                bytes: 90,
                sha256: "a".repeat(64),
                image_bytes: 33,
                image_sha256: "b".repeat(64),
                replay_bytes: Some(1),
                replay_sha256: Some("c".repeat(64)),
            },
        )
        .await
        .unwrap();
    let lease = queue.claim_drawing().await.unwrap().unwrap();
    let job = lease.job().id.clone();
    let token = lease.job().lease_token.clone().unwrap();
    assert_eq!(job, reservation.id);
    async fn reserve(
        pool: &PgPool,
        job: &str,
        token: &str,
        replay: &str,
    ) -> Result<String, sqlx::Error> {
        sqlx::query_scalar("SELECT id FROM media.prepare_drawing_output($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)")
            .bind(job).bind(token).bind("d".repeat(64)).bind(50_i64).bind(16_i32).bind(16_i32)
            .bind("e".repeat(32)).bind("f".repeat(64)).bind(40_i64).bind(16_i32).bind(16_i32)
            .bind(33_i64).bind(vec![1_u8;16]).bind(replay).bind(288_i64).fetch_one(pool).await
    }
    assert!(
        reserve(&media, &job, &"0".repeat(32), &"1".repeat(64))
            .await
            .is_err()
    );
    let asset = reserve(&media, &job, &token, &"1".repeat(64))
        .await
        .unwrap();
    assert_eq!(
        reserve(&media, &job, &token, &"1".repeat(64))
            .await
            .unwrap(),
        asset
    );
    assert!(
        reserve(&media, &job, &token, &"2".repeat(64))
            .await
            .is_err()
    );
    use board_store::media_assets::{
        OutputMetadata, OutputVariants, SourceProfile, SourceProvenance,
    };
    let image = OutputMetadata {
        sha256: "d".repeat(64),
        bytes: 50,
        width: 16,
        height: 16,
    };
    let variants = OutputVariants {
        md5: "e".repeat(32),
        thumbnail: OutputMetadata {
            sha256: "f".repeat(64),
            bytes: 40,
            width: 16,
            height: 16,
        },
    };
    let source = SourceProvenance {
        input_sha256: "b".repeat(64),
        input_bytes: 33,
        profile: SourceProfile::PngV1,
        retained_bytes: 33,
        md5: [1; 16],
    };
    let replay = OutputMetadata {
        sha256: "1".repeat(64),
        bytes: 288,
        width: 16,
        height: 16,
    };
    assert_eq!(
        queue
            .prepare_drawing_output(&lease, &image, &variants, &source, Some(&replay))
            .await
            .unwrap()
            .id,
        asset
    );
    assert!(
        queue
            .prepare_drawing_output(&lease, &image, &variants, &source, None)
            .await
            .is_err()
    );
    let wrong = OutputMetadata {
        width: 17,
        ..replay.clone()
    };
    assert!(
        queue
            .prepare_drawing_output(&lease, &image, &variants, &source, Some(&wrong))
            .await
            .is_err()
    );
    let early = sqlx::query(
        "UPDATE media.assets SET state='approved',approved_at=clock_timestamp() WHERE id=$1",
    )
    .bind(&asset)
    .execute(&media)
    .await
    .unwrap_err();
    assert_eq!(
        early.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
    let early=sqlx::query("UPDATE media.jobs SET state='published',output_sha256=$2,output_bytes=50,expires_at=NULL WHERE id=$1")
        .bind(&job).bind("d".repeat(64)).execute(&media).await.unwrap_err();
    assert_eq!(
        early.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
    use board_store::media_assets::{InputComponent, InputReceipt};
    let input = InputReceipt {
        component: InputComponent::PairedImage,
        sha256: "b".repeat(64),
        bytes: 33,
        md5: [7; 16],
    };
    assert!(
        sqlx::query("SELECT media.require_input_receipt($1,$2,$3,$4,$5,$6,$7)")
            .bind(&job)
            .bind(&token)
            .bind(&asset)
            .bind(&input.sha256)
            .bind(input.bytes)
            .bind(input.md5.as_slice())
            .bind("paired-image")
            .execute(&media)
            .await
            .is_err()
    );
    for wrong in [
        InputReceipt {
            component: InputComponent::Upload,
            ..input.clone()
        },
        InputReceipt {
            sha256: "a".repeat(64),
            ..input.clone()
        },
        InputReceipt {
            bytes: 90,
            ..input.clone()
        },
    ] {
        assert!(
            queue
                .record_input_receipt(&job, &token, &asset, &wrong)
                .await
                .is_err()
        );
    }
    queue
        .record_input_receipt(&job, &token, &asset, &input)
        .await
        .unwrap();
    assert_eq!(
        queue
            .approve_drawing_output_with_input(lease, &asset, &input)
            .await
            .unwrap()
            .id,
        asset
    );
    for _ in 0..2 {
        let approved: String =
            sqlx::query_scalar("SELECT id FROM media.approve_drawing_output($1,$2,$3)")
                .bind(&job)
                .bind(&token)
                .bind(&asset)
                .fetch_one(&media)
                .await
                .unwrap();
        assert_eq!(approved, asset);
    }

    let row:(String,String,i64)=sqlx::query_as("SELECT j.state,a.state,r.bytes FROM media.jobs j JOIN media.assets a ON a.job_id=j.id JOIN media.replay_assets r ON r.asset_id=a.id WHERE j.id=$1")
        .bind(&job).fetch_one(&admin).await.unwrap();
    assert_eq!(row, ("published".into(), "approved".into(), 288));
    assert!(
        sqlx::query("UPDATE media.replay_assets SET bytes=304 WHERE asset_id=$1")
            .bind(&asset)
            .execute(&admin)
            .await
            .is_err()
    );
    sqlx::query("DELETE FROM media.assets WHERE id=$1")
        .bind(&asset)
        .execute(&admin)
        .await
        .unwrap();
    let remains: i64 =
        sqlx::query_scalar("SELECT count(*) FROM media.replay_assets WHERE asset_id=$1")
            .bind(&asset)
            .fetch_one(&admin)
            .await
            .unwrap();
    assert_eq!(remains, 0);
    sqlx::query("DELETE FROM media.jobs WHERE id=$1")
        .bind(&job)
        .execute(&admin)
        .await
        .unwrap();
    intake.close().await.unwrap();
    media.close().await;
    admin.close().await;
}
