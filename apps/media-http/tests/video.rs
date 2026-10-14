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

// This fixture tests the host envelope/publication boundary, not codec admission.
// The synthetic twelve bytes deliberately are not a playable container.
async fn document(format: board_media::video::VideoFormat) -> board_media::video::ValidatedVideo {
    use board_media::video::{OUTPUT_DISK_BYTES, ValidatedVideo, VideoFormat};
    let mut wire = vec![0; OUTPUT_DISK_BYTES as usize];
    wire[..8].copy_from_slice(b"IBVID005");
    wire[8] = format.tag();
    wire[16..20].copy_from_slice(&300_u32.to_be_bytes());
    wire[20..24].copy_from_slice(&200_u32.to_be_bytes());
    wire[24..32].copy_from_slice(&500_000_u64.to_be_bytes());
    wire[40..44].copy_from_slice(&250_u32.to_be_bytes());
    wire[44..48].copy_from_slice(&166_u32.to_be_bytes());
    wire[48..56].copy_from_slice(&12_u64.to_be_bytes());
    match format {
        VideoFormat::Webm => wire[64..68].copy_from_slice(&[0x1a, 0x45, 0xdf, 0xa3]),
        VideoFormat::Mp4 => wire[68..72].copy_from_slice(b"ftyp"),
    }
    for pixel in wire[76..76 + 250 * 166 * 4].chunks_exact_mut(4) {
        pixel.copy_from_slice(&[255, 0, 0, 255]);
    }
    ValidatedVideo::read_disk(std::io::Cursor::new(wire), format)
        .await
        .unwrap()
}

fn request(method: &str, path: &str) -> axum::http::request::Builder {
    Request::builder()
        .method(method)
        .uri(path)
        .header("host", "127.0.0.1:3002")
}

#[tokio::test]
async fn video_approval_binds_format_and_checks_actual_bytes_before_conditional_http() {
    for format in [
        board_media::video::VideoFormat::Webm,
        board_media::video::VideoFormat::Mp4,
    ] {
        for reply in [false, true] {
            video_case(format, reply).await;
        }
    }
}

async fn video_case(format: board_media::video::VideoFormat, reply: bool) {
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
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) VALUES ($1,'WebM fixture','Owned retained WebM attachment',2000,100,100,10,10,10,0,0,0)")
        .bind(&board).execute(&admin).await.unwrap();
    let task_board = board.clone();
    let result =
        tokio::spawn(
            async move { exercise(task_admin, task_ids, &task_board, format, reply).await },
        )
        .await;
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

async fn exercise(
    admin: sqlx::PgPool,
    ids: Arc<Mutex<Vec<String>>>,
    board: &str,
    format: board_media::video::VideoFormat,
    reply: bool,
) {
    let ext = format.extension();
    let store_format = if format == board_media::video::VideoFormat::Webm {
        MediaFormat::Webm
    } else {
        MediaFormat::Mp4
    };
    let mime = store_format.content_type();
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
    let job = intake
        .reserve(&format!("owned-video.{}", ext.to_ascii_uppercase()))
        .await
        .unwrap();
    ids.lock().unwrap().push(job.id.clone());
    intake.begin_upload(&job.id, &job.capability).await.unwrap();
    intake
        .finish_upload(&job.id, &job.capability, 12)
        .await
        .unwrap();
    let claim = queue.claim().await.unwrap().unwrap();
    assert_eq!(claim.id, job.id, "Use an idle disposable queue");
    let token = claim.lease_token.as_deref().unwrap();
    let encoded = document(format).await;
    let thumbnail = encoded.preview().thumbnail().unwrap();
    let metadata = OutputMetadata {
        sha256: encoded.sha256().into(),
        bytes: encoded.len() as i64,
        width: 300,
        height: 200,
    };
    let variants = OutputVariants {
        md5: encoded.md5().into(),
        thumbnail: OutputMetadata {
            sha256: thumbnail.sha256().into(),
            bytes: thumbnail.len() as i64,
            width: 250,
            height: 166,
        },
    };
    let guard = store.try_lock().unwrap();
    let pending = queue
        .prepare_video_output(
            &job.id,
            token,
            &metadata,
            &variants,
            store_format,
            &board_store::media_assets::VideoMetadata {
                video_duration_micros: 500_000,
                video_has_audio: false,
            },
        )
        .await
        .unwrap();
    assert_eq!(pending.output_format, store_format);
    assert!(
        queue
            .prepare_output_with_variants(&job.id, token, &metadata, Some(&variants))
            .await
            .is_err()
    );
    for url in [
        format!("/media/{}.{ext}", pending.id),
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
        .install_video(pending.id.parse().unwrap(), &encoded)
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
        store_format
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
        format!("/media/{}.WEBM", approved.id),
        format!("/media/{}.thumb.{ext}", approved.id),
        format!(
            "/media/%{:02x}{}.{ext}",
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
    let url = format!("/media/{}.{ext}", approved.id);
    let response = app
        .clone()
        .oneshot(request("GET", &url).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-type"], mime);
    assert_eq!(response.headers()["x-content-type-options"], "nosniff");
    assert!(
        response.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("sandbox")
    );
    assert_eq!(
        response.headers()["content-disposition"],
        format!("inline; filename=\"{}.{ext}\"", approved.id)
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
                    .header("range", "bytes=0-3")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(response.headers()["content-type"], mime);
        assert!(to_bytes(response.into_body(), 1).await.unwrap().is_empty());
    }
    for (range, status, expected) in [
        (
            "bytes=0-3",
            StatusCode::PARTIAL_CONTENT,
            encoded.bytes()[..4].to_vec(),
        ),
        (
            "bytes=-2",
            StatusCode::PARTIAL_CONTENT,
            encoded.bytes()[10..].to_vec(),
        ),
        (
            "bytes=4-",
            StatusCode::PARTIAL_CONTENT,
            encoded.bytes()[4..].to_vec(),
        ),
        ("bytes=12-", StatusCode::RANGE_NOT_SATISFIABLE, Vec::new()),
    ] {
        let response = app
            .clone()
            .oneshot(
                request("GET", &url)
                    .header("range", range)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), status);
        if status == StatusCode::PARTIAL_CONTENT {
            assert_eq!(
                to_bytes(response.into_body(), 1000).await.unwrap().as_ref(),
                expected
            );
        } else {
            assert_eq!(response.headers()["content-range"], "bytes */12");
        }
    }
    let response = app
        .clone()
        .oneshot(
            request("HEAD", &url)
                .header("range", "bytes=0-3")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["content-length"], "12");
    assert!(to_bytes(response.into_body(), 0).await.unwrap().is_empty());
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
    for statement in [
        "UPDATE media.assets SET video_duration_micros=1 WHERE id=$1",
        "UPDATE media.assets SET video_has_audio=true WHERE id=$1",
    ] {
        let error = sqlx::query(statement)
            .bind(&approved.id)
            .execute(&writer)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501")
        );
    }
    writer.close().await;
    // WebM precedes the source image-only GIF policy. Check current board policy again at consumption.
    sqlx::query("UPDATE content.boards SET gif_only=true,video_enabled=true,max_image_bytes=8388608,max_image_dimension=2048 WHERE slug=$1")
        .bind(board).execute(&admin).await.unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let identity = board_domain::poster_id::PosterIdKey::parse(&"7".repeat(64)).unwrap();
    let thread_id = if reply {
        board_store::create_post_with_metadata(
            &public,
            board,
            0,
            &board_store::NewPost {
                name: String::new(),
                subject: "Owned parent".into(),
                comment: "Parent for video reply".into(),
                deletion_hash: "fixture-hash".into(),
                sage: false,
            },
            None,
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
        .unwrap()
    } else {
        0
    };
    // Board policy is authoritative at consumption, even after processing.
    sqlx::query("UPDATE content.boards SET video_enabled=false WHERE slug=$1")
        .bind(board)
        .execute(&admin)
        .await
        .unwrap();
    let denied = board_store::create_post_with_metadata(
        &public,
        board,
        thread_id,
        &board_store::NewPost {
            name: String::new(),
            subject: "Document fixture".into(),
            comment: "Owned WebM attachment".into(),
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
    sqlx::query("UPDATE content.boards SET video_enabled=true WHERE slug=$1")
        .bind(board)
        .execute(&admin)
        .await
        .unwrap();
    let post = board_store::create_post_with_metadata(
        &public,
        board,
        thread_id,
        &board_store::NewPost {
            name: String::new(),
            subject: "Document fixture".into(),
            comment: "Owned WebM attachment".into(),
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
    assert_eq!(attachment.output_format, store_format);
    assert_eq!(
        (attachment.thumbnail_width, attachment.thumbnail_height),
        if reply {
            (Some(125), Some(83))
        } else {
            (Some(250), Some(166))
        }
    );
    let post_url = format!("/{board}/{}.{ext}", attachment.tim);
    let post_thumbnail = format!("/{board}/{}s.jpg", attachment.tim);
    for (path, mime, expected) in [
        (&post_url, mime, encoded.bytes().to_vec()),
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
    // WebM and both PNG thumbnails remain outside the drawing CORS path.
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
                        .header("range", "bytes=0-3")
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
        format!("/{board}/0{}.{ext}", attachment.tim),
        format!("/{board}/{}.WEBM", attachment.tim),
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
        root.join(format!("{}.{ext}", approved.id)),
        vec![0; encoded.len() as usize],
    )
    .unwrap();
    for path in [&url, &post_url] {
        let response = app
            .clone()
            .oneshot(
                request("GET", path)
                    .header("if-none-match", &etag)
                    .header("range", "bytes=0-3")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(!response.headers().contains_key("etag"));
    }
    std::fs::write(root.join(format!("{}.{ext}", approved.id)), encoded.bytes()).unwrap();
    board_store::post_media::delete_attachment(&public, board, post)
        .await
        .unwrap();
    for path in [&post_url, &post_thumbnail] {
        let response = app
            .clone()
            .oneshot(
                request("GET", path)
                    .header("if-none-match", &etag)
                    .header("range", "bytes=0-3")
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
                    .header("range", "bytes=0-3")
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
