#![cfg(feature = "database-tests")]

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use board_media::{ApprovedFiles, PublicationStore, Quarantine};
use board_store::{
    media::MediaQueue,
    media_assets::{MediaFormat, MediaReader, OutputMetadata, OutputVariants},
    media_intake::IntakeStore,
};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

// This HTTP fixture exercises retained-byte serving, not parser admission.
// Guest parser tests use complete synthetic PDFs independently.
const DOCUMENT: &[u8] = b"%PDF-1.4\nowned retained-byte HTTP fixture\n%%EOF\n";
async fn document() -> board_media::pdf::RetainedPdf {
    use std::io::{Seek, SeekFrom, Write};
    let mut input = tempfile::tempfile().unwrap();
    input.write_all(DOCUMENT).unwrap();
    input.seek(SeekFrom::Start(0)).unwrap();
    let snapshot = board_media::InputSnapshot::read(input, DOCUMENT.len() as u64)
        .await
        .unwrap();
    let mut pixels = b"IBRGBA01".to_vec();
    pixels.extend_from_slice(&1u32.to_be_bytes());
    pixels.extend_from_slice(&1u32.to_be_bytes());
    pixels.extend_from_slice(&[255, 0, 0, 255]);
    let preview = board_media::ValidatedOutput::read(pixels.as_slice())
        .await
        .unwrap();
    board_media::pdf::RetainedPdf::from_processed_snapshot(snapshot, preview).unwrap()
}

fn request(method: &str, path: &str) -> axum::http::request::Builder {
    Request::builder()
        .method(method)
        .uri(path)
        .header("host", "127.0.0.1:3002")
}

#[tokio::test]
async fn pdf_approval_binds_format_and_checks_actual_bytes_before_conditional_http() {
    let admin = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let ids = Arc::new(Mutex::new(Vec::<String>::new()));
    let task_ids = ids.clone();
    let task_admin = admin.clone();
    let board: String =
        sqlx::query_scalar("SELECT substr(replace(gen_random_uuid()::text,'-',''),1,10)")
            .fetch_one(&admin)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) VALUES ($1,'PDF fixture','Owned retained PDF attachment',2000,100,100,10,10,10,0,0,0)")
        .bind(&board).execute(&admin).await.unwrap();
    let task_board = board.clone();
    let result =
        tokio::spawn(async move { exercise(task_admin, task_ids, &task_board).await }).await;
    let mut cleanup = admin.begin().await.unwrap();
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(&board)
        .fetch_one(&mut *cleanup)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM content.threads WHERE board=$1 ORDER BY id FOR UPDATE")
        .bind(&board)
        .fetch_all(&mut *cleanup)
        .await
        .unwrap();
    for statement in [
        "DELETE FROM content.post_media WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(statement)
            .bind(&board)
            .execute(&mut *cleanup)
            .await
            .unwrap();
    }
    cleanup.commit().await.unwrap();
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

async fn exercise(admin: sqlx::PgPool, ids: Arc<Mutex<Vec<String>>>, board: &str) {
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let reader = MediaReader::connect(&std::env::var("MEDIA_READ_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(temp.path().join("private")).unwrap();
    let root = temp.path().join("objects");
    let store = PublicationStore::new(&root, &quarantine).unwrap();
    let files = ApprovedFiles::open(&root).unwrap();
    let app = board_media_http::router(board_media_http::AppState::new(
        reader.clone(),
        ApprovedFiles::open(&root).unwrap(),
        &board_config::Origin::parse("http://127.0.0.1:3002").unwrap(),
        &board_config::Origin::parse("http://127.0.0.1:3000").unwrap(),
    ));
    let intake = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let job = intake.reserve("owned-document.PDF").await.unwrap();
    ids.lock().unwrap().push(job.id.clone());
    intake.begin_upload(&job.id, &job.capability).await.unwrap();
    intake
        .finish_upload(&job.id, &job.capability, DOCUMENT.len() as u64)
        .await
        .unwrap();
    let claim = queue.claim().await.unwrap().unwrap();
    assert_eq!(claim.id, job.id, "Use an idle disposable queue");
    let token = claim.lease_token.as_deref().unwrap();
    let encoded = document().await;
    let thumbnail = encoded.preview().thumbnail().unwrap();
    let metadata = OutputMetadata {
        sha256: encoded.sha256().into(),
        bytes: encoded.len() as i64,
        width: 1,
        height: 1,
    };
    let variants = OutputVariants {
        md5: encoded.md5().into(),
        thumbnail: OutputMetadata {
            sha256: thumbnail.sha256().into(),
            bytes: thumbnail.len() as i64,
            width: 1,
            height: 1,
        },
    };
    let guard = store.try_lock().unwrap();
    let pending = queue
        .prepare_pdf_output(&job.id, token, &metadata, &variants)
        .await
        .unwrap();
    assert_eq!(pending.output_format, MediaFormat::Pdf);
    assert!(
        queue
            .prepare_output_with_variants(&job.id, token, &metadata, Some(&variants))
            .await
            .is_err()
    );
    for url in [
        format!("/media/{}.pdf", pending.id),
        format!("/media/{}.png", pending.id),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(request("GET", &url).body(Body::empty()).unwrap())
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
    }
    guard
        .install_pdf(pending.id.parse().unwrap(), &encoded)
        .unwrap();
    guard
        .install_thumbnail(pending.id.parse().unwrap(), &thumbnail)
        .unwrap();
    drop(guard);
    let approved = queue
        .approve_output(&job.id, token, &pending.id)
        .await
        .unwrap();
    assert_eq!(approved, pending);
    assert_eq!(
        queue
            .approve_output(&job.id, token, &pending.id)
            .await
            .unwrap(),
        approved
    );
    assert_eq!(
        reader.get(&approved.id).await.unwrap().output_format,
        MediaFormat::Pdf
    );
    assert_eq!(
        reader
            .get_thumbnail(&approved.id)
            .await
            .unwrap()
            .output_format,
        MediaFormat::Png
    );
    assert_eq!(
        board_media_admin::read_approved(&reader, &files, &approved.id)
            .await
            .unwrap(),
        encoded.bytes()
    );
    assert!(!root.join(format!("{}.png", approved.id)).exists());
    let source_profile: Option<String> =
        sqlx::query_scalar("SELECT source_profile FROM media.assets WHERE id=$1")
            .bind(&approved.id)
            .fetch_one(&admin)
            .await
            .unwrap();
    assert!(source_profile.is_none());
    for url in [
        format!("/media/{}.png", approved.id),
        format!("/media/{}.jpg", approved.id),
        format!("/media/{}.PDF", approved.id),
        format!("/media/{}.thumb.pdf", approved.id),
        format!(
            "/media/%{:02x}{}.pdf",
            approved.id.as_bytes()[0],
            &approved.id[1..]
        ),
    ] {
        let response = app
            .clone()
            .oneshot(request("GET", &url).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{url}");
    }
    let url = format!("/media/{}.pdf", approved.id);
    let response = app
        .clone()
        .oneshot(request("GET", &url).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], "application/pdf");
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert!(
        response.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("sandbox")
    );
    assert_eq!(
        response.headers()["content-disposition"],
        format!("inline; filename=\"{}.pdf\"", approved.id)
    );
    assert!(!response.headers().contains_key("set-cookie"));
    let etag = response.headers()["etag"].clone();
    assert_eq!(
        to_bytes(response.into_body(), 20_971_520)
            .await
            .unwrap()
            .as_ref(),
        encoded.bytes()
    );
    for method in ["GET", "HEAD"] {
        let response = app
            .clone()
            .oneshot(
                request(method, &url)
                    .header("if-none-match", &etag)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(response.headers()["content-type"], "application/pdf");
        assert!(to_bytes(response.into_body(), 1).await.unwrap().is_empty());
    }
    let thumbnail_url = format!("/media/{}.thumb.png", approved.id);
    let response = app
        .clone()
        .oneshot(request("GET", &thumbnail_url).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.headers()["content-type"], "image/png");
    assert_eq!(
        to_bytes(response.into_body(), 5_242_880)
            .await
            .unwrap()
            .as_ref(),
        files
            .read_thumbnail(
                approved.id.parse().unwrap(),
                thumbnail.sha256(),
                thumbnail.len()
            )
            .unwrap()
    );
    let writer = sqlx::PgPool::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    // Both the restricted writer and the migration identity must preserve format.
    for pool in [&writer, &admin] {
        let error = sqlx::query("UPDATE media.assets SET output_format='png' WHERE id=$1")
            .bind(&approved.id)
            .execute(pool)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501")
        );
    }
    writer.close().await;
    // PDF precedes the source image-only GIF policy. Charge the exact retained input.
    sqlx::query("UPDATE content.boards SET gif_only=true,pdf_enabled=true,max_image_bytes=8388608,max_image_dimension=1 WHERE slug=$1")
        .bind(board).execute(&admin).await.unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let identity = board_domain::poster_id::PosterIdKey::parse(&"7".repeat(64)).unwrap();
    // Board policy is authoritative at consumption, even after processing.
    sqlx::query("UPDATE content.boards SET pdf_enabled=false WHERE slug=$1")
        .bind(board)
        .execute(&admin)
        .await
        .unwrap();
    let denied = board_store::create_post_with_metadata(
        &public,
        board,
        0,
        &board_store::NewPost {
            name: String::new(),
            subject: "Document fixture".into(),
            comment: "Owned PDF attachment".into(),
            deletion_hash: "fixture-hash".into(),
            sage: false,
        },
        Some(&board_store::post_media::NewAttachment {
            upload: board_store::media_intake::IntakeReservation {
                id: job.id.clone(),
                capability: job.capability.clone(),
            },
            spoiler: false,
        }),
        board_store::PostingContext {
            request_start: sqlx::query_scalar("SELECT clock_timestamp()")
                .fetch_one(&admin)
                .await
                .unwrap(),
            peer: Some("192.0.2.202".parse().unwrap()),
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
    .unwrap_err();
    assert!(matches!(
        &denied,
        board_store::StoreError::Invalid("Error: Corrupted file or unsupported file type.")
    ));
    assert_eq!(
        intake.status(&job.id, &job.capability).await.unwrap().state,
        "published"
    );
    sqlx::query("UPDATE content.boards SET pdf_enabled=true WHERE slug=$1")
        .bind(board)
        .execute(&admin)
        .await
        .unwrap();
    let post = board_store::create_post_with_metadata(
        &public,
        board,
        0,
        &board_store::NewPost {
            name: String::new(),
            subject: "Document fixture".into(),
            comment: "Owned PDF attachment".into(),
            deletion_hash: "fixture-hash".into(),
            sage: false,
        },
        Some(&board_store::post_media::NewAttachment {
            upload: job,
            spoiler: false,
        }),
        board_store::PostingContext {
            request_start: sqlx::query_scalar("SELECT clock_timestamp()")
                .fetch_one(&admin)
                .await
                .unwrap(),
            peer: Some("192.0.2.202".parse().unwrap()),
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
    let attachment = board_store::post_media::attachment(&public, post)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(attachment.output_format, MediaFormat::Pdf);
    let post_url = format!("/{board}/{}.pdf", attachment.tim);
    let post_thumbnail = format!("/{board}/{}s.jpg", attachment.tim);
    for (path, mime, expected) in [
        (&post_url, "application/pdf", encoded.bytes().to_vec()),
        (
            &post_thumbnail,
            "image/png",
            files
                .read_thumbnail(
                    approved.id.parse().unwrap(),
                    thumbnail.sha256(),
                    thumbnail.len(),
                )
                .unwrap(),
        ),
    ] {
        let response = app
            .clone()
            .oneshot(request("GET", path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], mime);
        assert_eq!(
            to_bytes(response.into_body(), 20_971_520)
                .await
                .unwrap()
                .as_ref(),
            expected
        );
    }
    // PDF and both PNG thumbnails remain outside the drawing CORS path.
    for path in [&url, &thumbnail_url, &post_url, &post_thumbnail] {
        let response = app
            .clone()
            .oneshot(
                request("GET", path)
                    .header("origin", "http://127.0.0.1:3000")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            !response
                .headers()
                .contains_key("access-control-allow-origin")
        );
        assert!(!response.headers().contains_key("vary"));
        let etag = response.headers()["etag"].clone();
        drop(response);
        for method in ["GET", "HEAD"] {
            let response = app
                .clone()
                .oneshot(
                    request(method, path)
                        .header("origin", "http://127.0.0.1:3000")
                        .header("if-none-match", &etag)
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
            assert!(
                !response
                    .headers()
                    .contains_key("access-control-allow-origin")
            );
            assert!(!response.headers().contains_key("vary"));
            assert!(to_bytes(response.into_body(), 0).await.unwrap().is_empty());
        }
    }
    for path in [
        format!("/{board}/{}.png", attachment.tim),
        format!("/{board}/0{}.pdf", attachment.tim),
        format!("/{board}/{}.PDF", attachment.tim),
    ] {
        assert_eq!(
            app.clone()
                .oneshot(request("GET", &path).body(Body::empty()).unwrap())
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
    }
    std::fs::write(
        root.join(format!("{}.pdf", approved.id)),
        vec![0; encoded.len() as usize],
    )
    .unwrap();
    for path in [&url, &post_url] {
        let response = app
            .clone()
            .oneshot(
                request("GET", path)
                    .header("if-none-match", &etag)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(!response.headers().contains_key("etag"));
    }
    std::fs::write(root.join(format!("{}.pdf", approved.id)), encoded.bytes()).unwrap();
    board_store::post_media::delete_attachment(&public, board, post)
        .await
        .unwrap();
    for path in [&post_url, &post_thumbnail] {
        let response = app
            .clone()
            .oneshot(
                request("GET", path)
                    .header("if-none-match", &etag)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(!response.headers().contains_key("etag"));
    }
    public.close().await;
    sqlx::query("DELETE FROM media.assets WHERE id=$1")
        .bind(&approved.id)
        .execute(&admin)
        .await
        .unwrap();
    for path in [&url, &thumbnail_url] {
        let response = app
            .clone()
            .oneshot(
                request("GET", path)
                    .header("if-none-match", &etag)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert!(!response.headers().contains_key("etag"));
    }
    reader.close().await;
}
