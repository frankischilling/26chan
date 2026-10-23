#![cfg(feature = "database-tests")]
//! Actual multipart HTTP -> intake -> capability-scoped publication -> post.
//! The supervisor supplies an owned guest envelope, not a decoder or VM claim.
#[path = "support/posting.rs"]
mod posting;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use board_media::{
    InputSnapshot, PublicationStore, Quarantine,
    swf::{OUTPUT_DISK_BYTES, ValidatedSwf},
};
use board_store::{
    media::MediaQueue,
    media_assets::{InputComponent, InputReceipt, MediaReader, OutputMetadata},
    media_intake::IntakeStore,
};
use std::{path::Path, time::Duration};
use tower::ServiceExt;

fn multipart(filename: &str, bytes: &[u8], duplicate: bool) -> Vec<u8> {
    let mut body = Vec::new();
    for (name, value) in [
        ("resto", "0"),
        ("mode", "regist"),
        ("name", "Owned"),
        ("sub", "One submit"),
        ("pwd", "owned-one-submit-password"),
        ("com", "Owned one-submit SWF"),
    ] {
        body.extend_from_slice(format!("--owned-boundary\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n").as_bytes());
    }
    body.extend_from_slice(format!("--owned-boundary\r\nContent-Disposition: form-data; name=\"upfile\"; filename=\"{filename}\"\r\nContent-Type: application/x-shockwave-flash\r\n\r\n").as_bytes());
    body.extend_from_slice(bytes);
    body.extend_from_slice(b"\r\n");
    body.extend_from_slice(
        b"--owned-boundary\r\nContent-Disposition: form-data; name=\"filetag\"\r\n\r\n4\r\n",
    );
    if duplicate {
        body.extend_from_slice(
            b"--owned-boundary\r\nContent-Disposition: form-data; name=\"mode\"\r\n\r\nregist\r\n",
        );
    }
    body.extend_from_slice(b"--owned-boundary--\r\n");
    body
}
fn request(body: Vec<u8>) -> Request<Body> {
    Request::post("/f/imgboard.php")
        .header("origin", "http://127.0.0.1:3000")
        .header(
            "content-type",
            "multipart/form-data; boundary=owned-boundary",
        )
        .body(Body::from(body))
        .unwrap()
}
async fn publish_one(root: &Path, expected: &str, wait_seconds: u64) -> String {
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let job = tokio::time::timeout(Duration::from_secs(wait_seconds), async {
        loop {
            if let Some(job) = queue.claim().await.unwrap() {
                break job;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(job.filename, expected, "Use an idle disposable queue");
    let quarantine = Quarantine::new(root.join("private")).unwrap();
    let snapshot = InputSnapshot::read_still(
        quarantine
            .open_still_input(job.id.parse().unwrap(), job.input_bytes.unwrap() as u64)
            .unwrap(),
        job.input_bytes.unwrap() as u64,
    )
    .await
    .unwrap();
    let bytes = snapshot.bytes();
    let mut disk = vec![0; OUTPUT_DISK_BYTES as usize];
    disk[..8].copy_from_slice(b"IBSWF008");
    disk[8..12].copy_from_slice(&640u32.to_be_bytes());
    disk[12..16].copy_from_slice(&480u32.to_be_bytes());
    disk[16..24].copy_from_slice(&(bytes.len() as u64).to_be_bytes());
    disk[24] = bytes[3];
    disk[32..32 + bytes.len()].copy_from_slice(bytes);
    let output = ValidatedSwf::read_disk(disk.as_slice()).await.unwrap();
    let receipt = InputReceipt {
        component: InputComponent::Upload,
        sha256: output.sha256().into(),
        bytes: bytes.len() as i64,
        md5: *snapshot.raw_md5(),
    };
    let metadata = OutputMetadata {
        sha256: output.sha256().into(),
        bytes: bytes.len() as i64,
        width: 640,
        height: 480,
    };
    let token = job.lease_token.as_deref().unwrap();
    let store = PublicationStore::new(root.join("objects"), &quarantine).unwrap();
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
    job.id
}
#[tokio::test]
async fn a_source_form_posts_without_javascript_and_rejects_late_duplicate_fields() {
    let owner = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = sqlx::PgPool::connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let before:String=sqlx::query_scalar("SELECT to_jsonb(b)::text FROM content.boards b WHERE slug='f' AND captcha_enabled AND NOT captcha_twister").fetch_one(&owner).await.unwrap();
    assert_eq!(sqlx::query("UPDATE content.boards b SET captcha_enabled=false,posting_thread_seconds=0,posting_image_seconds=0 WHERE slug='f' AND to_jsonb(b)=$1::jsonb").bind(&before).execute(&owner).await.unwrap().rows_affected(),1);
    let nonce: String = sqlx::query_scalar("SELECT replace(gen_random_uuid()::text,'-','')")
        .fetch_one(&owner)
        .await
        .unwrap();
    let filename = format!("owned-one-submit-{nonce}.swf");
    let rejected = format!("owned-rejected-{nonce}.swf");
    let browser_name = format!("owned-browser-{nonce}.swf");
    let reordered_name = format!("owned-reordered-{nonce}.swf");
    let root = tempfile::tempdir().unwrap();
    let path = root.path().to_owned();
    let intake = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = board_media_intake::router(
        board_media_intake::AppState::new(
            intake,
            Quarantine::new(path.join("private")).unwrap(),
            "a".repeat(64),
        )
        .unwrap(),
    );
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let settings = board_config::PublicMediaSettings::development(
        &address.to_string(),
        &"a".repeat(64),
        "http://localhost:3002",
    )
    .unwrap();
    let (web, _) = posting::routers_with_media(
        public.clone(),
        "f",
        "http://127.0.0.1:3000".into(),
        false,
        Some(settings),
    );
    let test_filename = filename.clone();
    let test_rejected = rejected.clone();
    let test_reordered = reordered_name.clone();
    #[cfg(feature = "browser-tests")]
    let test_browser_name = browser_name.clone();
    let admin = owner.clone();
    let result=tokio::spawn(async move {
        let source:serde_json::Value=serde_json::from_str(include_str!("../../../fixtures/swf-header-reference.json")).unwrap();
        let raw=source["cases"][0]["bytes_hex"].as_str().unwrap();
        let mut bytes:Vec<u8>=raw.as_bytes().chunks_exact(2).map(|pair|u8::from_str_radix(std::str::from_utf8(pair).unwrap(),16).unwrap()).collect();
        assert_eq!(&bytes[bytes.len()-2..],&[0,0]);bytes.truncate(bytes.len()-2);
        let metadata=format!("<owned nonce=\"{nonce}\"/>\0");assert!(metadata.len()<63);
        bytes.extend_from_slice(&((77u16<<6)|metadata.len() as u16).to_le_bytes());bytes.extend_from_slice(metadata.as_bytes());bytes.extend_from_slice(&[0,0]);let length=bytes.len() as u32;bytes[4..8].copy_from_slice(&length.to_le_bytes());
        let response=web.clone().oneshot(Request::get("/f/").body(Body::empty()).unwrap()).await.unwrap();
        let html=String::from_utf8(to_bytes(response.into_body(),2*1024*1024).await.unwrap().to_vec()).unwrap();
        assert!(html.contains("data-upload-file-row>"));assert!(!html.contains("form.postForm[action"));assert!(!html.contains("action=\"/f/upload\""));
        let (response,job)=tokio::join!(web.clone().oneshot(request(multipart(&test_filename,&bytes,false))),publish_one(&path,&test_filename,8));
        let response=response.unwrap();let status=response.status();let location=response.headers().get("location").and_then(|v|v.to_str().ok()).map(str::to_owned);let message=String::from_utf8(to_bytes(response.into_body(),1024*1024).await.unwrap().to_vec()).unwrap();
        assert_eq!(status,StatusCode::SEE_OTHER,"{message}");let id:i64=location.unwrap().split("#p").nth(1).unwrap().parse().unwrap();
        let post=board_store::find_post(&public,"f",id).await.unwrap();assert_eq!(post.subject,"4|One submit");assert_eq!(board_store::post_media::attachment(&public,id).await.unwrap().unwrap().output_format,board_store::media_assets::MediaFormat::Swf);
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM content.post_media WHERE job_id=$1").bind(job).fetch_one(&admin).await.unwrap(),1);
        let reader=MediaReader::connect(&std::env::var("MEDIA_READ_DATABASE_URL").unwrap()).await.unwrap();
        let media=board_media_http::router(board_media_http::AppState::new(reader,board_media::ApprovedFiles::open(path.join("objects")).unwrap(),&board_config::Origin::parse("http://localhost:3002").unwrap(),&board_config::Origin::parse("http://127.0.0.1:3000").unwrap()));
        let download=media.oneshot(Request::get(format!("/f/{test_filename}")).header("host","localhost:3002").body(Body::empty()).unwrap()).await.unwrap();assert_eq!(download.status(),StatusCode::OK);assert_eq!(to_bytes(download.into_body(),10_485_760).await.unwrap().as_ref(),bytes.as_slice());
        let mut reordered_bytes=bytes[..bytes.len()-2].to_vec();let marker=b"<owned-reordered/>\0";
        reordered_bytes.extend_from_slice(&((77u16<<6)|marker.len() as u16).to_le_bytes());reordered_bytes.extend_from_slice(marker);reordered_bytes.extend_from_slice(&[0,0]);let length=reordered_bytes.len() as u32;reordered_bytes[4..8].copy_from_slice(&length.to_le_bytes());
        let mut encoded=multipart(&test_reordered,&reordered_bytes,false);
        let old=b"Owned one-submit SWF";let at=encoded.windows(old.len()).position(|part|part==old).unwrap();encoded.splice(at..at+old.len(),b"Owned reordered SWF".iter().copied());
        let start=b"--owned-boundary\r\nContent-Disposition: form-data; name=\"upfile\"";let at=encoded.windows(start.len()).position(|part|part==start).unwrap();
        let boundary=b"--owned-boundary";let end=at+start.len()+encoded[at+start.len()..].windows(boundary.len()).position(|part|part==boundary).unwrap();
        let mut moved=encoded[at..end].to_vec();moved.extend_from_slice(&encoded[..at]);moved.extend_from_slice(&encoded[end..]);
        let (response,_)=tokio::join!(web.clone().oneshot(request(moved)),publish_one(&path,&test_reordered,8));
        let response=response.unwrap();let status=response.status();let message=String::from_utf8(to_bytes(response.into_body(),1024*1024).await.unwrap().to_vec()).unwrap();assert_eq!(status,StatusCode::SEE_OTHER,"File-first source multipart: {message}");
        #[cfg(feature = "browser-tests")]
        browser_form(&path,&public,address,&test_browser_name,&bytes).await;
        let refused=web.clone().oneshot(request(multipart(&test_rejected,&bytes,true))).await.unwrap();assert_eq!(refused.status(),StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM media_intake.handles h JOIN media.jobs j ON j.id=h.job_id WHERE j.filename=$1").bind(&test_rejected).fetch_one(&admin).await.unwrap(),0,"Late form errors revoke the issued capability");
        let too_large=web.oneshot(Request::post("/test/imgboard.php").header("origin","http://127.0.0.1:3000").header("content-type","application/x-www-form-urlencoded").body(Body::from("x".repeat(262145))).unwrap()).await.unwrap();assert_eq!(too_large.status(),StatusCode::PAYLOAD_TOO_LARGE,"Other legacy forms keep their 256 KiB body limit");
    }).await;
    server.abort();
    let _ = server.await;
    let names = vec![filename, rejected, browser_name, reordered_name];
    let ids:Vec<i64>=sqlx::query_scalar("SELECT m.post_id FROM content.post_media m JOIN media.jobs j ON j.id=m.job_id WHERE j.filename=ANY($1)").bind(&names).fetch_all(&owner).await.unwrap();
    for query in [
        "DELETE FROM content.post_media WHERE post_id=ANY($1)",
        "DELETE FROM post_secrets.deletion WHERE post_id=ANY($1)",
        "DELETE FROM content.posts WHERE id=ANY($1)",
        "DELETE FROM content.threads WHERE id=ANY($1)",
    ] {
        sqlx::query(query).bind(&ids).execute(&owner).await.unwrap();
    }
    for query in [
        "DELETE FROM media.assets WHERE job_id IN(SELECT id FROM media.jobs WHERE filename=ANY($1))",
        "DELETE FROM media.jobs WHERE filename=ANY($1)",
    ] {
        sqlx::query(query)
            .bind(&names)
            .execute(&owner)
            .await
            .unwrap();
    }
    assert_eq!(sqlx::query("UPDATE content.boards b SET captcha_enabled=($1::jsonb->>'captcha_enabled')::boolean,posting_thread_seconds=($1::jsonb->>'posting_thread_seconds')::integer,posting_image_seconds=($1::jsonb->>'posting_image_seconds')::integer WHERE slug='f' AND to_jsonb(b)=($1::jsonb || '{\"captcha_enabled\":false,\"posting_thread_seconds\":0,\"posting_image_seconds\":0}'::jsonb)").bind(&before).execute(&owner).await.unwrap().rows_affected(),1);
    result.unwrap();
}

#[cfg(feature = "browser-tests")]
async fn browser_form(
    root: &Path,
    public: &sqlx::PgPool,
    intake: std::net::SocketAddr,
    filename: &str,
    original: &[u8],
) {
    let mut bytes = original[..original.len() - 2].to_vec();
    let payload = b"<owned-browser/>\0";
    bytes.extend_from_slice(&((77u16 << 6) | payload.len() as u16).to_le_bytes());
    bytes.extend_from_slice(payload);
    bytes.extend_from_slice(&[0, 0]);
    let length = bytes.len() as u32;
    bytes[4..8].copy_from_slice(&length.to_le_bytes());
    let source = root.join(filename);
    std::fs::write(&source, &bytes).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let settings = board_config::PublicMediaSettings::development(
        &intake.to_string(),
        &"a".repeat(64),
        "http://localhost:3002",
    )
    .unwrap();
    let (web, _) =
        posting::routers_with_media(public.clone(), "f", origin.clone(), false, Some(settings));
    let server = tokio::spawn(async move {
        axum::serve(listener, web).await.unwrap();
    });
    let mut node = tokio::process::Command::new("node");
    node.env_clear().kill_on_drop(true);
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
            node.env(key, value);
        }
    }
    node.arg(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/browser/swf-one-submit.mjs"),
    )
    .arg(origin)
    .arg(source);
    let (result, _) = tokio::join!(
        tokio::time::timeout(Duration::from_secs(60), node.output()),
        publish_one(root, filename, 30)
    );
    server.abort();
    let _ = server.await;
    let output = result.expect("Source form browser deadline").unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
