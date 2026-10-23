#![cfg(feature = "browser-tests")]
//! Real pinned Ruffle playback; fixture publication uses a controlled host envelope.
use axum::{Router, routing::get};
use board_media::{
    PublicationStore, Quarantine,
    swf::{OUTPUT_DISK_BYTES, ValidatedSwf},
};
use board_store::{
    media::MediaQueue,
    media_assets::{InputComponent, InputReceipt, OutputMetadata},
    media_intake::IntakeStore,
};
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

fn environment(command: &mut Command) {
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
struct Parent(tokio::task::JoinHandle<()>);
impl Drop for Parent {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[tokio::test]
async fn pinned_player_renders_original_fws_and_cws_inside_the_opaque_frame() {
    let admin = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let before:String=sqlx::query_scalar("SELECT to_jsonb(b)::text FROM content.boards b WHERE slug='f' AND captcha_enabled AND NOT captcha_twister").fetch_one(&admin).await.unwrap();
    assert_eq!(sqlx::query("UPDATE content.boards b SET captcha_enabled=false WHERE slug='f' AND to_jsonb(b)=$1::jsonb").bind(&before).execute(&admin).await.unwrap().rows_affected(),1);
    let jobs = Arc::new(Mutex::new(Vec::<String>::new()));
    let posts = Arc::new(Mutex::new(Vec::<i64>::new()));
    let child = Arc::new(Mutex::new(None::<Child>));
    let root = tempfile::tempdir().unwrap();
    let (work_jobs, work_posts, work_child) = (jobs.clone(), posts.clone(), child.clone());
    let directory = root.path().to_owned();
    let result =
        tokio::spawn(async move { exercise(&directory, work_jobs, work_posts, work_child).await })
            .await;
    let server = child.lock().unwrap().take();
    if let Some(mut server) = server {
        let _ = server.kill().await;
        let _ = server.wait().await;
    }
    let ids = posts.lock().unwrap().clone();
    for query in [
        "DELETE FROM content.post_media WHERE post_id=ANY($1)",
        "DELETE FROM post_secrets.deletion WHERE post_id=ANY($1)",
        "DELETE FROM content.posts WHERE id=ANY($1)",
        "DELETE FROM content.threads WHERE id=ANY($1)",
    ] {
        sqlx::query(query).bind(&ids).execute(&admin).await.unwrap();
    }
    let ids = jobs.lock().unwrap().clone();
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
    assert_eq!(sqlx::query("UPDATE content.boards b SET captcha_enabled=true WHERE slug='f' AND to_jsonb(b)=($1::jsonb || '{\"captcha_enabled\":false}'::jsonb)").bind(before).execute(&admin).await.unwrap().rows_affected(),1,"Do not overwrite unrelated policy changes");
    result.unwrap();
}

async fn exercise(
    root: &Path,
    jobs: Arc<Mutex<Vec<String>>>,
    posts: Arc<Mutex<Vec<i64>>>,
    child: Arc<Mutex<Option<Child>>>,
) {
    let assets = std::env::var_os("SWF_PLAYER_ASSETS")
        .expect("Prepare the pinned SWF_PLAYER_ASSETS directory before browser qualification");
    let assets = std::fs::canonicalize(assets).unwrap();
    board_media_http::swf_player::PlayerAssets::open(&assets).unwrap();
    let admin = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = sqlx::PgPool::connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let intake = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let quarantine = Quarantine::new(root.join("private")).unwrap();
    let objects = root.join("objects");
    let store = PublicationStore::new(&objects, &quarantine).unwrap();
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../fixtures/swf-header-reference.json")).unwrap();
    let identity = board_domain::poster_id::PosterIdKey::parse(&"7".repeat(64)).unwrap();
    let mut movies = Vec::new();
    for number in 0..2 {
        let hex = fixture["cases"][number]["bytes_hex"].as_str().unwrap();
        let bytes: Vec<u8> = hex
            .as_bytes()
            .chunks_exact(2)
            .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
            .collect();
        let filename = format!("owned player {number} &猫.swf");
        let upload = intake
            .reserve_image(&filename, "f", 0, false)
            .await
            .unwrap();
        jobs.lock().unwrap().push(upload.id.clone());
        intake
            .begin_upload(&upload.id, &upload.capability)
            .await
            .unwrap();
        intake
            .finish_upload(&upload.id, &upload.capability, bytes.len() as u64)
            .await
            .unwrap();
        let job = queue.claim().await.unwrap().unwrap();
        assert_eq!(job.id, upload.id, "Use an idle disposable queue");
        let token = job.lease_token.as_deref().unwrap();
        let mut disk = vec![0; OUTPUT_DISK_BYTES as usize];
        disk[..8].copy_from_slice(b"IBSWF008");
        disk[8..12].copy_from_slice(&640u32.to_be_bytes());
        disk[12..16].copy_from_slice(&480u32.to_be_bytes());
        disk[16..24].copy_from_slice(&(bytes.len() as u64).to_be_bytes());
        disk[24] = 10;
        disk[25] = number as u8;
        disk[32..32 + bytes.len()].copy_from_slice(&bytes);
        let output = ValidatedSwf::read_disk(disk.as_slice()).await.unwrap();
        let mut md5 = [0; 16];
        for (index, byte) in md5.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&output.md5()[index * 2..index * 2 + 2], 16).unwrap();
        }
        let receipt = InputReceipt {
            component: InputComponent::Upload,
            sha256: output.sha256().into(),
            bytes: bytes.len() as i64,
            md5,
        };
        let metadata = OutputMetadata {
            sha256: output.sha256().into(),
            bytes: bytes.len() as i64,
            width: 640,
            height: 480,
        };
        let guard = store.try_lock().unwrap();
        let asset = queue
            .prepare_swf_output(&job.id, token, &metadata, &receipt)
            .await
            .unwrap();
        queue
            .record_input_receipt(&job.id, token, &asset.id, &receipt)
            .await
            .unwrap();
        guard
            .install_swf(asset.id.parse().unwrap(), &output)
            .unwrap();
        queue
            .approve_output_with_input(&job.id, token, &asset.id, &receipt)
            .await
            .unwrap();
        drop(guard);
        let post = board_store::create_post_with_metadata(
            &public,
            "f",
            0,
            &board_store::NewPost {
                name: String::new(),
                subject: format!("4|Owned browser movie {number}"),
                comment: format!("Owned browser playback {number}"),
                deletion_hash: "owned-hash".into(),
                sage: false,
            },
            Some(&board_store::post_media::NewAttachment {
                upload,
                spoiler: false,
            }),
            board_store::PostingContext {
                request_start: sqlx::query_scalar("SELECT clock_timestamp()")
                    .fetch_one(&admin)
                    .await
                    .unwrap(),
                peer: Some(format!("192.0.2.{}", 230 + number).parse().unwrap()),
                op_password_proof: None,
            },
            board_store::PostMetadata {
                keys: board_store::PostIdentityKeys {
                    tripcode: None,
                    poster_id: Some(&identity),
                },
                spoiler: false,
                country_database: None,
                flag: "",
                options: "",
                drawing: None,
            },
        )
        .await
        .unwrap();
        posts.lock().unwrap().push(post);
        let file = board_store::post_media::attachment(&public, post)
            .await
            .unwrap()
            .unwrap();
        movies.push((file.tim, filename));
    }
    let parent_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let parent_origin = format!("http://{}", parent_listener.local_addr().unwrap());
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    drop(listener);
    let media_origin = format!("http://{address}");
    let mut router = Router::new();
    for (number, (tim, _)) in movies.iter().enumerate() {
        let html = format!(
            "<!doctype html><title>Owned SWF parent</title><iframe width=\"640\" height=\"480\" sandbox=\"allow-scripts\" allow=\"autoplay; fullscreen\" src=\"{media_origin}/player/f/{tim}\"></iframe>"
        );
        let policy = format!(
            "default-src 'none'; frame-src {media_origin}/player/; base-uri 'none'; form-action 'none'"
        );
        router = router.route(
            &format!("/{number}"),
            get(move || {
                let (html, policy) = (html.clone(), policy.clone());
                async move {
                    (
                        [
                            ("content-type", "text/html; charset=utf-8".to_owned()),
                            ("content-security-policy", policy),
                        ],
                        html,
                    )
                }
            }),
        );
    }
    let _parent = Parent(tokio::spawn(async move {
        axum::serve(parent_listener, router).await.unwrap();
    }));
    let mut command = Command::new(env!("CARGO_BIN_EXE_board-media-http"));
    environment(&mut command);
    command
        .env("APP_ENV", "development")
        .env(
            "MEDIA_READ_DATABASE_URL",
            std::env::var("MEDIA_READ_DATABASE_URL").unwrap(),
        )
        .env("MEDIA_APPROVED_DIR", &objects)
        .env("MEDIA_PLAYER_DIR", assets)
        .env("PUBLIC_ORIGIN", &parent_origin)
        .env("MEDIA_ORIGIN", &media_origin)
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
    .expect("actual SWF reader readiness deadline");
    for (number, (tim, filename)) in movies.iter().enumerate() {
        let mut node = Command::new("node");
        environment(&mut node);
        node.arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/browser/media-swf.mjs"))
            .arg(format!("{parent_origin}/{number}"))
            .arg(format!("{media_origin}/player/f/{tim}"))
            .arg(format!(
                "{media_origin}/f/{}",
                board_domain::file_name::url_component(filename)
            ))
            .stdin(Stdio::null())
            .kill_on_drop(true);
        let result = tokio::time::timeout(Duration::from_secs(90), node.output())
            .await
            .unwrap()
            .unwrap();
        assert!(
            result.status.success(),
            "SWF browser failed: {} {}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
    }
}
