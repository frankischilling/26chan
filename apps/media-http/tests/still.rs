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
async fn candidate(format: StillFormat) -> ValidatedStill {
    let policy = source_policy("aco", format, true, true).unwrap();
    let mut disk = vec![0; OUTPUT_DISK_BYTES as usize];
    disk[..8].copy_from_slice(if format == StillFormat::Gif {
        b"IBIMG007"
    } else {
        b"IBIMG006"
    });
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
    if format == StillFormat::Gif {
        disk[40..48].fill(0);
        disk[56..60].fill(0);
        disk[64..96].fill(0);
        disk[64..70].copy_from_slice(b"GIF89a");
        disk[95] = b';';
        disk[101..106].fill(0);
    }
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
async fn jpeg_and_gif_require_approval_exact_suffixes_and_intact_bytes() {
    exercise(StillFormat::Jpeg).await;
    exercise(StillFormat::Gif).await;
}
async fn exercise(format: StillFormat) {
    let gif = format == StillFormat::Gif;
    let extension = format.extension();
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
        .reserve_image(&format!("synthetic.{extension}"), "aco", 0, true)
        .await
        .unwrap();
    let id = upload.id.clone();
    let posted = std::sync::Arc::new(std::sync::atomic::AtomicI64::new(0));
    let owned_post = posted.clone();
    let owner = admin.clone();
    let identity = board_domain::poster_id::PosterIdKey::parse(&"8".repeat(64)).unwrap();
    let actor = *identity
        .public_posting_rate_identity("192.0.2.203".parse().unwrap())
        .as_bytes();
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
        let encoded = candidate(format).await;
        let md5 = encoded.retained().md5();
        let digest: Vec<u8> = (0..32)
            .step_by(2)
            .map(|i| u8::from_str_radix(&md5[i..i + 2], 16).unwrap())
            .collect();
        let output = StillOutput {
            format: if gif {
                MediaFormat::Gif
            } else {
                MediaFormat::Jpeg
            },
            primary: metadata(encoded.retained()),
            variants: OutputVariants {
                md5: md5.into(),
                thumbnail: metadata(encoded.thumbnail()),
            },
            mobile: encoded.mobile().map(metadata),
            source: SourceProvenance {
                input_sha256: "a".repeat(64),
                input_bytes: 100,
                profile: if gif {
                    SourceProfile::GifV7
                } else {
                    SourceProfile::StillV6
                },
                retained_bytes: 32,
                md5: digest.try_into().unwrap(),
            },
        };
        let guard = store.try_lock().unwrap();
        let asset = queue
            .prepare_still_output(&job.id, token, &output)
            .await
            .unwrap();
        let mut urls = vec![
            format!("/media/{}.{extension}", asset.id),
            format!("/media/{}.thumb.jpg", asset.id),
        ];
        if !gif {
            urls.push(format!("/media/{}.mobile.jpg", asset.id));
        }
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
        let mut files = vec![encoded.retained(), encoded.thumbnail()];
        if gif {
            assert!(reader.get_mobile(&asset.id).await.is_err());
        } else {
            assert_eq!(reader.get_mobile(&asset.id).await.unwrap().width, 1024);
            files.push(encoded.mobile().unwrap());
        }
        for (index, (url, expected)) in urls.iter().zip(&files).enumerate() {
            let response = app
                .clone()
                .oneshot(request(url).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response.headers()["content-type"],
                if gif && index == 0 {
                    "image/gif"
                } else {
                    "image/jpeg"
                }
            );
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
        for suffix in [
            "png",
            "thumb.png",
            if gif { "jpg" } else { "gif" },
            "mobile.png",
        ] {
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
        let public =
            board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
                .await
                .unwrap();
        let post = board_store::create_post_with_metadata(
            &public,
            "aco",
            0,
            &board_store::NewPost {
                name: String::new(),
                subject: "Owned still image".into(),
                comment: "Synthetic retained-image receipt".into(),
                deletion_hash: "fixture-hash".into(),
                sage: false,
            },
            Some(&board_store::post_media::NewAttachment {
                upload,
                spoiler: false,
            }),
            board_store::PostingContext {
                request_start: sqlx::query_scalar("SELECT clock_timestamp()")
                    .fetch_one(&owner)
                    .await
                    .unwrap(),
                peer: Some("192.0.2.203".parse().unwrap()),
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
        owned_post.store(post, std::sync::atomic::Ordering::SeqCst);
        let attachment = board_store::post_media::attachment(&public, post)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(attachment.has_mobile, !gif);
        let mut source_urls = vec![
            format!("/aco/{}.{extension}", attachment.tim),
            format!("/aco/{}s.jpg", attachment.tim),
        ];
        if !gif {
            source_urls.push(format!("/aco/{}m.jpg", attachment.tim));
        }
        for (index, (url, expected)) in source_urls.iter().zip(&files).enumerate() {
            let response = app
                .clone()
                .oneshot(request(url).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response.headers()["content-type"],
                if gif && index == 0 {
                    "image/gif"
                } else {
                    "image/jpeg"
                }
            );
            assert_eq!(
                to_bytes(response.into_body(), 1024).await.unwrap().as_ref(),
                expected.bytes()
            );
        }
        let corrupt = if gif { "thumb.jpg" } else { "mobile.jpg" };
        std::fs::write(root.join(format!("{}.{corrupt}", asset.id)), b"changed").unwrap();
        let last = files.len() - 1;
        let response = app
            .clone()
            .oneshot(
                request(&urls[last])
                    .header("if-none-match", format!("\"{}\"", files[last].sha256()))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        board_store::post_media::delete_attachment(&public, "aco", post)
            .await
            .unwrap();
        let deleted = board_store::post_media::attachment(&public, post)
            .await
            .unwrap()
            .unwrap();
        assert!(deleted.file_deleted);
        assert!(!deleted.has_mobile);
        for url in urls.iter().chain(&source_urls) {
            assert_eq!(
                app.clone()
                    .oneshot(request(url).body(Body::empty()).unwrap())
                    .await
                    .unwrap()
                    .status(),
                StatusCode::NOT_FOUND
            );
        }
        public.close().await;
    })
    .await;
    let post = posted.load(std::sync::atomic::Ordering::SeqCst);
    if post > 0 {
        let mut tx = admin.begin().await.unwrap();
        sqlx::query("SELECT slug FROM content.boards WHERE slug='aco' FOR UPDATE")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SELECT id FROM content.threads WHERE board='aco' AND id=$1 FOR UPDATE")
            .bind(post)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        for query in [
            "DELETE FROM content.post_media WHERE post_id=$1",
            "DELETE FROM post_secrets.deletion WHERE post_id=$1",
            "DELETE FROM content.posts WHERE board='aco' AND id=$1",
            "DELETE FROM content.threads WHERE board='aco' AND id=$1",
        ] {
            sqlx::query(query)
                .bind(post)
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        tx.commit().await.unwrap();
    }
    sqlx::query(
        "DELETE FROM post_secrets.posting_thread_actions WHERE board='aco' AND actor_hash=$1",
    )
    .bind(actor.as_slice())
    .execute(&admin)
    .await
    .unwrap();
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
