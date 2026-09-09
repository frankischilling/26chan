#![cfg(feature = "database-tests")]

use board_media::{Promoter, Quarantine, ValidatedOutput};
use board_store::media::{Failure, MediaQueue};

#[tokio::test]
async fn synthetic_result_connects_private_intake_fenced_queue_and_idempotent_publication() {
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(temp.path().join("private")).unwrap();
    let public = temp.path().join("public");
    let promoter = Promoter::new(&public, &quarantine).unwrap();
    let job = queue
        .reserve("harmless fixture, no decoder runs")
        .await
        .unwrap();
    let id = job.id.parse().unwrap();
    let bytes = quarantine
        .receive(id, &b"undecoded original"[..])
        .await
        .unwrap();
    queue.queue(&job.id, bytes).await.unwrap();
    let lease = queue.claim().await.unwrap().unwrap();
    assert_eq!(lease.id, job.id);
    assert!(ValidatedOutput::read(&b"invalid result"[..]).await.is_err());
    queue
        .fail(
            &job.id,
            lease.lease_token.as_deref().unwrap(),
            Failure::InvalidOutput,
            false,
        )
        .await
        .unwrap();
    assert_eq!(std::fs::read_dir(&public).unwrap().count(), 0);
    assert_eq!(
        queue.get(&job.id).await.unwrap().failure.as_deref(),
        Some("invalid_output")
    );

    let accepted = queue.reserve("one synthetic red pixel").await.unwrap();
    let accepted_id = accepted.id.parse().unwrap();
    let bytes = quarantine
        .receive(accepted_id, &b"another undecoded original"[..])
        .await
        .unwrap();
    queue.queue(&accepted.id, bytes).await.unwrap();
    let lease = queue.claim().await.unwrap().unwrap();
    assert_eq!(lease.id, accepted.id);
    // Test fixture protocol, not a worker implementation or an isolation test.
    let frame = b"IBRGBA01\0\0\0\x01\0\0\0\x01\xff\0\0\xff";
    let output = ValidatedOutput::read(&frame[..]).await.unwrap();
    let receipt = promoter.promote(accepted_id, &output).unwrap();
    queue
        .complete(
            &accepted.id,
            lease.lease_token.as_deref().unwrap(),
            &receipt.sha256,
            receipt.bytes,
        )
        .await
        .unwrap();
    let replay = promoter.promote(accepted_id, &output).unwrap();
    assert!(replay.already_published);
    queue
        .complete(
            &accepted.id,
            lease.lease_token.as_deref().unwrap(),
            &replay.sha256,
            replay.bytes,
        )
        .await
        .unwrap();
    let persisted = queue.get(&accepted.id).await.unwrap();
    assert_eq!(persisted.state, "published");
    assert_eq!(
        persisted.output_sha256.as_deref(),
        Some(receipt.sha256.as_str())
    );
    assert_eq!(persisted.output_bytes, Some(receipt.bytes as i64));
    assert_eq!(std::fs::read_dir(&public).unwrap().count(), 1);
    let png = std::fs::read(public.join(format!("{accepted_id}.png"))).unwrap();
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    let admin = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    sqlx::query("DELETE FROM media.jobs WHERE id = ANY($1)")
        .bind(vec![job.id, accepted.id])
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
}
