#![cfg(feature = "database-tests")]

#[path = "support/posting.rs"]
mod posting;

use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use board_media::{ApprovedFiles, PublicationStore, Quarantine, ValidatedOutput};
use board_store::{
    NewPost, StoreError,
    media::MediaQueue,
    media_assets::{MediaReader, OutputMetadata, OutputVariants},
    media_intake::IntakeStore,
    post_media::NewAttachment,
};
use rand_core::OsRng;
use sqlx::PgPool;
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

const ORIGIN: &str = "http://127.0.0.1:3000";
const PASSWORD: &str = "owned-fresh-erasure-password";

async fn get(app: &Router, path: &str, etag: Option<&str>) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut request = Request::get(path).header("host", "127.0.0.1:3002");
    if let Some(etag) = etag {
        request = request.header("if-none-match", etag);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let (parts, body) = response.into_parts();
    (
        parts.status,
        parts.headers,
        to_bytes(body, 8 * 1024 * 1024).await.unwrap().to_vec(),
    )
}

async fn delete(web: &Router, board: &str, id: i64, file_only: bool) {
    let response = web
        .clone()
        .oneshot(
            Request::post(format!("/{board}/delete"))
                .header("origin", ORIGIN)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(format!(
                    "no={id}&password={PASSWORD}&file_only={file_only}"
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert_eq!(
        status,
        StatusCode::SEE_OTHER,
        "{}",
        String::from_utf8_lossy(&body)
    );
}

struct Fixture {
    owner: PgPool,
    public: PgPool,
    queue: MediaQueue,
    intake: IntakeStore,
    board: String,
    jobs: Arc<Mutex<Vec<String>>>,
    hash: String,
}

impl Fixture {
    async fn post(
        &self,
        parent: i64,
        comment: &str,
        attachment: Option<&NewAttachment>,
    ) -> Result<i64, StoreError> {
        posting::create_post_with_attachment(
            &self.public,
            &self.board,
            parent,
            &NewPost {
                name: "Owned synthetic author".into(),
                subject: "Owned erasure fixture".into(),
                comment: comment.into(),
                deletion_hash: self.hash.clone(),
                sage: false,
            },
            attachment,
        )
        .await
    }

    async fn image(&self, store: &PublicationStore) -> (NewAttachment, String) {
        let upload = self.intake.reserve("owned-erasure.png").await.unwrap();
        self.jobs.lock().unwrap().push(upload.id.clone());
        self.intake
            .begin_upload(&upload.id, &upload.capability)
            .await
            .unwrap();
        self.intake
            .finish_upload(&upload.id, &upload.capability, 4)
            .await
            .unwrap();
        let claim = self.queue.claim().await.unwrap().unwrap();
        assert_eq!(claim.id, upload.id, "Use an idle disposable queue");
        let token = claim.lease_token.unwrap();
        let mut pixels = b"IBRGBA01\0\0\0\x01\0\0\0\x01".to_vec();
        pixels.extend_from_slice(&[40, 80, 120, 255]);
        let output = ValidatedOutput::read(pixels.as_slice())
            .await
            .unwrap()
            .encode()
            .unwrap();
        let metadata = || OutputMetadata {
            sha256: output.sha256().into(),
            bytes: output.len() as i64,
            width: 1,
            height: 1,
        };
        let asset = self
            .queue
            .prepare_output_with_variants(
                &upload.id,
                &token,
                &metadata(),
                Some(&OutputVariants {
                    md5: output.md5().into(),
                    thumbnail: metadata(),
                }),
            )
            .await
            .unwrap();
        let guard = store.try_lock().unwrap();
        guard.install(asset.id.parse().unwrap(), &output).unwrap();
        guard
            .install_thumbnail(asset.id.parse().unwrap(), &output)
            .unwrap();
        self.queue
            .approve_output(&upload.id, &token, &asset.id)
            .await
            .unwrap();
        (
            NewAttachment {
                upload,
                spoiler: false,
            },
            asset.id,
        )
    }

    async fn media_paths(&self, post: i64, asset: &str) -> Vec<String> {
        let tim: i64 = sqlx::query_scalar("SELECT tim FROM content.post_media WHERE post_id=$1")
            .bind(post)
            .fetch_one(&self.owner)
            .await
            .unwrap();
        vec![
            format!("/media/{asset}.png"),
            format!("/media/{asset}.thumb.png"),
            format!("/{}/{tim}.png", self.board),
            format!("/{}/{tim}s.jpg", self.board),
        ]
    }
}

async fn exercise(f: Fixture) {
    let temp = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(temp.path().join("quarantine")).unwrap();
    let root = temp.path().join("objects");
    let store = PublicationStore::new(&root, &quarantine).unwrap();
    let reader = MediaReader::connect(&std::env::var("MEDIA_READ_DATABASE_URL").unwrap())
        .await
        .unwrap();
    reader.ready().await.unwrap();
    let media = board_media_http::router(board_media_http::AppState::new(
        reader.clone(),
        ApprovedFiles::open(&root).unwrap(),
        &board_config::Origin::parse("http://127.0.0.1:3002").unwrap(),
    ));
    let (web, api) = posting::routers(f.public.clone(), &f.board, ORIGIN.into(), false);
    let (op_upload, op_asset) = f.image(&store).await;
    let op = f
        .post(0, "ownedopsecretneedle", Some(&op_upload))
        .await
        .unwrap();
    let (reply_upload, reply_asset) = f.image(&store).await;
    let reply = f
        .post(op, "ownedreplysecretneedle", Some(&reply_upload))
        .await
        .unwrap();
    let (live_upload, live_asset) = f.image(&store).await;
    let live = f
        .post(
            op,
            &format!("Surviving same-thread quote >>{reply}"),
            Some(&live_upload),
        )
        .await
        .unwrap();
    let (archive_upload, archive_asset) = f.image(&store).await;
    let cross = f
        .post(
            0,
            &format!("Surviving cross-thread quote >>{reply} >>{op}"),
            Some(&archive_upload),
        )
        .await
        .unwrap();
    let reply_paths = f.media_paths(reply, &reply_asset).await;
    let op_paths = f.media_paths(op, &op_asset).await;
    let live_paths = f.media_paths(live, &live_asset).await;
    let archive_paths = f.media_paths(cross, &archive_asset).await;
    let mut media_tags = Vec::new();
    for path in &reply_paths {
        let (status, headers, body) = get(&media, path, None).await;
        assert_eq!(status, StatusCode::OK, "{path}");
        assert!(!body.is_empty());
        media_tags.push(headers["etag"].to_str().unwrap().to_owned());
    }
    for path in op_paths.iter().chain(&live_paths).chain(&archive_paths) {
        assert_eq!(get(&media, path, None).await.0, StatusCode::OK, "{path}");
    }
    // Replay publication is not enabled. Neither the numeric nor opaque spelling
    // may expose sidecar bytes before or after a deletion.
    let reply_tim: i64 = sqlx::query_scalar("SELECT tim FROM content.post_media WHERE post_id=$1")
        .bind(reply)
        .fetch_one(&f.owner)
        .await
        .unwrap();
    let replay_paths = [
        format!("/media/{reply_asset}.tgkr"),
        format!("/{}/{reply_tim}.tgkr", f.board),
    ];
    for path in &replay_paths {
        assert_eq!(get(&media, path, None).await.0, StatusCode::NOT_FOUND);
    }
    let representations = [
        format!("/{}/thread/{op}.json", f.board),
        format!("/{}/thread/{cross}.json", f.board),
        format!("/{}/1.json", f.board),
        format!("/{}/catalog.json", f.board),
    ];
    let mut tags = Vec::new();
    for app in [&web, &api] {
        for path in &representations {
            let (status, headers, _) = get(app, path, None).await;
            assert_eq!(status, StatusCode::OK);
            tags.push(headers["etag"].to_str().unwrap().to_owned());
        }
    }
    delete(&web, &f.board, reply, false).await;
    let erased: (bool, String, String, String, bool) = sqlx::query_as("SELECT content_erased,name,subject,comment,EXISTS(SELECT 1 FROM post_secrets.deletion d WHERE d.post_id=p.id) FROM content.posts p WHERE id=$1")
        .bind(reply).fetch_one(&f.owner).await.unwrap();
    assert_eq!(
        erased,
        (true, String::new(), String::new(), String::new(), false)
    );
    // The commit revokes reads before asynchronous byte cleanup, including a
    // conditional request carrying an ETag from the formerly readable object.
    assert!(root.join(format!("{reply_asset}.png")).is_file());
    assert!(root.join(format!("{reply_asset}.thumb.png")).is_file());
    for (path, tag) in reply_paths.iter().zip(&media_tags) {
        let (status, headers, _) = get(&media, path, Some(tag)).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "{path}");
        assert_eq!(headers["cache-control"], "no-store");
        assert!(!headers.contains_key("etag"));
    }
    assert!(matches!(
        reader.get(&reply_asset).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        reader.get_thumbnail(&reply_asset).await,
        Err(StoreError::NotFound)
    ));
    for thumbnail in [false, true] {
        assert!(matches!(
            reader.get_post(&f.board, reply_tim, thumbnail).await,
            Err(StoreError::NotFound)
        ));
    }
    for (which, app) in [&web, &api].into_iter().enumerate() {
        for (index, path) in representations.iter().enumerate() {
            let (status, headers, bytes) = get(
                app,
                path,
                Some(&tags[which * representations.len() + index]),
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{path}");
            assert_ne!(headers["etag"], tags[which * representations.len() + index]);
            let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let rendered = value.to_string();
            assert!(!rendered.contains("ownedreplysecretneedle"));
            if index < 2 {
                let comments: String = value["posts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|p| p["com"].as_str())
                    .collect();
                assert!(
                    comments.contains(&format!("<span class=\"deadlink\">&gt;&gt;{reply}</span>")),
                    "{path}: {comments}"
                );
                if index == 0 {
                    assert_eq!(value["posts"][0]["replies"], 1);
                    assert_eq!(value["posts"][0]["images"], 1);
                }
            }
            assert_eq!(
                get(app, path, Some(headers["etag"].to_str().unwrap()))
                    .await
                    .0,
                StatusCode::NOT_MODIFIED
            );
        }
    }
    for path in [
        format!("/{}/", f.board),
        format!("/{}/catalog", f.board),
        format!("/{}/index.rss", f.board),
        format!("/search/api?q=ownedreplysecretneedle&b={}", f.board),
    ] {
        let (status, _, body) = get(&web, &path, None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            !String::from_utf8_lossy(&body).contains("ownedreplysecretneedle"),
            "{path}"
        );
    }
    for path in [
        format!("/{}/post/{reply}", f.board),
        format!("/_watch/{}/post/{reply}", f.board),
    ] {
        assert_eq!(get(&web, &path, None).await.0, StatusCode::NOT_FOUND);
    }
    assert!(matches!(
        f.post(
            op,
            "Consumed receipt reuse before cleanup",
            Some(&reply_upload)
        )
        .await,
        Err(StoreError::Conflict(_))
    ));
    {
        let guard = store.try_lock().unwrap();
        assert!(f.queue.retire_output(&reply_asset).await.unwrap());
        guard.remove(reply_asset.parse().unwrap()).unwrap();
        assert!(f.queue.forget_output(&reply_asset).await.unwrap());
    }
    assert!(matches!(
        f.post(
            op,
            "Consumed receipt reuse after cleanup",
            Some(&reply_upload)
        )
        .await,
        Err(StoreError::Conflict(_))
    ));
    let retained: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM content.post_media WHERE post_id=$1 AND job_id=$2 AND asset_id=$3)")
        .bind(reply).bind(&reply_upload.upload.id).bind(&reply_asset).fetch_one(&f.owner).await.unwrap();
    assert!(
        retained,
        "Cleanup must keep the consumed attachment tombstone"
    );
    for path in reply_paths.iter().chain(&replay_paths) {
        assert_eq!(get(&media, path, None).await.0, StatusCode::NOT_FOUND);
    }
    for path in op_paths.iter().chain(&live_paths).chain(&archive_paths) {
        assert_eq!(get(&media, path, None).await.0, StatusCode::OK);
    }

    // File-only deletion preserves author/text and the surviving thread; a
    // retained archive continues to expose its intended content and media.
    delete(&web, &f.board, live, true).await;
    let preserved: (bool, String, bool) = sqlx::query_as("SELECT content_erased,comment,EXISTS(SELECT 1 FROM post_secrets.deletion d WHERE d.post_id=p.id) FROM content.posts p WHERE id=$1")
        .bind(live).fetch_one(&f.owner).await.unwrap();
    assert_eq!(
        preserved,
        (
            false,
            format!("Surviving same-thread quote >>{reply}"),
            true
        )
    );
    for path in &live_paths {
        assert_eq!(get(&media, path, None).await.0, StatusCode::NOT_FOUND);
    }
    sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 day' WHERE id=$1")
        .bind(cross).execute(&f.owner).await.unwrap();
    let archive = get(&web, &format!("/{}/thread/{cross}.json", f.board), None).await;
    assert_eq!(archive.0, StatusCode::OK);
    assert!(String::from_utf8_lossy(&archive.2).contains("Surviving cross-thread quote"));
    for path in &archive_paths {
        assert_eq!(get(&media, path, None).await.0, StatusCode::OK);
    }
    delete(&web, &f.board, op, false).await;
    for path in &op_paths {
        assert_eq!(get(&media, path, None).await.0, StatusCode::NOT_FOUND);
    }
    assert!(root.join(format!("{op_asset}.png")).is_file());
    for app in [&web, &api] {
        assert_eq!(
            get(app, &format!("/{}/thread/{op}.json", f.board), None)
                .await
                .0,
            StatusCode::NOT_FOUND
        );
    }
    let descendants: i64 = sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE thread_id=$1 AND (NOT content_erased OR comment<>'' OR name<>'' OR subject<>'')")
        .bind(op).fetch_one(&f.owner).await.unwrap();
    assert_eq!(descendants, 0);
    for path in [
        format!("/{}/", f.board),
        format!("/{}/catalog", f.board),
        format!("/{}/index.rss", f.board),
        format!("/search/api?q=ownedopsecretneedle&b={}", f.board),
    ] {
        let (status, _, body) = get(&web, &path, None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(
            !String::from_utf8_lossy(&body).contains("ownedopsecretneedle"),
            "{path}"
        );
    }
    for path in &archive_paths {
        assert_eq!(get(&media, path, None).await.0, StatusCode::OK);
    }
    let (status, headers, bytes) = get(
        &web,
        &format!("/{}/thread/{cross}.json", f.board),
        Some(archive.1["etag"].to_str().unwrap()),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_ne!(headers["etag"], archive.1["etag"]);
    let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(
        value["posts"][0]["com"]
            .as_str()
            .unwrap()
            .contains(&format!("<span class=\"deadlink\">&gt;&gt;{op}</span>"))
    );
    for path in [
        format!("/{}/1.json", f.board),
        format!("/{}/catalog.json", f.board),
    ] {
        for app in [&web, &api] {
            let (status, _, bytes) = get(app, &path, None).await;
            assert_eq!(status, StatusCode::OK);
            assert!(!String::from_utf8_lossy(&bytes).contains("ownedopsecretneedle"));
        }
    }
    reader.ready().await.unwrap();
    reader.close().await;
}

#[tokio::test]
async fn fresh_whole_deletion_revokes_public_payload_media_and_consumed_receipt_reuse() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let seed: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&owner)
        .await
        .unwrap();
    let board = format!("fe{seed:x}");
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,deletion_known_min_seconds,deletion_unknown_min_seconds,archive_retention_seconds) VALUES($1,'Fresh erasure','Owned synthetic fixture',2000,100,100,100,10,100,0,0,0,0,0,86400)")
        .bind(&board).execute(&owner).await.unwrap();
    let jobs = Arc::new(Mutex::new(Vec::new()));
    let fixture = Fixture {
        owner: owner.clone(),
        public: public.clone(),
        board: board.clone(),
        jobs: jobs.clone(),
        intake: IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
            .await
            .unwrap(),
        queue: MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
            .await
            .unwrap(),
        hash: Argon2::default()
            .hash_password(PASSWORD.as_bytes(), &SaltString::generate(&mut OsRng))
            .unwrap()
            .to_string(),
    };
    let result = tokio::spawn(exercise(fixture)).await;
    posting::cleanup_posting(&owner, &board).await;
    // Only dispose of this test's owned fixture rows, after all tombstone checks.
    for statement in [
        "DELETE FROM content.post_media WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(statement)
            .bind(&board)
            .execute(&owner)
            .await
            .unwrap();
    }
    let jobs = jobs.lock().unwrap().clone();
    for statement in [
        "DELETE FROM media.assets WHERE job_id=ANY($1)",
        "DELETE FROM media.jobs WHERE id=ANY($1)",
    ] {
        sqlx::query(statement)
            .bind(&jobs)
            .execute(&owner)
            .await
            .unwrap();
    }
    public.close().await;
    owner.close().await;
    result.unwrap();
}
