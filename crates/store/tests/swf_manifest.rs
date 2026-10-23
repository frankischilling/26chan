#![cfg(feature = "database-tests")]

#[tokio::test]
async fn swf_manifest_requires_upload_board_lease_and_exact_original_receipt() {
    let pool = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut tx = pool.begin().await.unwrap();
    let result = sqlx::raw_sql(include_str!("fixtures/swf_manifest.sql"))
        .execute(&mut *tx)
        .await;
    tx.rollback().await.unwrap();
    pool.close().await;
    result.unwrap();
}

#[tokio::test]
async fn swf_reservation_retries_keep_exact_input_and_survive_terminal_job_cleanup() {
    use board_store::{
        media::MediaQueue,
        media_assets::{InputComponent, InputReceipt, OutputMetadata},
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
        .reserve_image("owned-retry.swf", "f", 0, false)
        .await
        .unwrap();
    intake
        .begin_upload(&upload.id, &upload.capability)
        .await
        .unwrap();
    intake
        .finish_upload(&upload.id, &upload.capability, 100)
        .await
        .unwrap();
    let job = queue.claim().await.unwrap().unwrap();
    assert_eq!(job.id, upload.id, "Use an idle disposable queue");
    let token = job.lease_token.as_deref().unwrap();
    let metadata = OutputMetadata {
        sha256: "a".repeat(64),
        bytes: 100,
        width: 640,
        height: 480,
    };
    let input = InputReceipt {
        component: InputComponent::Upload,
        sha256: metadata.sha256.clone(),
        bytes: 100,
        md5: [7; 16],
    };
    let asset = queue
        .prepare_swf_output(&job.id, token, &metadata, &input)
        .await
        .unwrap();
    assert_eq!(
        queue
            .prepare_swf_output(&job.id, token, &metadata, &input)
            .await
            .unwrap(),
        asset
    );
    assert!(
        queue
            .approve_output(&job.id, token, &asset.id)
            .await
            .is_err()
    );
    for change in [0, 1, 2, 3] {
        let mut altered = metadata.clone();
        let mut receipt = input.clone();
        match change {
            0 => {
                altered.sha256 = "b".repeat(64);
                receipt.sha256 = altered.sha256.clone();
            }
            1 => {
                receipt.md5 = [8; 16];
            }
            2 => {
                altered.width = 641;
            }
            _ => {
                receipt.bytes = 99;
            }
        }
        assert!(
            queue
                .prepare_swf_output(&job.id, token, &altered, &receipt)
                .await
                .is_err()
        );
    }
    queue
        .record_input_receipt(&job.id, token, &asset.id, &input)
        .await
        .unwrap();
    queue
        .approve_output_with_input(&job.id, token, &asset.id, &input)
        .await
        .unwrap();
    sqlx::query("DELETE FROM media.jobs WHERE id=$1")
        .bind(&job.id)
        .execute(&admin)
        .await
        .unwrap();
    assert_eq!(
        queue
            .prepare_swf_output(&job.id, token, &metadata, &input)
            .await
            .unwrap(),
        asset
    );
    let mut altered = input.clone();
    altered.md5 = [8; 16];
    assert!(
        queue
            .prepare_swf_output(&job.id, token, &metadata, &altered)
            .await
            .is_err()
    );
    sqlx::query("DELETE FROM media.assets WHERE id=$1")
        .bind(&asset.id)
        .execute(&admin)
        .await
        .unwrap();
}
