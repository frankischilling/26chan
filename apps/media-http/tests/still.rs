#![cfg(feature = "database-tests")]
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use board_media::{
    ApprovedFiles, PublicationStore, Quarantine,
    still::{OUTPUT_DISK_BYTES, StillFormat, ValidatedStill, source_policy},
};
use board_store::{
    media::MediaQueue,
    media_assets::{
        MediaFormat, MediaReader, OutputMetadata, OutputVariants, SourceProfile, SourceProvenance,
        StillOutput,
    },
    media_intake::IntakeStore,
};
use tower::ServiceExt;

// Serving receipts only: decoder admission is tested in the isolated guest suite.
async fn candidate() -> ValidatedStill {
    let policy = source_policy("aco", StillFormat::Jpeg, true, true).unwrap();
    let mut disk = vec![0; OUTPUT_DISK_BYTES as usize];
    disk[..8].copy_from_slice(b"IBIMG006");
    disk[8..24].copy_from_slice(&policy.header().unwrap());
    for (offset, value) in [
        (24, 2000u32),
        (28, 1000),
        (32, 250),
        (36, 125),
        (40, 1024),
        (44, 512),
        (48, 32),
        (52, 5),
        (56, 5),
    ] {
        disk[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
    }
    disk[64..67].copy_from_slice(b"\xff\xd8\xff");
    disk[94..96].copy_from_slice(b"\xff\xd9");
    disk[96..101].copy_from_slice(b"\xff\xd8\xff\xff\xd9");
    disk[101..106].copy_from_slice(b"\xff\xd8\xff\xff\xd9");
    ValidatedStill::read_disk(std::io::Cursor::new(disk), policy)
        .await
        .unwrap()
}
fn metadata(file: &board_media::still::EncodedStillFile) -> OutputMetadata {
    let (width, height) = file.dimensions();
    OutputMetadata {
        sha256: file.sha256().into(),
        bytes: file.bytes().len() as i64,
        width: width as i32,
        height: height as i32,
    }
}
fn request(path: &str) -> axum::http::request::Builder {
    Request::builder()
        .uri(path)
        .header("host", "127.0.0.1:3002")
}
#[tokio::test]
async fn jpeg_and_companions_require_approval_exact_suffixes_and_intact_bytes() {
    let admin = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let intake = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let upload = intake
        .reserve_image("synthetic.jpg", "aco", 0, true)
        .await
        .unwrap();
    let id = upload.id.clone();
    let result = tokio::spawn(async move {
        let reader = MediaReader::connect(&std::env::var("MEDIA_READ_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let temp = tempfile::tempdir().unwrap();
        let quarantine = Quarantine::new(temp.path().join("private")).unwrap();
        let root = temp.path().join("objects");
        let store = PublicationStore::new(&root, &quarantine).unwrap();
        let app = board_media_http::router(board_media_http::AppState::new(
            reader.clone(),
            ApprovedFiles::open(&root).unwrap(),
            &board_config::Origin::parse("http://127.0.0.1:3002").unwrap(),
            &board_config::Origin::parse("http://127.0.0.1:3000").unwrap(),
        ));
        intake
            .begin_upload(&upload.id, &upload.capability)
            .await
            .unwrap();
        intake
            .finish_upload(&upload.id, &upload.capability, 100)
            .await
            .unwrap();
        let job = queue.claim().await.unwrap().unwrap();
        assert_eq!(job.id, upload.id);
        let token = job.lease_token.as_deref().unwrap();
        queue.extend_still_lease(&job.id, token).await.unwrap();
        let encoded = candidate().await;
        let md5 = encoded.retained().md5();
        let digest: Vec<u8> = (0..32)
            .step_by(2)
            .map(|i| u8::from_str_radix(&md5[i..i + 2], 16).unwrap())
            .collect();
        let output = StillOutput {
            format: MediaFormat::Jpeg,
            primary: metadata(encoded.retained()),
            variants: OutputVariants {
                md5: md5.into(),
                thumbnail: metadata(encoded.thumbnail()),
            },
            mobile: encoded.mobile().map(metadata),
            source: SourceProvenance {
                input_sha256: "a".repeat(64),
                input_bytes: 100,
                profile: SourceProfile::StillV6,
                retained_bytes: 32,
                md5: digest.try_into().unwrap(),
            },
        };
        let guard = store.try_lock().unwrap();
        let asset = queue
            .prepare_still_output(&job.id, token, &output)
            .await
            .unwrap();
        let urls = [
            format!("/media/{}.jpg", asset.id),
            format!("/media/{}.thumb.jpg", asset.id),
            format!("/media/{}.mobile.jpg", asset.id),
        ];
        for url in &urls {
            assert_eq!(
                app.clone()
                    .oneshot(request(url).body(Body::empty()).unwrap())
                    .await
                    .unwrap()
                    .status(),
                StatusCode::NOT_FOUND
            );
        }
        let object = asset.id.parse().unwrap();
        guard.install_still(object, &encoded).unwrap();
        guard.install_still_thumbnail(object, &encoded).unwrap();
        guard.install_still_mobile(object, &encoded).unwrap();
        queue
            .approve_output(&job.id, token, &asset.id)
            .await
            .unwrap();
        drop(guard);
        assert_eq!(
            reader.get_thumbnail(&asset.id).await.unwrap().output_format,
            MediaFormat::Jpeg
        );
        assert_eq!(reader.get_mobile(&asset.id).await.unwrap().width, 1024);
        for (url, expected) in urls.iter().zip([
            encoded.retained(),
            encoded.thumbnail(),
            encoded.mobile().unwrap(),
        ]) {
            let response = app
                .clone()
                .oneshot(request(url).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()["content-type"], "image/jpeg");
            assert_eq!(
                to_bytes(response.into_body(), 1024).await.unwrap().as_ref(),
                expected.bytes()
            );
            let response = app
                .clone()
                .oneshot(
                    request(url)
                        .header("if-none-match", format!("\"{}\"", expected.sha256()))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        }
        for suffix in ["png", "thumb.png", "gif", "mobile.png"] {
            assert_eq!(
                app.clone()
                    .oneshot(
                        request(&format!("/media/{}.{suffix}", asset.id))
                            .body(Body::empty())
                            .unwrap()
                    )
                    .await
                    .unwrap()
                    .status(),
                StatusCode::NOT_FOUND
            );
        }
        std::fs::write(root.join(format!("{}.mobile.jpg", asset.id)), b"changed").unwrap();
        let response = app
            .oneshot(
                request(&urls[2])
                    .header(
                        "if-none-match",
                        format!("\"{}\"", encoded.mobile().unwrap().sha256()),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    })
    .await;
    sqlx::query("DELETE FROM media.assets WHERE job_id=$1")
        .bind(&id)
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("DELETE FROM media.jobs WHERE id=$1")
        .bind(&id)
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    result.unwrap();
}
