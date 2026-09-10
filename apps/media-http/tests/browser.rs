#![cfg(feature = "browser-tests")]

use board_media::{PublicationStore, Quarantine, ValidatedOutput};
use board_store::media::MediaQueue;
use std::{
    path::Path,
    process::Stdio,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, Command},
};

fn base_environment(command: &mut Command) {
    command.env_clear();
    for name in [
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
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
}

#[tokio::test]
async fn browser_displays_approved_png_from_the_actual_reader_binary() {
    let admin = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let ids = Arc::new(Mutex::new(Vec::<String>::new()));
    let child = Arc::new(Mutex::new(None::<Child>));
    let test_ids = ids.clone();
    let test_child = child.clone();
    // Keep the temporary store alive until the reader is explicitly stopped.
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().to_owned();
    let result = tokio::spawn(async move { exercise(&root, test_ids, test_child).await }).await;
    let server = child.lock().unwrap().take();
    if let Some(mut server) = server {
        server.kill().await.unwrap();
        server.wait().await.unwrap();
    }
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

async fn exercise(root: &Path, ids: Arc<Mutex<Vec<String>>>, child: Arc<Mutex<Option<Child>>>) {
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let quarantine = Quarantine::new(root.join("quarantine")).unwrap();
    let objects = root.join("objects");
    let store = PublicationStore::new(&objects, &quarantine).unwrap();
    let job = queue
        .reserve("media browser synthetic fixture")
        .await
        .unwrap();
    ids.lock().unwrap().push(job.id.clone());
    queue.queue(&job.id, 4).await.unwrap();
    let lease = queue.claim().await.unwrap().unwrap();
    assert_eq!(lease.id, job.id, "Use an idle disposable queue");
    let mut pixels = b"IBRGBA01\0\0\0\x01\0\0\0\x01".to_vec();
    pixels.extend_from_slice(&[9, 80, 30, 255]);
    let output = ValidatedOutput::read(pixels.as_slice()).await.unwrap();
    let asset = board_media_admin::publish(
        &queue,
        &store,
        &job.id,
        lease.lease_token.as_ref().unwrap(),
        &output,
    )
    .await
    .unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let mut command = Command::new(env!("CARGO_BIN_EXE_board-media-http"));
    base_environment(&mut command);
    command
        .env("APP_ENV", "development")
        .env(
            "MEDIA_READ_DATABASE_URL",
            std::env::var("MEDIA_READ_DATABASE_URL").unwrap(),
        )
        .env("MEDIA_APPROVED_DIR", &objects)
        .env("MEDIA_ORIGIN", format!("http://{address}"))
        .env("MEDIA_BIND_ADDR", address.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    *child.lock().unwrap() = Some(command.spawn().unwrap());
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(mut stream) = tokio::net::TcpStream::connect(address).await {
                stream
                    .write_all(
                        format!(
                            "GET /readyz HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n"
                        )
                        .as_bytes(),
                    )
                    .await
                    .unwrap();
                let mut response = Vec::new();
                stream.take(2048).read_to_end(&mut response).await.unwrap();
                if response.starts_with(b"HTTP/1.1 200") {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await
    .expect("actual media binary readiness deadline");
    let mut node = Command::new("node");
    base_environment(&mut node);
    node.arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/browser/media-image.mjs"))
        .arg(format!("http://{address}/media/{}.png", asset.id))
        .stdin(Stdio::null())
        .kill_on_drop(true);
    let result = tokio::time::timeout(Duration::from_secs(45), node.output())
        .await
        .unwrap()
        .unwrap();
    assert!(
        result.status.success(),
        "media browser failed: {} {}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
}
