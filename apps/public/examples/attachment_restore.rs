#![forbid(unsafe_code)]
//! Owned restore fixture, never a runtime service. Receives test role credentials
//! and synthetic pixels; it does not decode uploads or qualify worker isolation.
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use board_media::{ApprovedFiles, PublicationStore, Quarantine, ValidatedOutput};
use board_store::{
    media::MediaQueue,
    media_assets::MediaReader,
    media_intake::{IntakeReservation, IntakeStore},
    post_media::NewAttachment,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};
use tower::ServiceExt;

#[derive(Serialize, Deserialize)]
struct Record {
    job: String,
    capability: String,
    asset: String,
    post: i64,
    thread: i64,
    tim: i64,
    live: bool,
    retire: bool,
}
#[derive(Serialize, Deserialize)]
struct Manifest {
    records: Vec<Record>,
    files: BTreeMap<String, String>,
    threads: BTreeMap<i64, Value>,
    clock: i64,
}
struct Fixture {
    admin: PgPool,
    public: PgPool,
    queue: MediaQueue,
    intake: IntakeStore,
    root: PathBuf,
    store: PublicationStore,
}
impl Fixture {
    async fn open(root: &Path) -> Self {
        let quarantine = Quarantine::new(root.join("quarantine")).unwrap();
        Self {
            admin: PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
                .await
                .unwrap(),
            public: board_store::connect_public(
                &std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap(),
            )
            .await
            .unwrap(),
            queue: MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
                .await
                .unwrap(),
            intake: IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
                .await
                .unwrap(),
            root: root.to_owned(),
            store: PublicationStore::new(root.join("objects"), &quarantine).unwrap(),
        }
    }
    async fn add(&self, parent: i64, spoiler: bool) -> Record {
        let receipt = self
            .intake
            .reserve("<restore & fixture>.png")
            .await
            .unwrap();
        self.intake
            .begin_upload(&receipt.id, &receipt.capability)
            .await
            .unwrap();
        self.intake
            .finish_upload(&receipt.id, &receipt.capability, 100)
            .await
            .unwrap();
        let job = self.queue.claim().await.unwrap().unwrap();
        assert_eq!(job.id, receipt.id, "Requires the owned empty restore queue");
        let mut frame = b"IBRGBA01".to_vec();
        frame.extend_from_slice(&500u32.to_be_bytes());
        frame.extend_from_slice(&300u32.to_be_bytes());
        frame.extend_from_slice(&[220, 20, 90, 180].repeat(500 * 300));
        let output = ValidatedOutput::read(frame.as_slice()).await.unwrap();
        let asset = board_media_admin::publish(
            &self.queue,
            &self.store,
            &job.id,
            job.lease_token.as_deref().unwrap(),
            &output,
        )
        .await
        .unwrap();
        let attachment = NewAttachment {
            upload: receipt,
            spoiler,
        };
        let post = board_store::create_post_with_attachment(
            &self.public,
            "restore",
            parent,
            &new_post(),
            Some(&attachment),
        )
        .await
        .unwrap();
        let saved = board_store::post_media::attachment(&self.public, post)
            .await
            .unwrap()
            .unwrap();
        Record {
            job: job.id,
            capability: attachment.upload.capability,
            asset: asset.id,
            post,
            thread: if parent == 0 { post } else { parent },
            tim: saved.tim,
            live: true,
            retire: false,
        }
    }
    async fn api(&self, thread: i64) -> Value {
        let app = board_public::router(self.public.clone(), "http://127.0.0.1:3000".into(), false);
        let response = app
            .oneshot(
                Request::get(format!("/restore/thread/{thread}.json"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        serde_json::from_slice(&to_bytes(response.into_body(), 262_144).await.unwrap()).unwrap()
    }
    async fn create(&self) {
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM content.boards")
                .fetch_one(&self.admin)
                .await
                .unwrap(),
            0,
            "Source must be a fresh empty restore database"
        );
        sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit,archive_retention_seconds) VALUES ('restore','Restore fixture','Synthetic only',2000,100,100,100,10,20,3600)").execute(&self.admin).await.unwrap();
        // An ahead-of-wall-clock counter catches a restore that resets the clock.
        sqlx::query("UPDATE content.media_clock SET last_number=floor(extract(epoch FROM clock_timestamp())*1000)::bigint+86400000").execute(&self.admin).await.unwrap();
        let op = self.add(0, false).await;
        let spoiler = self.add(op.thread, true).await;
        let mut deleted = self.add(op.thread, false).await;
        board_store::post_media::delete_attachment(&self.public, "restore", deleted.post)
            .await
            .unwrap();
        deleted.live = false;
        deleted.retire = true;
        let mut retired = self.add(op.thread, false).await;
        board_store::post_media::delete_attachment(&self.public, "restore", retired.post)
            .await
            .unwrap();
        let guard = self.store.try_lock().unwrap();
        assert!(self.queue.retire_output(&retired.asset).await.unwrap());
        drop(guard); // Deliberate interrupted-cleanup window: both files still exist.
        retired.live = false;
        retired.retire = true;
        let archived = self.add(0, false).await;
        sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1").bind(archived.thread).execute(&self.admin).await.unwrap();
        let mut expired = self.add(0, false).await;
        sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp()-interval '2 seconds',archive_expires_at=clock_timestamp()-interval '1 second' WHERE id=$1").bind(expired.thread).execute(&self.admin).await.unwrap();
        expired.live = false;
        expired.retire = true;
        let mut removed = self.add(op.thread, false).await;
        board_store::delete_post(&self.public, "restore", removed.post)
            .await
            .unwrap();
        removed.live = false;
        removed.retire = true;
        let mut threads = BTreeMap::new();
        threads.insert(op.thread, self.api(op.thread).await);
        threads.insert(archived.thread, self.api(archived.thread).await);
        let records = vec![op, spoiler, deleted, retired, archived, expired, removed];
        let mut files = BTreeMap::new();
        for record in &records {
            for suffix in ["png", "thumb.png"] {
                let name = format!("{}.{suffix}", record.asset);
                let bytes = std::fs::read(self.root.join("objects").join(&name)).unwrap();
                files.insert(name, format!("{:x}", Sha256::digest(bytes)));
            }
        }
        let clock = sqlx::query_scalar("SELECT last_number FROM content.media_clock")
            .fetch_one(&self.admin)
            .await
            .unwrap();
        let manifest = Manifest {
            records,
            files,
            threads,
            clock,
        };
        std::fs::write(
            self.root.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
    }
    async fn verify(&self) {
        for statement in [
            "SELECT * FROM content.post_media",
            "SELECT media.retire_output(repeat('0',32))",
            "SELECT * FROM staff_identity.credentials",
        ] {
            let error = sqlx::query(statement)
                .execute(&self.public)
                .await
                .unwrap_err();
            assert_eq!(
                error.as_database_error().unwrap().code().as_deref(),
                Some("42501"),
                "Restored public privileges changed"
            );
        }
        let manifest: Manifest =
            serde_json::from_slice(&std::fs::read(self.root.join("manifest.json")).unwrap())
                .unwrap();
        for (name, expected) in &manifest.files {
            assert!(!name.contains(['/', '\\']));
            let bytes = std::fs::read(self.root.join("objects").join(name)).unwrap();
            assert_eq!(format!("{:x}", Sha256::digest(bytes)), *expected);
        }
        for (thread, expected) in &manifest.threads {
            assert_eq!(
                self.api(*thread).await,
                *expected,
                "Restored public JSON changed"
            );
        }
        let clock: i64 = sqlx::query_scalar("SELECT last_number FROM content.media_clock")
            .fetch_one(&self.admin)
            .await
            .unwrap();
        assert_eq!(clock, manifest.clock);
        let reader = MediaReader::connect(&std::env::var("MEDIA_READ_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let origin = board_config::Origin::parse("http://127.0.0.1:3002").unwrap();
        let app = board_media_http::router(board_media_http::AppState::new(
            reader,
            ApprovedFiles::open(self.root.join("objects")).unwrap(),
            &origin,
        ));
        for record in &manifest.records {
            for (path, suffix) in [
                (format!("/media/{}.png", record.asset), "png"),
                (format!("/media/{}.thumb.png", record.asset), "thumb.png"),
                (format!("/restore/{}.png", record.tim), "png"),
                (format!("/restore/{}s.jpg", record.tim), "thumb.png"),
            ] {
                let response = app
                    .clone()
                    .oneshot(
                        Request::get(&path)
                            .header("host", "127.0.0.1:3002")
                            .header("if-none-match", "\"old-validator\"")
                            .body(Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                assert_eq!(
                    response.status().as_u16(),
                    if record.live { 200 } else { 404 },
                    "{path}"
                );
                if record.live {
                    assert_eq!(response.headers()["content-type"], "image/png");
                    let bytes = to_bytes(response.into_body(), 5_242_880).await.unwrap();
                    assert_eq!(
                        format!("{:x}", Sha256::digest(bytes)),
                        manifest.files[&format!("{}.{suffix}", record.asset)]
                    );
                }
            }
            let reused = NewAttachment {
                upload: IntakeReservation {
                    id: record.job.clone(),
                    capability: record.capability.clone(),
                },
                spoiler: false,
            };
            assert!(
                board_store::create_post_with_attachment(
                    &self.public,
                    "restore",
                    0,
                    &new_post(),
                    Some(&reused)
                )
                .await
                .is_err(),
                "Restore must not resurrect a consumed receipt"
            );
        }
        let removed = board_media_admin::reconcile(&self.queue, &self.store)
            .await
            .unwrap();
        assert_eq!(
            removed as usize,
            manifest.records.iter().filter(|r| r.retire).count()
        );
        assert_eq!(
            board_media_admin::reconcile(&self.queue, &self.store)
                .await
                .unwrap(),
            0
        );
        for record in &manifest.records {
            for suffix in ["png", "thumb.png"] {
                assert_eq!(
                    self.root
                        .join("objects")
                        .join(format!("{}.{suffix}", record.asset))
                        .is_file(),
                    record.live
                );
            }
            let count: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM content.post_media WHERE asset_id=$1 AND tim=$2",
            )
            .bind(&record.asset)
            .bind(record.tim)
            .fetch_one(&self.admin)
            .await
            .unwrap();
            assert_eq!(count, 1, "Cleanup must preserve the attachment tombstone");
        }
        let new = self.add(0, false).await;
        assert_eq!(
            new.tim,
            manifest.clock + 1,
            "Restored clock must allocate without reuse"
        );
        assert!(new.post > manifest.records.iter().map(|r| r.post).max().unwrap());
    }
}
fn new_post() -> board_store::NewPost {
    board_store::NewPost {
        name: "Anonymous".into(),
        subject: "Restore fixture".into(),
        comment: "Synthetic restore comment".into(),
        deletion_hash: "not-a-password".into(),
        sage: false,
    }
}
#[tokio::main]
async fn main() {
    assert_eq!(
        std::env::var("ATTACHMENT_RESTORE_FIXTURE").as_deref(),
        Ok("owned-disposable")
    );
    let args: Vec<_> = std::env::args().skip(1).collect();
    assert_eq!(args.len(), 2);
    let expected_suffix = match args[0].as_str() {
        "create" => "_source",
        "verify" => "_restore",
        _ => panic!("Expected create or verify"),
    };
    let mut database = None;
    for key in [
        "MIGRATION_DATABASE_URL",
        "TEST_PUBLIC_DATABASE_URL",
        "MEDIA_DATABASE_URL",
        "MEDIA_READ_DATABASE_URL",
        "INTAKE_DATABASE_URL",
    ] {
        let url = std::env::var(key).unwrap();
        let name = url.rsplit('/').next().unwrap();
        let token = name
            .strip_prefix("imageboard_attachment_")
            .and_then(|v| v.strip_suffix(expected_suffix))
            .expect("Only generated restore databases are allowed");
        assert_eq!(token.len(), 24);
        assert!(
            token
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
        if let Some(expected) = &database {
            assert_eq!(name, expected);
        } else {
            database = Some(name.to_owned());
        }
    }
    let fixture = Fixture::open(Path::new(&args[1])).await;
    match args[0].as_str() {
        "create" => fixture.create().await,
        "verify" => fixture.verify().await,
        _ => panic!("Expected create or verify"),
    }
    println!("PASS attachment restore fixture {}", args[0]);
}
