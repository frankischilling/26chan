#![cfg(feature = "database-tests")]
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    response::Response,
};
use board_store::{
    media::MediaQueue,
    media_assets::{MediaReader, OutputMetadata},
    media_intake::IntakeStore,
};
use bytes::Bytes;
use sqlx::PgPool;
use tower::ServiceExt;

fn post_request(path: &str, body: String) -> Request<Body> {
    Request::post(path)
        .header("origin", "http://127.0.0.1:3000")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(body))
        .unwrap()
}
fn multipart(board: &str, data: Vec<u8>, extra: bool) -> Request<Body> {
    let head = format!(
        "--boundary\r\nContent-Disposition: form-data; name=\"resto\"\r\n\r\n0\r\n--boundary\r\nContent-Disposition: form-data; name=\"upfile\"; filename=\"<b>{board}</b>.png\"\r\nContent-Type: image/png\r\n\r\n"
    );
    let tail = if extra {
        "\r\n--boundary\r\nContent-Disposition: form-data; name=\"extra\"\r\n\r\nunexpected\r\n--boundary--\r\n"
    } else {
        "\r\n--boundary--\r\n"
    };
    let chunks: Vec<Result<Bytes, std::io::Error>> = std::iter::once(Ok(Bytes::from(head)))
        .chain(data.chunks(4096).map(|s| Ok(Bytes::copy_from_slice(s))))
        .chain(std::iter::once(Ok(Bytes::from_static(tail.as_bytes()))))
        .collect();
    Request::post(format!("/{board}/upload"))
        .header("origin", "http://127.0.0.1:3000")
        .header("content-type", "multipart/form-data; boundary=boundary")
        .body(Body::from_stream(futures_util::stream::iter(chunks)))
        .unwrap()
}
async fn html(response: Response, status: StatusCode) -> String {
    assert_eq!(response.status(), status);
    String::from_utf8(
        to_bytes(response.into_body(), 262_144)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}
fn hidden(page: &str, key: &str) -> String {
    page.split(&format!("name=\"{key}\" value=\""))
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .into()
}
fn receipt(id: &str, capability: &str) -> String {
    format!("upload_id={id}&upload_capability={capability}&resto=0")
}

#[tokio::test]
async fn real_intake_streaming_status_posting_and_file_deletion() {
    let admin = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT substr(replace(gen_random_uuid()::text,'-',''),1,10)")
            .fetch_one(&admin)
            .await
            .unwrap();
    let filename = format!("<b>{board}</b>.png");
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit) VALUES ($1,'Image test','Synthetic',2000,100,100,100,10,3)")
        .bind(&board).execute(&admin).await.unwrap();
    let root = tempfile::tempdir().unwrap();
    let intake = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let intake_app = board_media_intake::router(
        board_media_intake::AppState::new(
            intake.clone(),
            board_media::Quarantine::new(root.path()).unwrap(),
            "a".repeat(64),
        )
        .unwrap(),
    );
    let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, intake_app)
            .with_graceful_shutdown(async {
                let _ = stopped.await;
            })
            .await
            .unwrap();
    });
    let settings = board_config::PublicMediaSettings::development(
        &address.to_string(),
        &"a".repeat(64),
        "http://localhost:3002",
    )
    .unwrap();
    board_public::media_ready(&settings).await.unwrap();
    let app = board_public::routers_with_media(
        public.clone(),
        "http://127.0.0.1:3000".into(),
        false,
        Some(settings),
    )
    .0;
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let reader = MediaReader::connect(&std::env::var("MEDIA_READ_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let test_board = board.clone();
    let path = root.path().to_owned();
    let work_admin = admin.clone();
    let result = tokio::spawn(async move {
        exercise(app, &test_board, queue, reader, intake, &path, &work_admin).await
    })
    .await;
    let _ = stop.send(());
    server.await.unwrap();
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
    public.close().await;
    admin.close().await;
    result.unwrap();
}

async fn exercise(
    app: Router,
    board: &str,
    queue: MediaQueue,
    reader: MediaReader,
    intake: IntakeStore,
    root: &std::path::Path,
    admin: &PgPool,
) {
    let response = app
        .clone()
        .oneshot(
            Request::get(format!("/{board}/"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(
        response.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("img-src http://localhost:3002;")
    );
    let page = html(response, StatusCode::OK).await;
    assert!(page.contains("enctype=\"multipart/form-data\""));
    let input = vec![b'x'; 512_000]; // Intentionally opaque to the public/intake processes.
    let response = app
        .clone()
        .oneshot(multipart(board, input.clone(), false))
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert!(!response.headers().contains_key("location"));
    let page = html(response, StatusCode::OK).await;
    assert!(
        !page.contains(&"a".repeat(64)),
        "Service credentials must not enter HTML"
    );
    let id = hidden(&page, "upload_id");
    let capability = hidden(&page, "upload_capability");
    assert_eq!(
        std::fs::read(root.join(format!("{id}.input"))).unwrap(),
        input
    );
    let status = intake.status(&id, &capability).await.unwrap();
    assert_eq!(status.state, "queued");
    let body = receipt(&id, &capability);
    let pending = html(
        app.clone()
            .oneshot(post_request(
                &format!("/{board}/upload/status"),
                body.clone(),
            ))
            .await
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    assert!(pending.contains("still being processed"));
    assert!(!pending.contains("Post with image"));
    let posting =
        format!("{body}&name=Test&sub=Image&com=An+image&password=synthetic-password-123");
    assert_eq!(
        app.clone()
            .oneshot(post_request(&format!("/{board}/post"), posting.clone()))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    // Transport/storage integration: synthetic approval uses the real coordinator
    // role. The native Firecracker/browser qualification is a separate test.
    let job = queue.claim().await.unwrap().unwrap();
    assert_eq!(job.id, id, "Requires an idle disposable queue");
    let token = job.lease_token.unwrap();
    let asset = queue
        .prepare_output(
            &id,
            &token,
            &OutputMetadata {
                sha256: "c".repeat(64),
                bytes: 123,
                width: 10,
                height: 20,
            },
        )
        .await
        .unwrap();
    queue.approve_output(&id, &token, &asset.id).await.unwrap();
    let approved = html(
        app.clone()
            .oneshot(post_request(
                &format!("/{board}/upload/status"),
                body.clone(),
            ))
            .await
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    assert!(approved.contains("Post with image"));
    sqlx::query(
        "UPDATE media.jobs SET created_at=clock_timestamp()-interval '3 hours' WHERE id=$1",
    )
    .bind(&id)
    .execute(admin)
    .await
    .unwrap();
    assert_eq!(
        app.clone()
            .oneshot(post_request(
                &format!("/{board}/upload/status"),
                body.clone()
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    sqlx::query("UPDATE media.jobs SET created_at=clock_timestamp() WHERE id=$1")
        .bind(&id)
        .execute(admin)
        .await
        .unwrap();
    let response = app
        .clone()
        .oneshot(post_request(&format!("/{board}/post"), posting.clone()))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let location = response.headers()["location"].to_str().unwrap().to_owned();
    assert!(!location.contains(&id) && !location.contains(&capability));
    let thread: i64 = location.split("#p").nth(1).unwrap().parse().unwrap();
    for action in ["status", "cancel"] {
        assert_eq!(
            app.clone()
                .oneshot(post_request(
                    &format!("/{board}/upload/{action}"),
                    body.clone()
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::CONFLICT
        );
    }
    for path in [
        format!("/{board}/thread/{thread}"),
        format!("/{board}/"),
        format!("/{board}/catalog"),
    ] {
        let page = html(
            app.clone()
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap(),
            StatusCode::OK,
        )
        .await;
        assert!(page.contains(&format!("http://localhost:3002/media/{}.png", asset.id)));
        assert!(
            page.contains(&format!("&#60;b&#62;{board}&#60;/b&#62;.png")),
            "Filename must be escaped"
        );
        assert!(!page.contains(&capability));
    }
    assert_eq!(
        app.clone()
            .oneshot(post_request(&format!("/{board}/post"), posting))
            .await
            .unwrap()
            .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        app.clone()
            .oneshot(post_request(
                &format!("/{board}/delete"),
                format!("no={thread}&password=wrong-password&file_only=true")
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    reader.get(&asset.id).await.unwrap();
    assert_eq!(
        app.clone()
            .oneshot(post_request(
                &format!("/{board}/delete"),
                format!("no={thread}&password=synthetic-password-123&file_only=true")
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::SEE_OTHER
    );
    assert!(matches!(
        reader.get(&asset.id).await,
        Err(board_store::StoreError::NotFound)
    ));
    let page = html(
        app.clone()
            .oneshot(
                Request::get(format!("/{board}/thread/{thread}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    assert!(page.contains("File deleted."));
    assert!(!page.contains(&format!("/media/{}.png", asset.id)));

    let response = app
        .clone()
        .oneshot(multipart(board, vec![1; 32], false))
        .await
        .unwrap();
    let page = html(response, StatusCode::OK).await;
    let cancel_id = hidden(&page, "upload_id");
    let cancel_cap = hidden(&page, "upload_capability");
    assert_eq!(
        app.clone()
            .oneshot(post_request(
                &format!("/{board}/upload/cancel"),
                receipt(&cancel_id, &"0".repeat(64))
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        app.clone()
            .oneshot(post_request(
                &format!("/{board}/upload/cancel"),
                receipt(&cancel_id, &cancel_cap)
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::SEE_OTHER
    );
    assert!(matches!(
        intake.status(&cancel_id, &cancel_cap).await,
        Err(board_store::StoreError::NotFound)
    ));
    assert_eq!(
        app.clone()
            .oneshot(multipart(board, vec![1; 32], true))
            .await
            .unwrap()
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        app.clone()
            .oneshot(multipart(board, vec![1; 8_388_609], false))
            .await
            .unwrap()
            .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    assert_eq!(
        app.clone()
            .oneshot(multipart(board, vec![], false))
            .await
            .unwrap()
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM media.jobs")
        .fetch_one(admin)
        .await
        .unwrap();
    let mut cross_origin = multipart(board, vec![1; 32], false);
    cross_origin
        .headers_mut()
        .insert("origin", "https://outside.example".parse().unwrap());
    assert_eq!(
        app.oneshot(cross_origin).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let after: i64 = sqlx::query_scalar("SELECT count(*) FROM media.jobs")
        .fetch_one(admin)
        .await
        .unwrap();
    assert_eq!(before, after);
}
