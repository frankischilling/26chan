#![cfg(feature = "database-tests")]
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use board_media::{
    ApprovedFiles, PublicationStore, Quarantine,
    swf::{OUTPUT_DISK_BYTES, ValidatedSwf},
};
use board_store::{
    media::MediaQueue,
    media_assets::{InputComponent, InputReceipt, MediaReader, OutputMetadata},
    media_intake::IntakeStore,
};
use tower::ServiceExt;

fn request(path: &str) -> axum::http::request::Builder {
    Request::builder()
        .uri(path)
        .header("host", "127.0.0.1:3002")
}

#[tokio::test]
async fn swf_bytes_are_download_only_and_approval_precedes_conditional_reads() {
    let admin = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let intake = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let reader = MediaReader::connect(&std::env::var("MEDIA_READ_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(temp.path().join("private")).unwrap();
    let objects = temp.path().join("objects");
    let store = PublicationStore::new(&objects, &quarantine).unwrap();
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../fixtures/swf-header-reference.json")).unwrap();
    let hex = fixture["cases"][0]["bytes_hex"].as_str().unwrap();
    let bytes: Vec<u8> = hex
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect();
    // Host framing qualification; guest structural checks have separate tests.
    let mut disk = vec![0; OUTPUT_DISK_BYTES as usize];
    disk[..8].copy_from_slice(b"IBSWF008");
    disk[8..12].copy_from_slice(&640u32.to_be_bytes());
    disk[12..16].copy_from_slice(&480u32.to_be_bytes());
    disk[16..24].copy_from_slice(&(bytes.len() as u64).to_be_bytes());
    disk[24] = 10;
    disk[32..32 + bytes.len()].copy_from_slice(&bytes);
    let output = ValidatedSwf::read_disk(disk.as_slice()).await.unwrap();
    let upload = intake
        .reserve_image("owned.swf", "f", 0, false)
        .await
        .unwrap();
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
    guard
        .install_swf(asset.id.parse().unwrap(), &output)
        .unwrap();
    let app = board_media_http::router(board_media_http::AppState::new(
        reader.clone(),
        ApprovedFiles::open(&objects).unwrap(),
        &board_config::Origin::parse("http://127.0.0.1:3002").unwrap(),
        &board_config::Origin::parse("http://127.0.0.1:3000").unwrap(),
    ));
    let path = format!("/media/{}.swf", asset.id);
    assert_eq!(
        app.clone()
            .oneshot(request(&path).body(Body::empty()).unwrap())
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    queue
        .record_input_receipt(&job.id, token, &asset.id, &receipt)
        .await
        .unwrap();
    queue
        .approve_output_with_input(&job.id, token, &asset.id, &receipt)
        .await
        .unwrap();
    drop(guard);
    let response = app
        .clone()
        .oneshot(request(&path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "application/x-shockwave-flash"
    );
    assert!(
        response.headers()["content-disposition"]
            .to_str()
            .unwrap()
            .starts_with("attachment;")
    );
    assert_eq!(response.headers()["x-frame-options"], "DENY");
    assert!(
        response.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("default-src 'none'; sandbox;")
    );
    let etag = response.headers()["etag"].clone();
    assert_eq!(
        to_bytes(response.into_body(), 11 * 1024 * 1024)
            .await
            .unwrap(),
        bytes
    );
    let response = app
        .clone()
        .oneshot(request(&path).method("HEAD").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(to_bytes(response.into_body(), 1).await.unwrap().is_empty());
    assert_eq!(
        app.clone()
            .oneshot(
                request(&path)
                    .header("if-none-match", &etag)
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_MODIFIED
    );
    std::fs::write(objects.join(format!("{}.swf", asset.id)), b"changed bytes").unwrap();
    assert_eq!(
        app.clone()
            .oneshot(
                request(&path)
                    .header("if-none-match", &etag)
                    .body(Body::empty())
                    .unwrap()
            )
            .await
            .unwrap()
            .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    sqlx::query("UPDATE media.assets SET state='deleting',approved_at=NULL WHERE id=$1")
        .bind(&asset.id)
        .execute(&admin)
        .await
        .unwrap();
    assert_eq!(
        app.oneshot(
            request(&path)
                .header("if-none-match", etag)
                .body(Body::empty())
                .unwrap()
        )
        .await
        .unwrap()
        .status(),
        StatusCode::NOT_FOUND
    );
    sqlx::query("DELETE FROM media.assets WHERE id=$1")
        .bind(&asset.id)
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("DELETE FROM media.jobs WHERE id=$1")
        .bind(&job.id)
        .execute(&admin)
        .await
        .unwrap();
}
