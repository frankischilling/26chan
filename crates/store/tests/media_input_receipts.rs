#![cfg(feature = "database-tests")]

use board_store::{
    media::MediaQueue,
    media_assets::{InputComponent, InputReceipt, OutputMetadata},
};
use sqlx::PgPool;

fn receipt() -> InputReceipt {
    InputReceipt {
        component: InputComponent::Upload,
        sha256: "b".repeat(64),
        bytes: 100,
        md5: [7; 16],
    }
}

async fn pending(queue: &MediaQueue) -> (String, String, String) {
    let job = queue.reserve("private-input.png").await.unwrap();
    queue.queue(&job.id, 100).await.unwrap();
    let claimed = queue.claim().await.unwrap().unwrap();
    assert_eq!(claimed.id, job.id, "Use an idle disposable queue");
    let token = claimed.lease_token.unwrap();
    let asset = queue
        .prepare_output(
            &job.id,
            &token,
            &OutputMetadata {
                sha256: "a".repeat(64),
                bytes: 80,
                width: 10,
                height: 10,
            },
        )
        .await
        .unwrap();
    (job.id, token, asset.id)
}

#[tokio::test]
async fn original_receipts_are_private_lease_fenced_and_survive_queue_cleanup() {
    let admin = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let media = PgPool::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let (job, token, asset) = pending(&queue).await;
    let input = receipt();
    assert!(
        queue
            .approve_output_with_input(&job, &token, &asset, &input)
            .await
            .is_err()
    );
    for bad in [
        InputReceipt {
            bytes: 99,
            ..receipt()
        },
        InputReceipt {
            component: InputComponent::PairedImage,
            ..receipt()
        },
    ] {
        assert!(
            queue
                .record_input_receipt(&job, &token, &asset, &bad)
                .await
                .is_err()
        );
    }
    assert!(
        queue
            .record_input_receipt(&job, &"0".repeat(32), &asset, &input)
            .await
            .is_err()
    );
    for sql in [
        "SELECT * FROM media.input_receipts",
        "DELETE FROM media.input_receipts",
        "UPDATE media.input_receipts SET bytes=1",
    ] {
        assert!(sqlx::query(sql).execute(&media).await.is_err());
    }
    let exposed: i64 = sqlx::query_scalar("SELECT count(*) FROM pg_attribute WHERE attrelid IN ('media.approved_assets'::regclass,'media.approved_post_assets'::regclass) AND attname IN ('component','raw_md5','input_md5') AND NOT attisdropped")
        .fetch_one(&admin).await.unwrap();
    assert_eq!(exposed, 0);
    for role in [
        "board_public",
        "board_staff",
        "board_media_read",
        "board_media_intake",
        "board_auth",
    ] {
        let allowed: bool = sqlx::query_scalar("SELECT has_table_privilege($1,'media.input_receipts','SELECT') OR has_function_privilege($1,'media.record_input_receipt(text,text,text,text,bigint,bytea,text)','EXECUTE')")
            .bind(role).fetch_one(&admin).await.unwrap();
        assert!(!allowed, "{role}");
    }
    for (sha, bytes, md5, component) in [
        (None, Some(100_i64), Some(vec![7_u8; 16]), Some("upload")),
        (
            Some("b".repeat(64)),
            None,
            Some(vec![7_u8; 16]),
            Some("upload"),
        ),
        (Some("b".repeat(64)), Some(100_i64), None, Some("upload")),
        (
            Some("b".repeat(64)),
            Some(100_i64),
            Some(vec![7_u8; 16]),
            None,
        ),
        (
            Some("b".repeat(64)),
            Some(100_i64),
            Some(vec![7_u8; 15]),
            Some("upload"),
        ),
    ] {
        assert!(
            sqlx::query("SELECT media.record_input_receipt($1,$2,$3,$4,$5,$6,$7)")
                .bind(&job)
                .bind(&token)
                .bind(&asset)
                .bind(sha)
                .bind(bytes)
                .bind(md5)
                .bind(component)
                .execute(&media)
                .await
                .is_err()
        );
    }
    queue
        .record_input_receipt(&job, &token, &asset, &input)
        .await
        .unwrap();
    queue
        .record_input_receipt(&job, &token, &asset, &input)
        .await
        .unwrap();
    for bad in [
        InputReceipt {
            sha256: "c".repeat(64),
            ..receipt()
        },
        InputReceipt {
            md5: [8; 16],
            ..receipt()
        },
        InputReceipt {
            bytes: 101,
            ..receipt()
        },
        InputReceipt {
            component: InputComponent::PairedImage,
            ..receipt()
        },
    ] {
        assert!(
            queue
                .record_input_receipt(&job, &token, &asset, &bad)
                .await
                .is_err()
        );
    }
    assert!(
        queue
            .approve_output_with_input(
                &job,
                &token,
                &asset,
                &InputReceipt {
                    md5: [8; 16],
                    ..receipt()
                }
            )
            .await
            .is_err()
    );
    assert_eq!(queue.get(&job).await.unwrap().state, "processing");
    assert!(
        sqlx::query("UPDATE media.input_receipts SET md5=$2 WHERE asset_id=$1")
            .bind(&asset)
            .bind(vec![8u8; 16])
            .execute(&admin)
            .await
            .is_err()
    );
    queue
        .approve_output_with_input(&job, &token, &asset, &input)
        .await
        .unwrap();
    sqlx::query("DELETE FROM media.jobs WHERE id=$1")
        .bind(&job)
        .execute(&admin)
        .await
        .unwrap();
    queue
        .record_input_receipt(&job, &token, &asset, &input)
        .await
        .unwrap();
    let saved: (String, i64, Vec<u8>, String) = sqlx::query_as(
        "SELECT sha256,bytes,md5,component FROM media.input_receipts WHERE asset_id=$1",
    )
    .bind(&asset)
    .fetch_one(&admin)
    .await
    .unwrap();
    assert_eq!(saved, (input.sha256, 100, vec![7; 16], "upload".into()));
    sqlx::query("DELETE FROM media.assets WHERE id=$1")
        .bind(&asset)
        .execute(&admin)
        .await
        .unwrap();
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM media.input_receipts WHERE asset_id=$1")
            .bind(&asset)
            .fetch_one(&admin)
            .await
            .unwrap();
    assert_eq!(count, 0);

    // An old/manual approval cannot acquire invented original input later.
    let (job, token, asset) = pending(&queue).await;
    queue.approve_output(&job, &token, &asset).await.unwrap();
    assert!(
        queue
            .record_input_receipt(&job, &token, &asset, &receipt())
            .await
            .is_err()
    );
    sqlx::query("DELETE FROM media.assets WHERE id=$1")
        .bind(&asset)
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("DELETE FROM media.jobs WHERE id=$1")
        .bind(&job)
        .execute(&admin)
        .await
        .unwrap();

    // Existing pending receipts do not exempt expired processing leases.
    let (job, token, asset) = pending(&queue).await;
    queue
        .record_input_receipt(&job, &token, &asset, &receipt())
        .await
        .unwrap();
    sqlx::query(
        "UPDATE media.jobs SET expires_at=clock_timestamp()-interval '1 second' WHERE id=$1",
    )
    .bind(&job)
    .execute(&admin)
    .await
    .unwrap();
    assert!(
        queue
            .record_input_receipt(&job, &token, &asset, &receipt())
            .await
            .is_err()
    );
    // A reconciliation pass intentionally skips a job still held by another
    // transaction; a settled rollback is required before expecting a retry.
    let mut lock = admin.begin().await.unwrap();
    sqlx::query("SELECT id FROM media.jobs WHERE id=$1 FOR UPDATE")
        .bind(&job)
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    assert_eq!(queue.expire().await.unwrap(), 0);
    assert!(queue.claim().await.unwrap().is_none());
    lock.commit().await.unwrap();
    assert_eq!(queue.expire().await.unwrap(), 1);
    assert_eq!(queue.claim().await.unwrap().unwrap().id, job);

    sqlx::query("DELETE FROM media.assets WHERE id=$1")
        .bind(&asset)
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("DELETE FROM media.jobs WHERE id=$1")
        .bind(&job)
        .execute(&admin)
        .await
        .unwrap();
}
