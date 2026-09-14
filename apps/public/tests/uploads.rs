#![cfg(feature = "database-tests")]
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    response::Response,
};
use board_store::{
    media::MediaQueue,
    media_assets::{MediaReader, OutputMetadata, OutputVariants},
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
fn multipart_post_request(path: &str, fields: &[(&str, String)]) -> Request<Body> {
    let mut body = String::new();
    for (name, value) in fields {
        body.push_str(&format!(
            "--post-boundary\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
        ));
    }
    body.push_str("--post-boundary--\r\n");
    Request::post(path)
        .header("origin", "http://127.0.0.1:3000")
        .header(
            "content-type",
            "multipart/form-data; boundary=post-boundary",
        )
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
    // Expanded admission cases share one synthetic peer; production limits
    // remain independently exercised by http_limits.rs.
    let limits = board_config::PublicRequestLimits::from_lookup(|key| match key {
        "PUBLIC_WRITES_PER_MINUTE" => Some("60".into()),
        _ => None,
    })
    .unwrap();
    let app = board_public::routers_with_limits(
        public.clone(),
        "http://127.0.0.1:3000".into(),
        false,
        Some(settings),
        limits,
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
            .contains("img-src http://localhost:3002 http://127.0.0.1:3000/static/themes/fade.png http://127.0.0.1:3000/static/themes/fade-blue.png http://127.0.0.1:3000/static/catalog/filedeleted-res.gif")
    );
    let page = html(response, StatusCode::OK).await;
    assert!(page.contains("enctype=\"multipart/form-data\""));
    assert!(page.contains("rows=\"4\" required aria-describedby=\"postHelp\""));
    for comment in ["&com=", "", "&com=+%09%0A"] {
        assert_eq!(
            app.clone()
                .oneshot(post_request(
                    &format!("/{board}/post"),
                    format!("name=Test&password=synthetic-password-123{comment}")
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
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
        format!("{body}&name=Test&sub=Image&com=&spoiler=true&password=synthetic-password-123");
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
    assert!(approved.contains("rows=\"4\" aria-describedby=\"postHelp\""));
    for comment in ["+%09%0A", "%00"] {
        assert_eq!(
            app.clone()
                .oneshot(post_request(
                    &format!("/{board}/post"),
                    format!("{body}&com={comment}&password=synthetic-password-123")
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
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
    let (image_only, _) = json(&app, &format!("/{board}/thread/{thread}.json")).await;
    assert!(image_only["posts"][0].get("com").is_none());
    let comment: String = sqlx::query_scalar("SELECT comment FROM content.posts WHERE id=$1")
        .bind(thread)
        .fetch_one(admin)
        .await
        .unwrap();
    assert!(comment.is_empty());
    let tim: i64 = sqlx::query_scalar("SELECT tim FROM content.post_media WHERE post_id=$1")
        .bind(thread)
        .fetch_one(admin)
        .await
        .unwrap();
    for (option, source, dimensions) in [
        (
            "off",
            "/static/catalog/spoiler.png".to_owned(),
            "width=\"100\" height=\"100\"",
        ),
        (
            "on",
            format!("http://localhost:3002/{board}/{tim}.png"),
            "width=\"10\" height=\"20\"",
        ),
    ] {
        let page = html(
            app.clone()
                .oneshot(
                    Request::get(format!("/{board}/catalog?spoilers={option}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
            StatusCode::OK,
        )
        .await;
        let card = page
            .split(&format!("id=\"thread-{thread}\""))
            .nth(1)
            .unwrap()
            .split("</section>")
            .next()
            .unwrap();
        assert!(card.contains(&format!(" src=\"{source}\"")));
        assert!(card.contains(dimensions));
        assert!(card.contains(&format!("href=\"/{board}/thread/{thread}\"")));
    }
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
        assert!(page.contains(&format!("http://localhost:3002/{board}/{tim}.png")));
        assert!(
            page.contains(&format!("&#60;b&#62;{board}&#60;/b&#62;.png")),
            "Filename must be escaped"
        );
        assert!(!page.contains(&capability));
    }
    let mut replay = post_request(&format!("/{board}/post"), posting.clone());
    replay
        .headers_mut()
        .insert("accept", "application/json".parse().unwrap());
    let replay = app.clone().oneshot(replay).await.unwrap();
    assert_eq!(replay.status(), StatusCode::OK);
    assert!(!replay.headers().contains_key("set-cookie"));
    let value: serde_json::Value =
        serde_json::from_slice(&to_bytes(replay.into_body(), 1024).await.unwrap()).unwrap();
    assert!(value["error"].is_string());
    assert_eq!(value.as_object().unwrap().len(), 1);
    assert_eq!(
        board_store::visible_post_count(admin, board, thread)
            .await
            .unwrap(),
        1
    );
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
    let (deleted_image_only, _) = json(&app, &format!("/{board}/thread/{thread}.json")).await;
    assert!(deleted_image_only["posts"][0].get("com").is_none());
    assert_eq!(deleted_image_only["posts"][0]["filedeleted"], 1);
    let revealed_deleted = html(
        app.clone()
            .oneshot(
                Request::get(format!("/{board}/catalog?spoilers=on"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    let deleted_card = revealed_deleted
        .split(&format!("id=\"thread-{thread}\""))
        .nth(1)
        .unwrap()
        .split("</section>")
        .next()
        .unwrap();
    assert!(deleted_card.contains("src=\"/static/catalog/filedeleted-res.gif\""));
    assert!(!deleted_card.contains("data-spoiler-src"));
    assert!(!page.contains(&format!("/{board}/{tim}.png")));

    image_reply_contract(&app, board, &queue, &intake, admin).await;
    image_admission_http(&app, board, &queue, &intake, admin).await;

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

async fn json(app: &Router, path: &str) -> (serde_json::Value, String) {
    let response = app
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "{path}");
    let etag = response.headers()["etag"].to_str().unwrap().to_owned();
    let bytes = to_bytes(response.into_body(), 262_144).await.unwrap();
    (serde_json::from_slice(&bytes).unwrap(), etag)
}

async fn catalog_counts(
    app: &Router,
    board: &str,
    thread: i64,
    replies: i64,
    images: i64,
    limited: bool,
) {
    let page = html(
        app.clone()
            .oneshot(
                Request::get(format!("/{board}/catalog"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    let image_count = if limited {
        format!("<i>I: <b>{images}</b></i>")
    } else {
        format!("I: <b>{images}</b>")
    };
    assert!(page.contains(&format!("id=\"meta-{thread}\" title=\"(R)eplies / (I)mage Replies\">R: <b>{replies}</b> / {image_count}</div>")));
    let (value, _) = json(app, &format!("/{board}/thread/{thread}.json")).await;
    assert_eq!(
        value["posts"][0]["imagelimit"].as_i64(),
        limited.then_some(1)
    );
}

async fn image_flags(
    app: &Router,
    board: &str,
    thread: i64,
    images: i64,
    replies: i64,
    json_limited: bool,
    html_limited: bool,
) -> String {
    let mut full_etag = String::new();
    for (suffix, kind) in [
        (format!("thread/{thread}.json"), 0),
        (format!("thread/{thread}-tail.json"), 1),
        ("1.json".into(), 2),
        ("catalog.json".into(), 3),
    ] {
        let (value, etag) = json(app, &format!("/{board}/{suffix}")).await;
        if kind == 0 {
            full_etag = etag;
        }
        let op = match kind {
            0 | 1 => &value["posts"][0],
            2 => &value["threads"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["posts"][0]["no"] == thread)
                .unwrap()["posts"][0],
            _ => value[0]["threads"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["no"] == thread)
                .unwrap(),
        };
        assert_eq!(op["no"], thread, "{suffix}");
        assert_eq!(op["images"], images, "{suffix}");
        assert_eq!(op["replies"], replies, "{suffix}");
        assert_eq!(
            op["imagelimit"].as_i64(),
            if kind == 1 {
                Some(i64::from(json_limited))
            } else {
                json_limited.then_some(1)
            },
            "{suffix}"
        );
        for internal in ["undead", "permaage", "permasage"] {
            assert!(op.get(internal).is_none(), "{internal} is not a JSON field");
        }
    }
    let page = html(
        app.clone()
            .oneshot(
                Request::get(format!("/{board}/catalog"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    let meta = page
        .split(&format!("id=\"meta-{thread}\""))
        .nth(1)
        .unwrap()
        .split("</div>")
        .next()
        .unwrap();
    if images > 0 {
        assert!(meta.contains(&format!("I: <b>{images}</b>")));
    } else {
        assert!(!meta.contains("I:"), "zero images omit the image segment");
    }
    assert_eq!(meta.contains("<i>I: <b>"), html_limited && images > 0);
    full_etag
}

async fn image_flag_transitions(app: &Router, board: &str, thread: i64, admin: &PgPool) {
    sqlx::query("UPDATE content.boards SET json_tail_size=1 WHERE slug=$1")
        .bind(board)
        .execute(admin)
        .await
        .unwrap();
    let mut previous: Option<(String, bool)> = None;
    for sticky in [false, true] {
        for permaage in [false, true] {
            for undead in [false, true] {
                let mut same_flags = String::new();
                for permasage in [false, true] {
                    sqlx::query("UPDATE content.threads SET sticky=$2,permaage=$3,undead=$4,permasage=$5,modified_at=clock_timestamp() WHERE id=$1")
                        .bind(thread).bind(sticky).bind(permaage).bind(undead).bind(permasage).execute(admin).await.unwrap();
                    let json_limited = !sticky && !permaage && !undead;
                    let etag =
                        image_flags(app, board, thread, 7, 7, json_limited, !sticky && !permaage)
                            .await;
                    if let Some((old, old_limited)) = &previous
                        && *old_limited != json_limited
                    {
                        assert_ne!(&etag, old);
                        let response = app
                            .clone()
                            .oneshot(
                                Request::get(format!("/{board}/thread/{thread}.json"))
                                    .header("if-none-match", old)
                                    .body(Body::empty())
                                    .unwrap(),
                            )
                            .await
                            .unwrap();
                        assert_eq!(response.status(), StatusCode::OK);
                    }
                    if permasage {
                        assert_eq!(
                            etag, same_flags,
                            "permasage does not change image indicators"
                        );
                    } else {
                        same_flags = etag.clone();
                    }
                    previous = Some((etag, json_limited));
                }
            }
        }
    }
    sqlx::query("UPDATE content.threads SET sticky=false,permaage=false,undead=false,permasage=false WHERE id=$1").bind(thread).execute(admin).await.unwrap();
    sqlx::query("UPDATE content.boards SET json_tail_size=0 WHERE slug=$1")
        .bind(board)
        .execute(admin)
        .await
        .unwrap();
}

async fn image_admission_http(
    app: &Router,
    board: &str,
    queue: &MediaQueue,
    intake: &IntakeStore,
    admin: &PgPool,
) {
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    sqlx::query("UPDATE content.boards SET image_limit=1 WHERE slug=$1")
        .bind(board)
        .execute(admin)
        .await
        .unwrap();
    let draft = board_store::NewPost {
        name: "Anonymous".into(),
        subject: String::new(),
        comment: "Owned image admission".into(),
        deletion_hash: "unused-owned-hash".into(),
        sage: false,
    };
    let thread = board_store::create_post(&public, board, 0, &draft)
        .await
        .unwrap();
    for (index, (sticky, undead, permaage, accepted)) in [
        (false, false, false, true), // Fill the sole ordinary slot.
        (false, false, true, false),
        (true, false, false, true),
        (false, true, false, true),
        (false, false, false, false),
    ]
    .into_iter()
    .enumerate()
    {
        // Metadata-only approval fixture; the independent publisher/browser
        // qualification tests real normalized bytes and file visibility.
        let upload = intake
            .reserve(&format!("<b>{board}</b>.png"))
            .await
            .unwrap();
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
        let asset = queue
            .prepare_output(
                &job.id,
                job.lease_token.as_deref().unwrap(),
                &OutputMetadata {
                    sha256: "c".repeat(64),
                    bytes: 123,
                    width: 10,
                    height: 20,
                },
            )
            .await
            .unwrap();
        queue
            .approve_output(&job.id, job.lease_token.as_deref().unwrap(), &asset.id)
            .await
            .unwrap();
        sqlx::query("UPDATE content.threads SET sticky=$2,undead=$3,permaage=$4 WHERE id=$1")
            .bind(thread)
            .bind(sticky)
            .bind(undead)
            .bind(permaage)
            .execute(admin)
            .await
            .unwrap();
        let route = if index % 2 == 0 {
            "post"
        } else {
            "imgboard.php"
        };
        let mut request = if index % 2 == 0 {
            post_request(
                &format!("/{board}/{route}"),
                format!(
                    "resto={thread}&upload_id={}&upload_capability={}&pwd=owned-image-password&sticky=1&undead=1",
                    upload.id, upload.capability
                ),
            )
        } else {
            multipart_post_request(
                &format!("/{board}/{route}"),
                &[
                    ("resto", thread.to_string()),
                    ("upload_id", upload.id.clone()),
                    ("upload_capability", upload.capability.clone()),
                    ("pwd", "owned-image-password".into()),
                    ("sticky", "1".into()),
                    ("undead", "1".into()),
                ],
            )
        };
        request
            .headers_mut()
            .insert("accept", "application/json".parse().unwrap());
        let before = board_store::thread(&public, board, thread).await.unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        // Negotiated source rule failures remain JSON errors with HTTP 200.
        assert_eq!(response.status(), StatusCode::OK);
        let value: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        let after = board_store::thread(&public, board, thread).await.unwrap();
        if accepted {
            let id = value["pid"].as_i64().unwrap();
            assert_eq!(value["tid"], thread);
            assert_eq!(after.reply_count, before.reply_count + 1);
            let attachment = board_store::post_media::attachment(&public, id)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(attachment.asset_id, asset.id);
        } else {
            assert!(
                value["error"].as_str().unwrap().contains("another image"),
                "{value}"
            );
            assert_eq!(
                (after.reply_count, after.modified_at, after.http_modified_at),
                (
                    before.reply_count,
                    before.modified_at,
                    before.http_modified_at
                )
            );
        }
        assert_eq!(
            (after.sticky, after.undead),
            (sticky, undead),
            "client flags cannot grant the exception"
        );
    }
    let (value, _) = json(app, &format!("/{board}/thread/{thread}.json")).await;
    assert_eq!(value["posts"][0]["images"], 3);
    assert_eq!(value["posts"][0]["imagelimit"], 1);
    public.close().await;
    sqlx::query("UPDATE content.boards SET image_limit=7 WHERE slug=$1")
        .bind(board)
        .execute(admin)
        .await
        .unwrap();
}

async fn image_reply_contract(
    app: &Router,
    board: &str,
    queue: &MediaQueue,
    intake: &IntakeStore,
    admin: &PgPool,
) {
    sqlx::query("UPDATE content.boards SET image_limit=7 WHERE slug=$1")
        .bind(board)
        .execute(admin)
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut thread = 0;
    let mut posts = Vec::new();
    let mut times = std::collections::BTreeSet::new();
    for index in 0..8 {
        // Metadata-only fixture; byte encoding is covered by the publisher/browser tests.
        let upload = intake
            .reserve(&format!("<b>{board}</b>.png"))
            .await
            .unwrap();
        intake
            .begin_upload(&upload.id, &upload.capability)
            .await
            .unwrap();
        intake
            .finish_upload(&upload.id, &upload.capability, 100)
            .await
            .unwrap();
        let job = queue.claim().await.unwrap().unwrap();
        assert_eq!(job.id, upload.id, "Requires an idle disposable queue");
        let metadata = OutputMetadata {
            sha256: "c".repeat(64),
            bytes: 123,
            width: 500,
            height: 300,
        };
        let variants = OutputVariants {
            md5: "00".repeat(16),
            thumbnail: OutputMetadata {
                sha256: "d".repeat(64),
                bytes: 80,
                width: 250,
                height: 150,
            },
        };
        let asset = queue
            .prepare_output_with_variants(
                &job.id,
                job.lease_token.as_deref().unwrap(),
                &metadata,
                Some(&variants),
            )
            .await
            .unwrap();
        queue
            .approve_output(&job.id, job.lease_token.as_deref().unwrap(), &asset.id)
            .await
            .unwrap();
        let id = if index == 0 {
            let mut request = post_request(
                &format!("/{board}/post"),
                format!(
                    "upload_id={}&upload_capability={}&sub=Image+replies&password=synthetic-password-123",
                    upload.id, upload.capability
                ),
            );
            request
                .headers_mut()
                .insert("accept", "application/json".parse().unwrap());
            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert!(!response.headers().contains_key("location"));
            let value: serde_json::Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 1024).await.unwrap())
                    .unwrap();
            assert_eq!(value["tid"], 0);
            value["pid"].as_i64().unwrap()
        } else if index == 1 {
            // Omitted com is equivalent to an empty form field, but only an
            // approved attachment can make this actual HTTP reply succeed.
            let response = app
                .clone()
                .oneshot(multipart_post_request(
                    &format!("/{board}/imgboard.php"),
                    &[
                        ("mode", "regist".into()),
                        ("pwd", "synthetic-password-123".into()),
                        ("upload_id", upload.id.clone()),
                        ("upload_capability", upload.capability.clone()),
                        ("resto", thread.to_string()),
                        ("spoiler", "on".into()),
                    ],
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::SEE_OTHER);
            response.headers()["location"]
                .to_str()
                .unwrap()
                .split("#p")
                .nth(1)
                .unwrap()
                .parse::<i64>()
                .unwrap()
        } else {
            board_store::create_post_with_attachment(
                &public,
                board,
                thread,
                &board_store::NewPost {
                    name: "Anonymous".into(),
                    subject: "Image replies".into(),
                    comment: if index % 2 == 0 {
                        String::new()
                    } else {
                        "Synthetic API fixture".into()
                    },
                    deletion_hash: "not-a-password".into(),
                    sage: false,
                },
                Some(&board_store::post_media::NewAttachment {
                    upload,
                    spoiler: index == 1,
                }),
            )
            .await
            .unwrap()
        };
        if thread == 0 {
            thread = id;
        }
        posts.push(id);
        let file = board_store::post_media::attachment(&public, id)
            .await
            .unwrap()
            .unwrap();
        assert!(
            times.insert(file.tim),
            "Numbers must be unique even during rapid attachment commits"
        );
    }
    let (boards, _) = json(app, "/boards.json").await;
    let policy = boards["boards"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["board"] == board)
        .unwrap();
    assert_eq!(policy["image_limit"], 7);
    assert_eq!(policy["max_filesize"], 8_388_608);
    assert_eq!(policy["spoilers"], 1);
    assert!(policy.get("text_only").is_none());
    let path = format!("/{board}/thread/{thread}.json");
    let (full, etag) = json(app, &path).await;
    assert_eq!(
        full["posts"][0]["images"], 7,
        "An image OP does not use a reply image slot"
    );
    assert_eq!(full["posts"][0]["imagelimit"], 1);
    for (index, p) in full["posts"].as_array().unwrap().iter().enumerate() {
        if index % 2 == 0 || index == 1 {
            assert!(p.get("com").is_none());
        } else {
            assert!(p["com"].as_str().unwrap().contains("Synthetic API fixture"));
        }
        assert_eq!(p["md5"], "AAAAAAAAAAAAAAAAAAAAAA==");
        assert_eq!(p["filename"], format!("<b>{board}</b>"));
        assert_eq!(p["tn_w"], 250);
        assert_eq!(p["tn_h"], 150);
        if index == 1 {
            assert_eq!(p["spoiler"], 1);
        } else {
            assert!(p.get("spoiler").is_none());
        }
        if index != 0 {
            assert!(p.get("images").is_none());
        }
    }
    let (index, _) = json(app, &format!("/{board}/1.json")).await;
    let preview = index["threads"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["posts"][0]["no"] == thread)
        .unwrap();
    assert_eq!(preview["posts"][0]["images"], 7);
    assert_eq!(preview["posts"][0]["omitted_images"], 2);
    let (catalog, _) = json(app, &format!("/{board}/catalog.json")).await;
    let op = catalog[0]["threads"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["no"] == thread)
        .unwrap();
    assert_eq!(op["images"], 7);
    assert!(op.get("com").is_none());
    catalog_counts(app, board, thread, 7, 7, true).await;
    image_flag_transitions(app, board, thread, admin).await;
    board_store::post_media::delete_attachment(&public, board, posts[1])
        .await
        .unwrap();
    let response = app
        .clone()
        .oneshot(
            Request::get(&path)
                .header("if-none-match", etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Image deletion invalidates the full-thread validator"
    );
    let (after, _) = json(app, &path).await;
    assert_eq!(after["posts"][0]["images"], 6);
    assert!(after["posts"][0].get("imagelimit").is_none());
    assert_eq!(after["posts"][1]["filedeleted"], 1);
    for key in ["tim", "filename", "md5", "ext", "tn_w", "tn_h", "spoiler"] {
        assert!(after["posts"][1].get(key).is_none());
    }
    let (index, _) = json(app, &format!("/{board}/1.json")).await;
    let preview = index["threads"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["posts"][0]["no"] == thread)
        .unwrap();
    assert_eq!(preview["posts"][0]["images"], 6);
    assert_eq!(preview["posts"][0]["omitted_images"], 1);
    catalog_counts(app, board, thread, 7, 6, false).await;
    sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1 AND board=$2")
        .bind(posts[2])
        .bind(board)
        .execute(admin)
        .await
        .unwrap();
    catalog_counts(app, board, thread, 6, 5, false).await;
    for (limit, limited) in [(5, true), (4, true), (0, true), (7, false)] {
        sqlx::query("UPDATE content.boards SET image_limit=$1 WHERE slug=$2")
            .bind(limit)
            .bind(board)
            .execute(admin)
            .await
            .unwrap();
        catalog_counts(app, board, thread, 6, 5, limited).await;
    }
    let (catalog, _) = json(app, &format!("/{board}/catalog.json")).await;
    let op = catalog[0]["threads"]
        .as_array()
        .unwrap()
        .iter()
        .find(|post| post["no"] == thread)
        .unwrap();
    assert_eq!(op["replies"], 6);
    assert_eq!(op["images"], 5);
    let filter = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("q", &format!("<b>{board}</b>.png"))
        .finish();
    let filtered = html(
        app.clone()
            .oneshot(
                Request::get(format!("/{board}/catalog?{filter}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    let (visible, _) = filtered
        .split_once("<template id=\"catalogFiltered\">")
        .unwrap();
    assert!(
        visible.contains(&format!("id=\"thread-{thread}\"")),
        "catalog filter searches the OP filename as literal text"
    );
    board_store::post_media::delete_attachment(&public, board, thread)
        .await
        .unwrap();
    let filtered = html(
        app.clone()
            .oneshot(
                Request::get(format!("/{board}/catalog?{filter}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap(),
        StatusCode::OK,
    )
    .await;
    let (visible, inert) = filtered
        .split_once("<template id=\"catalogFiltered\">")
        .unwrap();
    assert!(
        !visible.contains(&format!("id=\"thread-{thread}\"")),
        "deleted OP filename cannot match a filter"
    );
    let card = inert
        .split(&format!("id=\"thread-{thread}\""))
        .nth(1)
        .unwrap()
        .split("</section>")
        .next()
        .unwrap();
    assert!(
        card.contains("data-search-file=\"\" data-has-file=\"false\""),
        "inert cards cannot restore deleted filenames as live search metadata"
    );
    // The zero-capacity boundary applies even to a thread with no attachments.
    sqlx::query("UPDATE content.boards SET image_limit=0,json_tail_size=1 WHERE slug=$1")
        .bind(board)
        .execute(admin)
        .await
        .unwrap();
    let text = board_store::NewPost {
        name: "Anonymous".into(),
        subject: String::new(),
        comment: "Owned zero-image fixture".into(),
        deletion_hash: "synthetic-unused-hash".into(),
        sage: false,
    };
    let text_thread = board_store::create_post(&public, board, 0, &text)
        .await
        .unwrap();
    for _ in 0..2 {
        board_store::create_post(&public, board, text_thread, &text)
            .await
            .unwrap();
    }
    image_flags(app, board, text_thread, 0, 2, true, true).await;
    board_store::delete_post(&public, board, text_thread)
        .await
        .unwrap();
    sqlx::query("UPDATE content.boards SET image_limit=7,json_tail_size=0 WHERE slug=$1")
        .bind(board)
        .execute(admin)
        .await
        .unwrap();
    public.close().await;
}
