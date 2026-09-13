#![cfg(feature = "browser-tests")]
//! Uses synthetic validated pixels, not a guest. Native CI runs the same browser
//! against the actual Firecracker pipeline in tests/media/public_upload_fixture.py.
use board_media::{ApprovedFiles, PublicationStore, Quarantine, ValidatedOutput};
use board_store::{media::MediaQueue, media_assets::MediaReader, media_intake::IntakeStore};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::Command,
};

fn environment(command: &mut Command) {
    command.env_clear();
    for key in [
        "PATH",
        "SystemRoot",
        "WINDIR",
        "TEMP",
        "TMP",
        "USERPROFILE",
        "HOME",
        "LOCALAPPDATA",
        "APPDATA",
        "PLAYWRIGHT_BROWSERS_PATH",
        "LANG",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
}

fn server(
    listener: tokio::net::TcpListener,
    app: axum::Router,
) -> (
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<()>,
) {
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    (stop, task)
}

#[tokio::test]
async fn no_javascript_browser_posts_and_deletes_an_approved_attachment() {
    let admin = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT substr(replace(gen_random_uuid()::text,'-',''),1,10)")
            .fetch_one(&admin)
            .await
            .unwrap();
    let filename = format!("public-upload-{board}.png");
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit) VALUES ($1,'Browser upload','Synthetic',2000,100,100,100,10,3)")
        .bind(&board).execute(&admin).await.unwrap();
    let directory = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(directory.path().join("quarantine")).unwrap();
    let objects = directory.path().join("objects");
    let store = PublicationStore::new(&objects, &quarantine).unwrap();
    let intake = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let reader = MediaReader::connect(&std::env::var("MEDIA_READ_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let intake_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let intake_address = intake_listener.local_addr().unwrap();
    let media_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let media_origin = format!("http://{}", media_listener.local_addr().unwrap());
    let (stop_intake, intake_task) = server(
        intake_listener,
        board_media_intake::router(
            board_media_intake::AppState::new(intake, quarantine, "a".repeat(64)).unwrap(),
        ),
    );
    let (stop_media, media_task) = server(
        media_listener,
        board_media_http::router(board_media_http::AppState::new(
            reader,
            ApprovedFiles::open(&objects).unwrap(),
            &board_config::Origin::parse(&media_origin).unwrap(),
        )),
    );
    let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let public_address = port.local_addr().unwrap();
    drop(port);
    let public_origin = format!("http://{public_address}");
    let mut command = Command::new(env!("CARGO_BIN_EXE_board-public"));
    environment(&mut command);
    let mut public = command
        .env("APP_ENV", "development")
        .env("MEDIA_ENABLED", "true")
        .env("PUBLIC_MEDIA_PROFILE", "isolated-development")
        .env(
            "DATABASE_URL",
            std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap(),
        )
        .env("BIND_ADDR", public_address.to_string())
        .env("PUBLIC_ORIGIN", &public_origin)
        .env("STAFF_ORIGIN", "http://localhost:3001")
        .env("MEDIA_ORIGIN", &media_origin)
        .env("PUBLIC_INTAKE_ADDR", intake_address.to_string())
        .env("PUBLIC_INTAKE_TOKEN", "a".repeat(64))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let test_board = board.clone();
    let test_root = directory.path().to_owned();
    let test_admin = admin.clone();
    let outcome = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if let Ok(mut socket) = tokio::net::TcpStream::connect(public_address).await {
                    socket.write_all(format!("GET /readyz HTTP/1.1\r\nHost: {public_address}\r\nConnection: close\r\n\r\n").as_bytes()).await.unwrap();
                    let mut response = Vec::new();
                    socket.take(4096).read_to_end(&mut response).await.unwrap();
                    if response.starts_with(b"HTTP/1.1 200") { break; }
                }
                tokio::time::sleep(Duration::from_millis(30)).await;
            }
        }).await.expect("public binary readiness deadline");
        // A second attachment leaves the first post's deletion tombstone on the
        // same board. Browser assertions must identify the post they changed.
        for _ in 0..2 {
            exercise(&test_admin, &test_root, &test_board, &public_origin, &store).await;
        }
    }).await;
    public.kill().await.unwrap();
    public.wait().await.unwrap();
    let _ = stop_intake.send(());
    let _ = stop_media.send(());
    intake_task.await.unwrap();
    media_task.await.unwrap();
    // The spawned assertion task unwinds before cleanup, including browser drop.
    sqlx::query("DELETE FROM content.post_media WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)").bind(&board).execute(&admin).await.unwrap();
    sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)").bind(&board).execute(&admin).await.unwrap();
    sqlx::query("DELETE FROM content.posts WHERE board=$1")
        .bind(&board)
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.threads WHERE board=$1")
        .bind(&board)
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.boards WHERE slug=$1")
        .bind(&board)
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query(
        "DELETE FROM media.assets WHERE job_id IN (SELECT id FROM media.jobs WHERE filename=$1)",
    )
    .bind(&filename)
    .execute(&admin)
    .await
    .unwrap();
    sqlx::query("DELETE FROM media.jobs WHERE filename=$1")
        .bind(&filename)
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    outcome.unwrap();
}

async fn exercise(
    admin: &sqlx::PgPool,
    root: &Path,
    board: &str,
    origin: &str,
    store: &PublicationStore,
) {
    // Public/intake treat the file as opaque. This separate trusted fixture
    // supplies bounded synthetic pixels to the normal publication code.
    let source = root.join(format!("public-upload-{board}.png"));
    std::fs::write(
        &source,
        b"synthetic opaque upload; no decoder runs in this test",
    )
    .unwrap();
    let mut node = Command::new("node");
    environment(&mut node);
    if let Some(path) = std::env::var_os("PUBLIC_UPLOAD_SCREENSHOTS") {
        node.env("PUBLIC_UPLOAD_SCREENSHOTS", path);
    }
    let mut browser = node
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/browser/public-upload.mjs"))
        .arg(origin)
        .arg(board)
        .arg(&source)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let filename = source.file_name().unwrap().to_str().unwrap();
    let job: String = tokio::time::timeout(Duration::from_secs(25), async {
        loop {
            if let Some(job) =
                sqlx::query_scalar("SELECT id FROM media.jobs WHERE filename=$1 AND state='queued'")
                    .bind(filename)
                    .fetch_optional(admin)
                    .await
                    .unwrap()
            {
                break job;
            }
            assert!(
                browser.try_wait().unwrap().is_none(),
                "browser exited before uploading"
            );
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("browser upload deadline");
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let claim = queue.claim().await.unwrap().unwrap();
    assert_eq!(claim.id, job, "Requires an idle disposable queue");
    let mut pixels = b"IBRGBA01\0\0\0\x01\0\0\0\x01".to_vec();
    pixels.extend_from_slice(&[255, 0, 0, 255]);
    let output = ValidatedOutput::read(pixels.as_slice()).await.unwrap();
    let asset = board_media_admin::publish(
        &queue,
        store,
        &job,
        claim.lease_token.as_ref().unwrap(),
        &output,
    )
    .await
    .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(45), browser.wait_with_output())
        .await
        .unwrap()
        .unwrap();
    assert!(
        result.status.success(),
        "browser workflow failed: {} {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let files = root.join("objects");
    assert!(files.join(format!("{}.png", asset.id)).is_file());
    assert!(files.join(format!("{}.thumb.png", asset.id)).is_file());
    assert_eq!(
        board_media_admin::reconcile(&queue, store).await.unwrap(),
        1
    );
    assert!(!files.join(format!("{}.png", asset.id)).exists());
    assert!(!files.join(format!("{}.thumb.png", asset.id)).exists());
    let tombstones: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM content.post_media WHERE asset_id=$1 AND file_deleted",
    )
    .bind(&asset.id)
    .fetch_one(admin)
    .await
    .unwrap();
    assert_eq!(
        tombstones, 1,
        "Physical cleanup preserves the one-use tombstone"
    );
}
