#![cfg(feature = "database-tests")]

#[path = "support/dispatch_cases.rs"]
mod dispatch_cases;

use board_media::{ApprovedFiles, ObjectId, PublicationStore, Quarantine, ValidatedOutput};
use board_media_admin::{publish, read_approved, reconcile};
use board_store::{
    media::MediaQueue,
    media_assets::{MediaReader, OutputMetadata, OutputVariants},
};
use std::{
    process::Command,
    sync::{Arc, Mutex},
};

async fn output(red: u8) -> ValidatedOutput {
    let mut frame = b"IBRGBA01\0\0\0\x01\0\0\0\x01".to_vec();
    frame.extend_from_slice(&[red, 0, 0, 255]);
    ValidatedOutput::read(frame.as_slice()).await.unwrap()
}

async fn lease(queue: &MediaQueue, ids: &Mutex<Vec<String>>) -> board_store::media::Job {
    let job = queue
        .reserve("approval integration owned fixture")
        .await
        .unwrap();
    ids.lock().unwrap().push(job.id.clone());
    queue.queue(&job.id, 4).await.unwrap();
    let claimed = queue.claim().await.unwrap().unwrap();
    assert_eq!(claimed.id, job.id, "Use an idle disposable queue");
    claimed
}

async fn expire(admin: &sqlx::PgPool, id: &str) {
    sqlx::query(
        "UPDATE media.jobs SET expires_at=clock_timestamp()-interval '1 second' WHERE id=$1",
    )
    .bind(id)
    .execute(admin)
    .await
    .unwrap();
}

#[tokio::test]
async fn publication_gates_reads_fences_expired_leases_and_recovers_every_file_window() {
    let admin = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let ids = Arc::new(Mutex::new(Vec::new()));
    let task_ids = ids.clone();
    let task_admin = admin.clone();
    let result = tokio::spawn(async move { exercise(task_admin, task_ids).await }).await;
    let ids = ids.lock().unwrap().clone();
    sqlx::query("DELETE FROM media.assets WHERE job_id=ANY($1)")
        .bind(&ids)
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("DELETE FROM media.jobs WHERE id=ANY($1)")
        .bind(&ids)
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    result.unwrap();
}

async fn exercise(admin: sqlx::PgPool, ids: Arc<Mutex<Vec<String>>>) {
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let reader = MediaReader::connect(&std::env::var("MEDIA_READ_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(temp.path().join("quarantine")).unwrap();
    let root = temp.path().join("objects");
    let store = PublicationStore::new(&root, &quarantine).unwrap();
    let files = ApprovedFiles::open(&root).unwrap();
    let pixels = output(5).await;
    let encoded = pixels.encode().unwrap();
    let metadata = OutputMetadata {
        sha256: encoded.sha256().to_owned(),
        bytes: encoded.len() as i64,
        width: 1,
        height: 1,
    };
    let job = lease(&queue, &ids).await;
    let token = job.lease_token.as_deref().unwrap();
    let guard = store.try_lock().unwrap();
    let pending = queue
        .prepare_output(&job.id, token, &metadata)
        .await
        .unwrap();
    assert_ne!(pending.id, job.id);
    assert_ne!(pending.id, token);
    assert!(read_approved(&reader, &files, &pending.id).await.is_err());
    guard
        .install(pending.id.parse().unwrap(), &encoded)
        .unwrap();
    assert!(root.join(format!("{}.png", pending.id)).is_file());
    assert!(read_approved(&reader, &files, &pending.id).await.is_err());
    assert!(reconcile(&queue, &store).await.is_err());
    expire(&admin, &job.id).await;
    assert!(
        queue
            .approve_output(&job.id, token, &pending.id)
            .await
            .is_err()
    );
    drop(guard);
    assert_eq!(reconcile(&queue, &store).await.unwrap(), 1);
    assert!(!root.join(format!("{}.png", pending.id)).exists());
    assert!(
        publish(&queue, &store, &job.id, token, &pixels)
            .await
            .is_err()
    );
    queue.expire().await.unwrap();
    let retry = queue.claim().await.unwrap().unwrap();
    assert_eq!(retry.id, job.id);
    let retry_token = retry.lease_token.as_deref().unwrap();
    let approved = publish(&queue, &store, &job.id, retry_token, &pixels)
        .await
        .unwrap();
    assert_ne!(approved.id, pending.id);
    assert_eq!(
        publish(&queue, &store, &job.id, retry_token, &pixels)
            .await
            .unwrap(),
        approved
    );
    assert!(
        publish(&queue, &store, &job.id, token, &pixels)
            .await
            .is_err()
    );
    assert!(
        publish(&queue, &store, &job.id, retry_token, &output(6).await)
            .await
            .is_err()
    );
    let bytes = read_approved(&reader, &files, &approved.id).await.unwrap();
    assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    let thumbnail = reader.get_thumbnail(&approved.id).await.unwrap();
    assert_eq!(
        files
            .read_thumbnail(
                approved.id.parse().unwrap(),
                &thumbnail.sha256,
                thumbnail.bytes as u64
            )
            .unwrap(),
        bytes
    );
    let mut changed_variants = OutputVariants {
        md5: "00".repeat(16),
        thumbnail: metadata.clone(),
    };
    assert!(
        queue
            .prepare_output_with_variants(&job.id, retry_token, &metadata, Some(&changed_variants))
            .await
            .is_err()
    );
    changed_variants.md5 = encoded.md5().to_owned();
    changed_variants.thumbnail.width = 2;
    assert!(
        queue
            .prepare_output_with_variants(&job.id, retry_token, &metadata, Some(&changed_variants))
            .await
            .is_err()
    );
    sqlx::query("UPDATE media.jobs SET updated_at=clock_timestamp()-interval '2 days' WHERE id=$1")
        .bind(&job.id)
        .execute(&admin)
        .await
        .unwrap();
    assert!(queue.forget_terminal(&job.id).await.unwrap());
    assert_eq!(
        read_approved(&reader, &files, &approved.id).await.unwrap(),
        bytes
    );
    assert_eq!(reconcile(&queue, &store).await.unwrap(), 0);
    assert_eq!(
        publish(&queue, &store, &job.id, retry_token, &pixels)
            .await
            .unwrap(),
        approved
    );
    let approved_path = root.join(format!("{}.png", approved.id));
    std::fs::write(&approved_path, vec![0; bytes.len()]).unwrap();
    assert!(read_approved(&reader, &files, &approved.id).await.is_err());
    std::fs::remove_file(approved_path).unwrap();
    assert!(read_approved(&reader, &files, &approved.id).await.is_err());

    // A failed thumbnail installation cannot approve an otherwise complete full image.
    let partial_job = lease(&queue, &ids).await;
    let partial_token = partial_job.lease_token.as_deref().unwrap();
    let variants = OutputVariants {
        md5: encoded.md5().to_owned(),
        thumbnail: metadata.clone(),
    };
    let partial = queue
        .prepare_output_with_variants(&partial_job.id, partial_token, &metadata, Some(&variants))
        .await
        .unwrap();
    let obstacle = root.join(format!("{}.thumb.png", partial.id));
    std::fs::create_dir(&obstacle).unwrap();
    assert!(
        publish(&queue, &store, &partial_job.id, partial_token, &pixels)
            .await
            .is_err()
    );
    assert!(root.join(format!("{}.png", partial.id)).is_file());
    assert!(reader.get(&partial.id).await.is_err());
    assert!(reader.get_thumbnail(&partial.id).await.is_err());
    std::fs::remove_dir(&obstacle).unwrap();
    assert_eq!(
        publish(&queue, &store, &partial_job.id, partial_token, &pixels)
            .await
            .unwrap(),
        partial
    );
    assert!(reader.get_thumbnail(&partial.id).await.is_ok());
    assert!(
        !queue.retire_output(&partial.id).await.unwrap(),
        "Recent unused approval must survive"
    );
    sqlx::query(
        "UPDATE media.assets SET approved_at=clock_timestamp()-interval '2 days' WHERE id=$1",
    )
    .bind(&partial.id)
    .execute(&admin)
    .await
    .unwrap();
    // Crash after durable retirement, before either file is removed.
    let retention_guard = store.try_lock().unwrap();
    assert!(queue.retire_output(&partial.id).await.unwrap());
    assert!(reader.get(&partial.id).await.is_err());
    assert!(reader.get_thumbnail(&partial.id).await.is_err());
    assert!(root.join(format!("{}.png", partial.id)).is_file());
    assert!(root.join(format!("{}.thumb.png", partial.id)).is_file());
    drop(retention_guard);
    assert_eq!(reconcile(&queue, &store).await.unwrap(), 1);
    assert!(!root.join(format!("{}.png", partial.id)).exists());
    assert!(!root.join(format!("{}.thumb.png", partial.id)).exists());
    assert_eq!(reconcile(&queue, &store).await.unwrap(), 0);

    // The normal reconciliation path discovers and removes an aged orphan.
    let orphan_job = lease(&queue, &ids).await;
    let orphan = publish(
        &queue,
        &store,
        &orphan_job.id,
        orphan_job.lease_token.as_deref().unwrap(),
        &pixels,
    )
    .await
    .unwrap();
    sqlx::query(
        "UPDATE media.assets SET approved_at=clock_timestamp()-interval '2 days' WHERE id=$1",
    )
    .bind(&orphan.id)
    .execute(&admin)
    .await
    .unwrap();
    let blocked_thumbnail = root.join(format!("{}.thumb.png", orphan.id));
    std::fs::remove_file(&blocked_thumbnail).unwrap();
    std::fs::create_dir(&blocked_thumbnail).unwrap();
    assert!(reconcile(&queue, &store).await.is_err());
    assert!(!root.join(format!("{}.png", orphan.id)).exists());
    assert!(
        blocked_thumbnail.is_dir(),
        "Cleanup must not recursively remove an unexpected object"
    );
    let state: String = sqlx::query_scalar("SELECT state FROM media.assets WHERE id=$1")
        .bind(&orphan.id)
        .fetch_one(&admin)
        .await
        .unwrap();
    assert_eq!(state, "deleting");
    assert!(reader.get(&orphan.id).await.is_err());
    std::fs::remove_dir(&blocked_thumbnail).unwrap();
    assert_eq!(reconcile(&queue, &store).await.unwrap(), 1);
    assert!(!root.join(format!("{}.png", orphan.id)).exists());
    assert!(!root.join(format!("{}.thumb.png", orphan.id)).exists());

    for window in [
        "part",
        "installed",
        "thumbnail-part",
        "thumbnail-installed",
        "deleting",
        "removed",
    ] {
        let job = lease(&queue, &ids).await;
        let guard = store.try_lock().unwrap();
        let pending = queue
            .prepare_output(&job.id, job.lease_token.as_deref().unwrap(), &metadata)
            .await
            .unwrap();
        let id: ObjectId = pending.id.parse().unwrap();
        if window == "part" {
            std::fs::write(root.join(format!("{id}.part")), b"interrupted").unwrap();
        } else {
            guard.install(id, &encoded).unwrap();
        }
        if window == "thumbnail-part" {
            std::fs::write(root.join(format!("{id}.thumb.part")), b"interrupted").unwrap();
        } else if ["thumbnail-installed", "deleting", "removed"].contains(&window) {
            guard
                .install_thumbnail(id, &pixels.thumbnail().unwrap())
                .unwrap();
        }
        expire(&admin, &job.id).await;
        if ["deleting", "removed"].contains(&window) {
            assert!(queue.begin_output_deletion(&pending.id).await.unwrap());
        }
        if window == "removed" {
            guard.remove(id).unwrap();
        }
        drop(guard);
        assert!(read_approved(&reader, &files, &pending.id).await.is_err());
        assert_eq!(
            reconcile(&queue, &store).await.unwrap(),
            1,
            "window {window}"
        );
        assert_eq!(reconcile(&queue, &store).await.unwrap(), 0);
        assert!(!root.join(format!("{id}.part")).exists());
        assert!(!root.join(format!("{id}.png")).exists());
        assert!(!root.join(format!("{id}.thumb.part")).exists());
        assert!(!root.join(format!("{id}.thumb.png")).exists());
        // Retire this fixture directly as owner; do not requeue it ahead of the next case.
        sqlx::query("UPDATE media.jobs SET state='failed',lease_token=NULL,expires_at=NULL,failure='abandoned' WHERE id=$1").bind(&job.id).execute(&admin).await.unwrap();
    }
    exercise_commands(&queue, &ids, temp.path(), &root).await;
    dispatch_cases::exercise(&queue, &admin, &ids).await;
}

fn command(binary: &str, role: &str) -> Command {
    let mut command = Command::new(binary);
    command
        .env_clear()
        .env("APP_ENV", "development")
        .env(role, std::env::var(role).unwrap());
    if let Some(value) = std::env::var_os("SystemRoot") {
        command.env("SystemRoot", value);
    }
    command
}

async fn exercise_commands(
    queue: &MediaQueue,
    ids: &Mutex<Vec<String>>,
    temp: &std::path::Path,
    root: &std::path::Path,
) {
    let job = queue.reserve("operator publication fixture").await.unwrap();
    ids.lock().unwrap().push(job.id.clone());
    queue.queue(&job.id, 4).await.unwrap();
    let manifest = temp.join("lease.json");
    let claim = command(env!("CARGO_BIN_EXE_media-publish"), "MEDIA_DATABASE_URL")
        .arg("claim")
        .arg(&manifest)
        .output()
        .unwrap();
    assert!(
        claim.status.success(),
        "{}",
        String::from_utf8_lossy(&claim.stderr)
    );
    assert!(claim.stdout.is_empty());
    let manifest_data: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest).unwrap()).unwrap();
    assert_eq!(manifest_data["job_id"], job.id);
    let disk = temp.join("quarantine").join("stopped-output.disk");
    std::fs::write(&disk, b"IBRGBA01\0\0\0\x01\0\0\0\x01\xff\0\0\xff").unwrap();
    std::fs::OpenOptions::new()
        .write(true)
        .open(&disk)
        .unwrap()
        .set_len(board_media::OUTPUT_DISK_BYTES)
        .unwrap();
    let published = command(env!("CARGO_BIN_EXE_media-publish"), "MEDIA_DATABASE_URL")
        .env("MEDIA_QUARANTINE_DIR", temp.join("quarantine"))
        .arg("publish")
        .arg(&manifest)
        .arg(&disk)
        .arg(root)
        .output()
        .unwrap();
    assert!(
        published.status.success(),
        "{}",
        String::from_utf8_lossy(&published.stderr)
    );
    let id = String::from_utf8(published.stdout).unwrap();
    let id = id.trim();
    assert!(id.parse::<ObjectId>().is_ok());
    assert_ne!(id, job.id);
    let destination = temp.join("approved-export.png");
    let read = command(env!("CARGO_BIN_EXE_media-read"), "MEDIA_READ_DATABASE_URL")
        .arg(id)
        .arg(root)
        .arg(&destination)
        .output()
        .unwrap();
    assert!(
        read.status.success(),
        "{}",
        String::from_utf8_lossy(&read.stderr)
    );
    assert!(std::fs::read(&destination).unwrap().starts_with(b"\x89PNG"));
    let invalid_destination = temp.join("unapproved-export.png");
    let denied = command(env!("CARGO_BIN_EXE_media-read"), "MEDIA_READ_DATABASE_URL")
        .arg(ObjectId::generate().unwrap().to_string())
        .arg(root)
        .arg(&invalid_destination)
        .output()
        .unwrap();
    assert!(!denied.status.success());
    assert!(!invalid_destination.exists());
}
