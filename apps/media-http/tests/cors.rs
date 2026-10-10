#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderValue, Method, Request, StatusCode, header},
    response::Response,
};
use board_media::{ApprovedFiles, PublicationStore, Quarantine, ValidatedOutput};
use board_store::{media::MediaQueue, media_assets::MediaReader, media_intake::IntakeStore};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

const CANONICAL_PUBLIC: &str = "http://localhost:3000";

async fn send(
    app: &Router,
    method: Method,
    path: &str,
    origins: &[&str],
    condition: Option<&str>,
) -> Response {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, "127.0.0.1:3002")
        .body(Body::empty())
        .unwrap();
    for value in origins {
        // Use append explicitly: even two identical Origin fields are rejected.
        request
            .headers_mut()
            .append(header::ORIGIN, HeaderValue::from_str(value).unwrap());
    }
    if let Some(condition) = condition {
        request.headers_mut().insert(
            header::IF_NONE_MATCH,
            HeaderValue::from_str(condition).unwrap(),
        );
    }
    app.clone().oneshot(request).await.unwrap()
}

fn assert_cors(response: &Response, eligible: bool, allowed: bool) {
    let headers = response.headers();
    assert_eq!(
        headers.get(header::VARY).cloned(),
        eligible.then_some(HeaderValue::from_static("Origin")),
    );
    assert_eq!(
        headers.get(header::ACCESS_CONTROL_ALLOW_ORIGIN).cloned(),
        allowed.then_some(HeaderValue::from_static(CANONICAL_PUBLIC)),
    );
    assert!(!headers.contains_key(header::ACCESS_CONTROL_ALLOW_CREDENTIALS));
    assert!(!headers.contains_key(header::ACCESS_CONTROL_ALLOW_METHODS));
    assert!(!headers.contains_key(header::ACCESS_CONTROL_ALLOW_HEADERS));
    assert!(!headers.contains_key(header::ACCESS_CONTROL_MAX_AGE));
    assert_eq!(headers["cross-origin-resource-policy"], "cross-origin");
}

#[tokio::test]
async fn drawing_png_cors_requires_visible_verified_bytes_and_one_canonical_origin() {
    let admin = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT substr(replace(gen_random_uuid()::text,'-',''),1,10)")
            .fetch_one(&admin)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) VALUES ($1,'Drawing CORS fixture','Owned synthetic PNG',2000,100,100,10,10,10,0,0,0)")
        .bind(&board)
        .execute(&admin)
        .await
        .unwrap();
    let ids = Arc::new(Mutex::new(Vec::<String>::new()));
    let exercise_ids = ids.clone();
    let exercise_admin = admin.clone();
    let exercise_board = board.clone();
    // Preserve fixture cleanup when an assertion inside the exercise panics.
    let outcome =
        tokio::spawn(async move { exercise(exercise_admin, exercise_ids, &exercise_board).await })
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
    let jobs = ids.lock().unwrap().clone();
    sqlx::query("DELETE FROM media.assets WHERE job_id=ANY($1)")
        .bind(&jobs)
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("DELETE FROM media.jobs WHERE id=ANY($1)")
        .bind(&jobs)
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    outcome.unwrap();
}

async fn exercise(admin: sqlx::PgPool, ids: Arc<Mutex<Vec<String>>>, board: &str) {
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let intake = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let reader = MediaReader::connect(&std::env::var("MEDIA_READ_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(temp.path().join("private")).unwrap();
    let root = temp.path().join("objects");
    let store = PublicationStore::new(&root, &quarantine).unwrap();
    let output = {
        let mut pixels = b"IBRGBA01\0\0\0\x01\0\0\0\x01".to_vec();
        pixels.extend_from_slice(&[10, 40, 90, 255]);
        ValidatedOutput::read(pixels.as_slice()).await.unwrap()
    };
    let job = intake.reserve("drawing.png").await.unwrap();
    ids.lock().unwrap().push(job.id.clone());
    intake.begin_upload(&job.id, &job.capability).await.unwrap();
    intake
        .finish_upload(&job.id, &job.capability, 4)
        .await
        .unwrap();
    let claim = queue.claim().await.unwrap().unwrap();
    assert_eq!(claim.id, job.id, "Use an idle disposable queue");
    let approved = board_media_admin::publish(
        &queue,
        &store,
        &job.id,
        claim.lease_token.as_deref().unwrap(),
        &output,
    )
    .await
    .unwrap();

    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let identity = board_domain::poster_id::PosterIdKey::parse(&"7".repeat(64)).unwrap();
    let post = board_store::create_post_with_metadata(
        &public,
        board,
        0,
        &board_store::NewPost {
            name: String::new(),
            subject: "Drawing fixture".into(),
            comment: "Owned PNG attachment".into(),
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
            peer: Some("192.0.2.211".parse().unwrap()),
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
    let url = format!("/{board}/{}.png", attachment.tim);
    let thumb = format!("/{board}/{}s.jpg", attachment.tim);
    let object = format!("/media/{}.png", approved.id);
    let canonical = board_config::Origin::parse("HTTP://LOCALHOST:3000/").unwrap();
    assert_eq!(canonical.as_string(), CANONICAL_PUBLIC);
    let media = board_config::Origin::parse("http://127.0.0.1:3002").unwrap();
    let app = board_media_http::router(board_media_http::AppState::new(
        reader.clone(),
        ApprovedFiles::open(&root).unwrap(),
        &media,
        &canonical,
    ));
    let disk = root.join(format!("{}.png", approved.id));
    let png = std::fs::read(&disk).unwrap();

    let response = send(&app, Method::GET, &url, &[CANONICAL_PUBLIC], None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_cors(&response, true, true);
    assert_eq!(response.headers()[header::CONTENT_TYPE], "image/png");
    let etag = response.headers()[header::ETAG]
        .to_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        to_bytes(response.into_body(), 5_242_880)
            .await
            .unwrap()
            .as_ref(),
        png.as_slice()
    );
    for method in [Method::GET, Method::HEAD] {
        let response = send(&app, method.clone(), &url, &[CANONICAL_PUBLIC], None).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_cors(&response, true, true);
        if method == Method::HEAD {
            assert_eq!(
                response.headers()[header::CONTENT_LENGTH],
                png.len().to_string()
            );
            assert!(to_bytes(response.into_body(), 0).await.unwrap().is_empty());
        } else {
            assert_eq!(
                to_bytes(response.into_body(), 5_242_880)
                    .await
                    .unwrap()
                    .as_ref(),
                png.as_slice()
            );
        }

        let response = send(&app, method.clone(), &url, &[CANONICAL_PUBLIC], Some(&etag)).await;
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        assert_cors(&response, true, true);
        assert!(to_bytes(response.into_body(), 0).await.unwrap().is_empty());

        let response = send(&app, method.clone(), &url, &[], Some(&etag)).await;
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        assert_cors(&response, true, false);
        assert!(to_bytes(response.into_body(), 0).await.unwrap().is_empty());

        let response = send(&app, method.clone(), &url, &[], None).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_cors(&response, true, false);
        if method == Method::HEAD {
            assert_eq!(
                response.headers()[header::CONTENT_LENGTH],
                png.len().to_string()
            );
            assert!(to_bytes(response.into_body(), 0).await.unwrap().is_empty());
        } else {
            assert_eq!(
                to_bytes(response.into_body(), 5_242_880)
                    .await
                    .unwrap()
                    .as_ref(),
                png.as_slice()
            );
        }

        let response = send(
            &app,
            method.clone(),
            &url,
            &["https://elsewhere.example"],
            Some(&etag),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        assert_cors(&response, true, false);
        assert!(to_bytes(response.into_body(), 0).await.unwrap().is_empty());
    }
    for origins in [
        vec!["http://LOCALHOST:3000"],
        vec!["HTTP://localhost:3000"],
        vec!["http://localhost:3000/"],
        vec!["http://127.0.0.1:3000"],
        vec!["null"],
        vec!["*"],
        vec!["http://localhost:3000, https://elsewhere.example"],
        vec![CANONICAL_PUBLIC, CANONICAL_PUBLIC],
        vec![CANONICAL_PUBLIC, "https://elsewhere.example"],
    ] {
        let response = send(&app, Method::GET, &url, &origins, None).await;
        assert_eq!(response.status(), StatusCode::OK, "{origins:?}");
        assert_cors(&response, true, false);
        drop(response);
        let response = send(&app, Method::HEAD, &url, &origins, Some(&etag)).await;
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED, "{origins:?}");
        assert_cors(&response, true, false);
        assert!(to_bytes(response.into_body(), 0).await.unwrap().is_empty());
    }
    for path in [&thumb, &object] {
        let initial = send(&app, Method::GET, path, &[CANONICAL_PUBLIC], None).await;
        assert_eq!(initial.status(), StatusCode::OK);
        assert_cors(&initial, false, false);
        let path_etag = initial.headers()[header::ETAG].to_str().unwrap().to_owned();
        drop(initial);
        for method in [Method::GET, Method::HEAD] {
            for condition in [None, Some(path_etag.as_str())] {
                let response =
                    send(&app, method.clone(), path, &[CANONICAL_PUBLIC], condition).await;
                assert_eq!(
                    response.status(),
                    if condition.is_some() {
                        StatusCode::NOT_MODIFIED
                    } else {
                        StatusCode::OK
                    },
                    "{method} {path}"
                );
                assert_cors(&response, false, false);
                drop(response);
            }
        }
    }
    for (method, path, status) in [
        (
            Method::GET,
            format!("/{board}/{}.gif", attachment.tim),
            StatusCode::NOT_FOUND,
        ),
        (
            Method::GET,
            format!("{url}?download=1"),
            StatusCode::BAD_REQUEST,
        ),
        (Method::OPTIONS, url.clone(), StatusCode::METHOD_NOT_ALLOWED),
        (Method::POST, url.clone(), StatusCode::METHOD_NOT_ALLOWED),
        (Method::GET, "/healthz".into(), StatusCode::OK),
        (Method::GET, "/readyz".into(), StatusCode::OK),
    ] {
        let response = send(&app, method, &path, &[CANONICAL_PUBLIC], None).await;
        assert_eq!(response.status(), status, "{path}");
        assert_cors(&response, false, false);
        drop(response);
    }
    let preflight = Request::builder()
        .method(Method::OPTIONS)
        .uri(&url)
        .header(header::HOST, "127.0.0.1:3002")
        .header(header::ORIGIN, CANONICAL_PUBLIC)
        .header(header::ACCESS_CONTROL_REQUEST_METHOD, "GET")
        .header(header::ACCESS_CONTROL_REQUEST_HEADERS, "content-type")
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(preflight).await.unwrap();
    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_cors(&response, false, false);
    let wrong_host = Request::builder()
        .uri(&url)
        .header(header::HOST, "invalid.example")
        .header(header::ORIGIN, CANONICAL_PUBLIC)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(wrong_host).await.unwrap();
    assert_eq!(response.status(), StatusCode::MISDIRECTED_REQUEST);
    assert_cors(&response, false, false);

    // Even a matching validator cannot expose CORS once disk verification fails.
    std::fs::write(&disk, vec![0; png.len()]).unwrap();
    let response = send(&app, Method::GET, &url, &[CANONICAL_PUBLIC], Some(&etag)).await;
    assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_cors(&response, false, false);
    assert!(!response.headers().contains_key(header::ETAG));
    std::fs::write(&disk, &png).unwrap();

    sqlx::query("UPDATE content.boards SET staff_only=true WHERE slug=$1")
        .bind(board)
        .execute(&admin)
        .await
        .unwrap();
    let response = send(&app, Method::GET, &url, &[CANONICAL_PUBLIC], Some(&etag)).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_cors(&response, false, false);
    sqlx::query("UPDATE content.boards SET staff_only=false WHERE slug=$1")
        .bind(board)
        .execute(&admin)
        .await
        .unwrap();

    let mut deletion = admin.begin().await.unwrap();
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(board)
        .fetch_one(&mut *deletion)
        .await
        .unwrap();
    sqlx::query("UPDATE content.posts SET deleted=true WHERE board=$1 AND id=$2")
        .bind(board)
        .bind(post)
        .execute(&mut *deletion)
        .await
        .unwrap();
    deletion.commit().await.unwrap();
    for method in [Method::GET, Method::HEAD] {
        let response = send(&app, method, &url, &[CANONICAL_PUBLIC], Some(&etag)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_cors(&response, false, false);
    }
    public.close().await;
    reader.close().await;
}
