#![cfg(feature = "database-tests")]

use board_store::{
    StoreError,
    media::MediaQueue,
    media_intake::{IntakeStore, PairedInputDescriptor},
};
use sqlx::PgPool;
use std::time::Duration;

#[tokio::test]
async fn candidate_claim_skips_locked_jobs_and_finish_rechecks_expiry_after_wait() {
    let intake = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let admin = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let media = PgPool::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut ids = Vec::new();
    for _ in 0..2 {
        let r = intake.reserve_pair("candidate-lock.png").await.unwrap();
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
        ids.push(r.id);
    }
    let mut locked = admin.begin().await.unwrap();
    sqlx::query("SELECT id FROM media.jobs WHERE id=$1 FOR UPDATE")
        .bind(&ids[0])
        .execute(&mut *locked)
        .await
        .unwrap();
    let second = tokio::time::timeout(Duration::from_secs(2), queue.claim_paired_candidate())
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(second.id(), ids[1]);
    assert!(queue.claim_paired_candidate().await.unwrap().is_none());
    locked.commit().await.unwrap();
    queue.finish_paired_candidate(second, false).await.unwrap();
    let first = queue.claim_paired_candidate().await.unwrap().unwrap();
    assert_eq!(first.id(), ids[0]);
    let old_token = queue.get(&ids[0]).await.unwrap().lease_token.unwrap();
    let mut locked = admin.begin().await.unwrap();
    sqlx::query("SELECT id FROM media.jobs WHERE id=$1 FOR UPDATE")
        .bind(&ids[0])
        .execute(&mut *locked)
        .await
        .unwrap();
    let xid: String = sqlx::query_scalar("SELECT txid_current()::text")
        .fetch_one(&mut *locked)
        .await
        .unwrap();
    let worker = queue.clone();
    let pending = tokio::spawn(async move { worker.finish_paired_candidate(first, true).await });
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let blocked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE locktype='transactionid' AND transactionid=$1::xid AND NOT granted)")
                .bind(&xid).fetch_one(&admin).await.unwrap();
            if blocked { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("Candidate finish must wait for the held job row");
    assert!(!pending.is_finished());
    sqlx::query(
        "UPDATE media.jobs SET expires_at=clock_timestamp()-interval '1 second' WHERE id=$1",
    )
    .bind(&ids[0])
    .execute(&mut *locked)
    .await
    .unwrap();
    locked.commit().await.unwrap();
    assert!(matches!(
        pending.await.unwrap(),
        Err(StoreError::Conflict(_))
    ));
    let current = queue.get(&ids[0]).await.unwrap();
    assert_eq!(current.state, "processing");
    assert!(current.output_sha256.is_none() && current.output_bytes.is_none());
    assert_eq!(queue.expire().await.unwrap(), 1);
    let retry = queue.claim_paired_candidate().await.unwrap().unwrap();
    assert_eq!(retry.id(), ids[0]);
    let stale: bool = sqlx::query_scalar("SELECT media.finish_paired_candidate($1,$2,true)")
        .bind(&ids[0])
        .bind(old_token)
        .fetch_one(&media)
        .await
        .unwrap();
    assert!(!stale);
    queue.finish_paired_candidate(retry, true).await.unwrap();
    let checked = queue.get(&ids[0]).await.unwrap();
    assert_eq!(checked.state, "failed");
    assert_eq!(checked.attempts, 2);
    assert_eq!(checked.failure.as_deref(), Some("candidate_checked"));
    assert!(
        checked.output_sha256.is_none()
            && checked.output_bytes.is_none()
            && checked.lease_token.is_none()
    );
    let assets: i64 = sqlx::query_scalar("SELECT count(*) FROM media.assets WHERE job_id=ANY($1)")
        .bind(&ids)
        .fetch_one(&admin)
        .await
        .unwrap();
    assert_eq!(
        assets, 0,
        "Candidate checks never create approval or assets"
    );
    sqlx::query("DELETE FROM media.jobs WHERE id=ANY($1)")
        .bind(&ids)
        .execute(&admin)
        .await
        .unwrap();
    intake.close().await.unwrap();
    admin.close().await;
    media.close().await;
}
