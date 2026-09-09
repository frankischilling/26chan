#![cfg(feature = "database-tests")]

use board_store::{
    StoreError,
    media::{Failure, MediaQueue},
};
use sqlx::{Connection, Executor, PgConnection};

#[tokio::test]
async fn queue_enforces_admission_claim_fencing_retries_and_terminal_cleanup() {
    let url = std::env::var("MEDIA_DATABASE_URL").expect("MEDIA_DATABASE_URL is required");
    let queue = MediaQueue::connect(&url).await.unwrap();
    let admin_url = std::env::var("MIGRATION_DATABASE_URL").unwrap();
    let admin = sqlx::PgPool::connect(&admin_url).await.unwrap();
    let fixture: String = sqlx::query_scalar("SELECT gen_random_uuid()::text")
        .fetch_one(&admin)
        .await
        .unwrap();
    let task_admin = admin.clone();
    let task_fixture = fixture.clone();
    let result =
        tokio::spawn(async move { exercise_queue(queue, task_admin, task_fixture).await }).await;
    sqlx::query("DELETE FROM media.jobs WHERE filename = $1")
        .bind(&fixture)
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    result.unwrap();
}

async fn exercise_queue(queue: MediaQueue, admin: sqlx::PgPool, fixture: String) {
    let pending: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM media.jobs WHERE state IN ('receiving', 'queued', 'processing')",
    )
    .fetch_one(&admin)
    .await
    .unwrap();
    assert_eq!(
        pending, 0,
        "Use an idle disposable media queue for this test; it never deletes someone else's jobs."
    );
    let mut ids = Vec::new();
    for _ in 0..63 {
        ids.push(queue.reserve(&fixture).await.unwrap().id);
    }
    let mut racing = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let queue = queue.clone();
        let fixture = fixture.clone();
        racing.spawn(async move { queue.reserve(&fixture).await });
    }
    let mut full = 0;
    while let Some(result) = racing.join_next().await {
        match result.unwrap() {
            Ok(job) => ids.push(job.id),
            Err(StoreError::Conflict(_)) => full += 1,
            Err(error) => panic!("{error}"),
        }
    }
    assert_eq!(ids.len(), 64);
    assert_eq!(full, 7);
    let id = ids[0].clone();
    assert!(queue.queue(&id, 0).await.is_err());
    assert!(queue.queue(&id, 8_388_609).await.is_err());
    queue.queue(&id, 7).await.unwrap();
    assert!(queue.queue(&id, 8).await.is_err());
    let mut claims = tokio::task::JoinSet::new();
    for _ in 0..8 {
        let queue = queue.clone();
        claims.spawn(async move { queue.claim().await.unwrap() });
    }
    let mut claimed = Vec::new();
    while let Some(result) = claims.join_next().await {
        if let Some(job) = result.unwrap() {
            claimed.push(job);
        }
    }
    assert_eq!(claimed.len(), 1);
    let first = claimed.pop().unwrap();
    assert_eq!(first.id, id);
    assert_eq!(first.attempts, 1);
    let first_token = first.lease_token.unwrap();
    assert!(
        queue
            .complete(&id, &"0".repeat(32), &"a".repeat(64), 100)
            .await
            .is_err()
    );
    assert!(
        queue
            .complete(&id, &first_token, "invalid", 100)
            .await
            .is_err()
    );
    sqlx::query(
        "UPDATE media.jobs SET expires_at = clock_timestamp() - interval '1 second' WHERE id = $1",
    )
    .bind(&id)
    .execute(&admin)
    .await
    .unwrap();
    assert!(
        queue
            .complete(&id, &first_token, &"a".repeat(64), 100)
            .await
            .is_err()
    );
    assert!(
        queue
            .fail(&id, &first_token, Failure::Processing, true)
            .await
            .is_err()
    );
    assert_eq!(queue.expire().await.unwrap(), 1);
    let second = queue.claim().await.unwrap().unwrap();
    assert_eq!(second.id, id);
    assert_eq!(second.attempts, 2);
    let second_token = second.lease_token.unwrap();
    assert_ne!(first_token, second_token);
    assert!(
        queue
            .complete(&id, &first_token, &"a".repeat(64), 100)
            .await
            .is_err()
    );
    queue
        .fail(&id, &second_token, Failure::Processing, true)
        .await
        .unwrap();
    let third = queue.claim().await.unwrap().unwrap();
    assert_eq!(third.attempts, 3);
    queue
        .fail(
            &id,
            third.lease_token.as_deref().unwrap(),
            Failure::Processing,
            true,
        )
        .await
        .unwrap();
    assert!(queue.claim().await.unwrap().is_none());
    assert_eq!(queue.get(&id).await.unwrap().state, "failed");
    assert_eq!(
        queue.get(&id).await.unwrap().failure.as_deref(),
        Some("retry_exhausted")
    );

    let success = &ids[1];
    queue.queue(success, 4).await.unwrap();
    let job = queue.claim().await.unwrap().unwrap();
    let token = job.lease_token.unwrap();
    queue
        .complete(success, &token, &"b".repeat(64), 120)
        .await
        .unwrap();
    queue
        .complete(success, &token, &"b".repeat(64), 120)
        .await
        .unwrap();
    assert!(
        queue
            .complete(success, &token, &"c".repeat(64), 120)
            .await
            .is_err()
    );
    let published = queue.get(success).await.unwrap();
    assert_eq!(published.state, "published");
    assert_eq!(published.output_bytes, Some(120));

    queue.abort_intake(&ids[2]).await.unwrap();
    assert!(queue.queue(&ids[2], 1).await.is_err());
    sqlx::query("UPDATE media.jobs SET expires_at = clock_timestamp() - interval '1 second' WHERE id = ANY($1) AND state = 'receiving'").bind(&ids).execute(&admin).await.unwrap();
    assert_eq!(queue.expire().await.unwrap(), 61);
    assert_eq!(
        queue.get(&ids[3]).await.unwrap().failure.as_deref(),
        Some("abandoned")
    );
    // Cleanup first returns a bounded set; records are forgotten only after the caller removes input.
    sqlx::query("UPDATE media.jobs SET updated_at = clock_timestamp() - interval '2 days' WHERE id = ANY($1)").bind(&ids).execute(&admin).await.unwrap();
    let cleanup = queue.cleanup_candidates().await.unwrap();
    assert_eq!(cleanup.len(), 64);
    for candidate in cleanup {
        assert!(ids.contains(&candidate.id));
        queue.forget_terminal(&candidate.id).await.unwrap();
    }
    let remaining: i64 = sqlx::query_scalar("SELECT count(*) FROM media.jobs WHERE id = ANY($1)")
        .bind(&ids)
        .fetch_one(&admin)
        .await
        .unwrap();
    assert_eq!(remaining, 0);
    assert!(queue.reserve("").await.is_err());
    assert!(queue.reserve(&"x".repeat(256)).await.is_err());
    assert!(
        MediaQueue::connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
            .await
            .is_err()
    );
}

#[tokio::test]
async fn media_role_requires_separate_queue_schema_and_denies_content_authority() {
    let url = std::env::var("MEDIA_DATABASE_URL").expect("MEDIA_DATABASE_URL is required");
    let mut media = PgConnection::connect(&url).await.unwrap();
    let capacity: i32 =
        sqlx::query_scalar("SELECT capacity FROM media.queue_policy WHERE singleton")
            .fetch_one(&mut media)
            .await
            .unwrap();
    assert_eq!(capacity, 64);
    for query in [
        "SELECT * FROM content.posts",
        "SELECT * FROM post_secrets.deletion",
        "SELECT * FROM staff_identity.accounts",
        "SELECT * FROM staff_identity.credentials",
        "SELECT * FROM deployment.settings",
        "UPDATE staff_identity.accounts SET role = 'admin'",
        "CREATE SCHEMA media_role_must_not_create",
        "SET ROLE board_migrator",
        "UPDATE media.queue_policy SET capacity = 1000",
    ] {
        let error = media.execute(query).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501"),
            "{query}"
        );
    }
    let public_url = std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap();
    let mut public = PgConnection::connect(&public_url).await.unwrap();
    public
        .execute("SELECT slug FROM content.boards LIMIT 1")
        .await
        .unwrap();
    for query in [
        "SELECT * FROM media.jobs",
        "INSERT INTO media.jobs (id, filename) VALUES ('00000000000000000000000000000000', 'x')",
    ] {
        let error = public.execute(query).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501")
        );
    }
}
