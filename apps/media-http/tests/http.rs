#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use board_media::{ApprovedFiles, ObjectId, PublicationStore, Quarantine, ValidatedOutput};
use board_media_http::AppState;
use board_store::{
    media::MediaQueue,
    media_assets::{MediaReader, OutputMetadata},
};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;

fn request(method: &str, path: &str) -> axum::http::request::Builder {
    Request::builder()
        .method(method)
        .uri(path)
        .header("host", "127.0.0.1:3002")
}

async fn send(app: &Router, method: &str, path: &str) -> axum::response::Response {
    app.clone()
        .oneshot(request(method, path).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

fn safe(response: &axum::response::Response) {
    let h = response.headers();
    assert_eq!(h["x-content-type-options"], "nosniff");
    assert_eq!(h["referrer-policy"], "no-referrer");
    assert_eq!(h["cross-origin-resource-policy"], "cross-origin");
    assert!(
        h["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("sandbox")
    );
    assert!(!h.contains_key("set-cookie"));
    assert!(!h.contains_key("access-control-allow-credentials"));
}

#[tokio::test]
async fn http_serves_only_currently_approved_checked_bytes() {
    let admin = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let ids = Arc::new(Mutex::new(Vec::new()));
    let test_ids = ids.clone();
    let test_admin = admin.clone();
    let outcome = tokio::spawn(async move { exercise(test_admin, test_ids).await }).await;
    let ids: Vec<String> = ids.lock().unwrap().clone();
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
    outcome.unwrap();
}

async fn exercise(admin: sqlx::PgPool, ids: Arc<Mutex<Vec<String>>>) {
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let reader = MediaReader::connect(&std::env::var("MEDIA_READ_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let temp = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(temp.path().join("quarantine")).unwrap();
    let root = temp.path().join("objects");
    let store = PublicationStore::new(&root, &quarantine).unwrap();
    let origin = board_config::Origin::parse("http://127.0.0.1:3002").unwrap();
    let state = AppState::new(reader.clone(), ApprovedFiles::open(&root).unwrap(), &origin);
    let (metrics, app) = board_media_http::observed_router(state);
    let healthy = send(&app, "GET", "/healthz").await;
    assert_eq!(healthy.status(), StatusCode::OK);
    drop(healthy);
    assert_eq!(
        send(&app, "GET", "/metrics").await.status(),
        StatusCode::NOT_FOUND
    );
    let snapshot = metrics.render();
    assert!(
        snapshot
            .contains("board_http_responses_total{listener=\"media\",status_class=\"2xx\"} 1\n")
    );
    assert!(
        snapshot
            .contains("board_http_responses_total{listener=\"media\",status_class=\"4xx\"} 1\n")
    );
    assert!(snapshot.contains("board_db_pool_max_connections{pool=\"media_read\"} 8\n"));
    assert!(!snapshot.contains("postgres"));
    let job = queue.reserve("media HTTP synthetic fixture").await.unwrap();
    ids.lock().unwrap().push(job.id.clone());
    queue.queue(&job.id, 4).await.unwrap();
    let claimed = queue.claim().await.unwrap().unwrap();
    assert_eq!(claimed.id, job.id, "Use an idle disposable queue");
    let token = claimed.lease_token.unwrap();
    let mut pixels = b"IBRGBA01\0\0\0\x01\0\0\0\x01".to_vec();
    pixels.extend_from_slice(&[100, 20, 30, 255]);
    let output = ValidatedOutput::read(pixels.as_slice())
        .await
        .unwrap()
        .encode()
        .unwrap();
    let guard = store.try_lock().unwrap();
    let asset = queue
        .prepare_output(
            &job.id,
            &token,
            &OutputMetadata {
                sha256: output.sha256().into(),
                bytes: output.len() as i64,
                width: 1,
                height: 1,
            },
        )
        .await
        .unwrap();
    guard.install(asset.id.parse().unwrap(), &output).unwrap();
    let path = format!("/media/{}.png", asset.id);
    let pending = send(&app, "GET", &path).await;
    assert_eq!(pending.status(), StatusCode::NOT_FOUND);
    safe(&pending);
    assert_eq!(pending.headers()["cache-control"], "no-store");
    drop(pending);
    queue
        .approve_output(&job.id, &token, &asset.id)
        .await
        .unwrap();
    drop(guard);
    let disk = root.join(format!("{}.png", asset.id));
    let png = std::fs::read(&disk).unwrap();
    let response = send(&app, "GET", &path).await;
    assert_eq!(response.status(), StatusCode::OK);
    safe(&response);
    assert_eq!(response.headers()["content-type"], "image/png");
    assert_eq!(response.headers()["content-length"], png.len().to_string());
    assert_eq!(
        response.headers()["cache-control"],
        "public, no-cache, must-revalidate"
    );
    let etag = response.headers()["etag"].to_str().unwrap().to_owned();
    assert_eq!(etag, format!("\"{}\"", asset.sha256));
    assert_eq!(
        to_bytes(response.into_body(), 5_242_880)
            .await
            .unwrap()
            .as_ref(),
        png.as_slice()
    );
    let head = send(&app, "HEAD", &path).await;
    assert_eq!(head.status(), StatusCode::OK);
    assert_eq!(head.headers()["content-length"], png.len().to_string());
    assert!(to_bytes(head.into_body(), 0).await.unwrap().is_empty());
    for condition in [
        etag.clone(),
        format!("W/{etag}"),
        "*".into(),
        format!("\"other\", {etag}"),
    ] {
        let response = app
            .clone()
            .oneshot(
                request("GET", &path)
                    .header("if-none-match", condition)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_MODIFIED);
        safe(&response);
        assert!(to_bytes(response.into_body(), 0).await.unwrap().is_empty());
    }
    let ranged = app
        .clone()
        .oneshot(
            request("GET", &path)
                .header("range", "bytes=0-2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(ranged.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(ranged.into_body(), 5_242_880)
            .await
            .unwrap()
            .as_ref(),
        png.as_slice()
    );
    for (method, uri, expected) in [
        ("POST", path.clone(), StatusCode::METHOD_NOT_ALLOWED),
        ("GET", format!("{path}?download=1"), StatusCode::BAD_REQUEST),
        ("GET", "/media/../secret.png".into(), StatusCode::NOT_FOUND),
        ("GET", "/media/%2e%2e.png".into(), StatusCode::NOT_FOUND),
        (
            "GET",
            "/media/.publication.lock".into(),
            StatusCode::NOT_FOUND,
        ),
        (
            "GET",
            format!("/media/{}.png", ObjectId::generate().unwrap()),
            StatusCode::NOT_FOUND,
        ),
        ("GET", "/".into(), StatusCode::NOT_FOUND),
    ] {
        let response = send(&app, method, &uri).await;
        assert_eq!(response.status(), expected, "{method} {uri}");
        safe(&response);
        assert_eq!(response.headers()["cache-control"], "no-store");
    }
    let wrong_host = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&path)
                .header("host", "staff.example")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(wrong_host.status(), StatusCode::MISDIRECTED_REQUEST);
    let body = app
        .clone()
        .oneshot(
            request("GET", &path)
                .body(Body::from("unneeded data"))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(body.status(), StatusCode::BAD_REQUEST);
    let mut held = Vec::new();
    for _ in 0..16 {
        let r = send(&app, "GET", &path).await;
        assert_eq!(r.status(), StatusCode::OK);
        held.push(r);
    }
    assert_eq!(
        send(&app, "GET", &path).await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    let head_overload = send(&app, "HEAD", &path).await;
    assert_eq!(head_overload.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        to_bytes(head_overload.into_body(), 0)
            .await
            .unwrap()
            .is_empty()
    );
    let mut data = Vec::new();
    for response in held.drain(..) {
        data.push(to_bytes(response.into_body(), 5_242_880).await.unwrap());
    }
    assert_eq!(
        send(&app, "GET", &path).await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    data.clear();
    assert_eq!(send(&app, "GET", &path).await.status(), StatusCode::OK);
    std::fs::write(&disk, vec![0; png.len()]).unwrap();
    let corrupt = app
        .clone()
        .oneshot(
            request("GET", &path)
                .header("if-none-match", &etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(corrupt.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(!corrupt.headers().contains_key("etag"));
    std::fs::remove_file(&disk).unwrap();
    assert_eq!(
        send(&app, "HEAD", &path).await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    std::fs::write(&disk, &png).unwrap();
    sqlx::query("DELETE FROM media.assets WHERE id=$1")
        .bind(&asset.id)
        .execute(&admin)
        .await
        .unwrap();
    let removed = app
        .clone()
        .oneshot(
            request("GET", &path)
                .header("if-none-match", &etag)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(removed.status(), StatusCode::NOT_FOUND);
    assert!(!removed.headers().contains_key("etag"));
    assert_eq!(send(&app, "GET", "/healthz").await.status(), StatusCode::OK);
    assert_eq!(send(&app, "GET", "/readyz").await.status(), StatusCode::OK);
    reader.close().await;
    assert_eq!(
        send(&app, "GET", "/readyz").await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        send(&app, "GET", &path).await.status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(send(&app, "GET", "/healthz").await.status(), StatusCode::OK);
}
