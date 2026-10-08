#![cfg(feature = "database-tests")]
use board_store::{
    media::MediaQueue,
    media_intake::{IntakeStore, PairedInputDescriptor},
};
use sqlx::PgPool;
static QUEUE_TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn typed_pair_lease_never_grants_legacy_publication() {
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
    let r = intake.reserve_pair("candidate.png").await.unwrap();
    intake
        .begin_pair_upload(&r.id, &r.capability)
        .await
        .unwrap();
    intake
        .finish_pair_upload(
            &r.id,
            &r.capability,
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
    assert!(queue.claim().await.unwrap().is_none());
    assert!(
        sqlx::query(
            "UPDATE media.jobs SET expires_at=clock_timestamp()+interval '2 hours' WHERE id=$1"
        )
        .bind(&r.id)
        .execute(&media)
        .await
        .is_err()
    );
    assert!(
        sqlx::query("UPDATE media.jobs SET attempts=1 WHERE id=$1")
            .bind(&r.id)
            .execute(&media)
            .await
            .is_err()
    );
    assert!(sqlx::query("UPDATE media.jobs SET state='processing',attempts=1,lease_token=repeat('a',32),expires_at=clock_timestamp()+interval '30 seconds' WHERE id=$1").bind(&r.id).execute(&media).await.is_err());
    let lease = queue.claim_paired_candidate().await.unwrap().unwrap();
    assert_eq!(lease.id(), r.id);
    assert_eq!(lease.input_bytes(), Some(60));
    assert_eq!(lease.image_sha256(), Some("b".repeat(64).as_str()));
    assert!(queue.claim_paired_candidate().await.unwrap().is_none());
    assert!(sqlx::query("UPDATE media.jobs SET state='queued',attempts=0,lease_token=NULL,expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1").bind(&r.id).execute(&media).await.is_err());
    let job = queue.get(&r.id).await.unwrap();
    assert_eq!(job.state, "processing");
    assert_eq!(job.attempts, 1);
    let token = job.lease_token.unwrap();
    assert!(
        queue
            .complete(&r.id, &token, &"e".repeat(64), 20)
            .await
            .is_err()
    );
    assert!(sqlx::query("UPDATE media.jobs SET state='failed',failure='candidate_checked',lease_token=NULL,expires_at=NULL WHERE id=$1").bind(&r.id).execute(&media).await.is_err());
    let wrong: bool = sqlx::query_scalar("SELECT media.finish_paired_candidate($1,$2,true)")
        .bind(&r.id)
        .bind("f".repeat(32))
        .fetch_one(&media)
        .await
        .unwrap();
    assert!(!wrong);
    queue.finish_paired_candidate(lease, true).await.unwrap();
    let job = queue.get(&r.id).await.unwrap();
    assert_eq!(job.state, "failed");
    assert_eq!(job.failure.as_deref(), Some("candidate_checked"));
    assert!(job.output_sha256.is_none() && job.output_bytes.is_none() && job.lease_token.is_none());
    let reuse: bool = sqlx::query_scalar("SELECT media.finish_paired_candidate($1,$2,true)")
        .bind(&r.id)
        .bind(token)
        .fetch_one(&media)
        .await
        .unwrap();
    assert!(!reuse);
    assert!(queue.claim_paired_candidate().await.unwrap().is_none());
    sqlx::query("DELETE FROM media.jobs WHERE id=$1")
        .bind(&r.id)
        .execute(&admin)
        .await
        .unwrap();
    intake.close().await.unwrap();
}

#[tokio::test]
async fn expired_pair_lease_cannot_finish_and_new_attempt_is_fenced() {
    let _serial = QUEUE_TEST.lock().await;
    let intake = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let admin = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let r = intake.reserve_pair("candidate.png").await.unwrap();
    intake
        .begin_pair_upload(&r.id, &r.capability)
        .await
        .unwrap();
    intake
        .finish_pair_upload(
            &r.id,
            &r.capability,
            &PairedInputDescriptor {
                bytes: 57,
                sha256: "a".repeat(64),
                image_bytes: 1,
                image_sha256: "b".repeat(64),
                replay_bytes: None,
                replay_sha256: None,
            },
        )
        .await
        .unwrap();
    let old = queue.claim_paired_candidate().await.unwrap().unwrap();
    sqlx::query(
        "UPDATE media.jobs SET expires_at=clock_timestamp()-interval '1 second' WHERE id=$1",
    )
    .bind(&r.id)
    .execute(&admin)
    .await
    .unwrap();
    queue.expire().await.unwrap();
    assert_eq!(queue.get(&r.id).await.unwrap().state, "queued");
    let new = queue.claim_paired_candidate().await.unwrap().unwrap();
    assert!(queue.finish_paired_candidate(old, true).await.is_err());
    queue.finish_paired_candidate(new, false).await.unwrap();
    let j = queue.get(&r.id).await.unwrap();
    assert_eq!(j.attempts, 2);
    assert_eq!(j.failure.as_deref(), Some("processing_failed"));
    sqlx::query("DELETE FROM media.jobs WHERE id=$1")
        .bind(&r.id)
        .execute(&admin)
        .await
        .unwrap();
    intake.close().await.unwrap();
}
