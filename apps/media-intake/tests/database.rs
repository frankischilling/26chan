#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    http::{Request, StatusCode},
};
use board_media::{MAX_INPUT_BYTES, Quarantine};
use board_media_intake::{AppState, router};
use board_store::media_intake::IntakeStore;
use futures_util::stream;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tower::ServiceExt;

struct Fixture {
    app: Router,
    store: IntakeStore,
    admin: PgPool,
    root: tempfile::TempDir,
    ids: Arc<Mutex<Vec<String>>>,
}

fn request(method: &str, path: &str) -> axum::http::request::Builder {
    Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {}", "a".repeat(64)))
}

async fn response_json(response: axum::response::Response, expected: StatusCode) -> Value {
    assert_eq!(response.status(), expected);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    serde_json::from_slice(&to_bytes(response.into_body(), 2048).await.unwrap()).unwrap()
}

impl Fixture {
    async fn reserve(&self) -> (String, String) {
        let response = self
            .app
            .clone()
            .oneshot(
                request("POST", "/v1/reservations")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"filename":"synthetic.png"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = response_json(response, StatusCode::CREATED).await;
        let id = body["id"].as_str().unwrap().to_owned();
        self.ids.lock().unwrap().push(id.clone());
        (id, body["capability"].as_str().unwrap().to_owned())
    }

    async fn upload(&self, id: &str, capability: &str, body: Body) -> axum::response::Response {
        self.app
            .clone()
            .oneshot(
                request("PUT", &format!("/v1/uploads/{id}"))
                    .header("upload-capability", capability)
                    .header("content-type", "application/octet-stream")
                    .body(body)
                    .unwrap(),
            )
            .await
            .unwrap()
    }

    async fn status(&self, id: &str, capability: &str, expected: StatusCode) -> Value {
        let response = self
            .app
            .clone()
            .oneshot(
                request("GET", &format!("/v1/uploads/{id}"))
                    .header("upload-capability", capability)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        response_json(response, expected).await
    }

    fn no_files(&self, id: &str) {
        assert!(!self.root.path().join(format!("{id}.part")).exists());
        assert!(!self.root.path().join(format!("{id}.input")).exists());
    }

    async fn exercise(&self) {
        // Admission cannot consume database queue capacity without authentication.
        let before: i64 = sqlx::query_scalar("SELECT count(*) FROM media.jobs")
            .fetch_one(&self.admin)
            .await
            .unwrap();
        let response = self
            .app
            .clone()
            .oneshot(
                Request::post("/v1/reservations")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"filename":"denied.png"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        response_json(response, StatusCode::UNAUTHORIZED).await;
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM media.jobs")
                .fetch_one(&self.admin)
                .await
                .unwrap(),
            before
        );

        for (body, expected) in [
            ("x".repeat(1025), StatusCode::PAYLOAD_TOO_LARGE),
            (
                r#"{"filename":"x","extra":true}"#.into(),
                StatusCode::BAD_REQUEST,
            ),
            (
                r#"{"filename":""}"#.into(),
                StatusCode::UNPROCESSABLE_ENTITY,
            ),
        ] {
            let response = self
                .app
                .clone()
                .oneshot(
                    request("POST", "/v1/reservations")
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            response_json(response, expected).await;
        }
        let response = self
            .app
            .clone()
            .oneshot(request("GET", "/readyz").body(Body::empty()).unwrap())
            .await
            .unwrap();
        response_json(response, StatusCode::OK).await;

        let (id, cap) = self.reserve().await;
        self.status(&id, &"0".repeat(64), StatusCode::NOT_FOUND)
            .await;
        assert_eq!(
            self.upload(&id, &"0".repeat(64), Body::from("denied"))
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
        self.no_files(&id);
        let body = response_json(
            self.upload(&id, &cap, Body::from("synthetic bytes")).await,
            StatusCode::ACCEPTED,
        )
        .await;
        assert_eq!(body["input_bytes"], 15);
        assert_eq!(
            std::fs::read(self.root.path().join(format!("{id}.input"))).unwrap(),
            b"synthetic bytes"
        );
        let status = self.status(&id, &cap, StatusCode::OK).await;
        assert_eq!(status["state"], "queued");
        assert!(
            status.get("output_id").is_none()
                && status.get("filename").is_none()
                && status.get("capability").is_none()
        );
        assert_eq!(
            self.upload(&id, &cap, Body::from("replacement"))
                .await
                .status(),
            StatusCode::CONFLICT
        );

        // No Content-Length; overflow is detected by actual stream consumption.
        let (id, cap) = self.reserve().await;
        let chunks = stream::iter(
            (0..=MAX_INPUT_BYTES / 8192)
                .map(|_| Ok::<_, std::io::Error>(Bytes::from(vec![1; 8192]))),
        );
        response_json(
            self.upload(&id, &cap, Body::from_stream(chunks)).await,
            StatusCode::PAYLOAD_TOO_LARGE,
        )
        .await;
        self.no_files(&id);
        assert_eq!(
            self.status(&id, &cap, StatusCode::OK).await["state"],
            "failed"
        );

        let (id, cap) = self.reserve().await;
        response_json(
            self.upload(&id, &cap, Body::empty()).await,
            StatusCode::BAD_REQUEST,
        )
        .await;
        self.no_files(&id);
        let (id, cap) = self.reserve().await;
        let chunks = stream::iter([
            Ok(Bytes::from_static(b"partial")),
            Err(std::io::Error::other("synthetic disconnect")),
        ]);
        response_json(
            self.upload(&id, &cap, Body::from_stream(chunks)).await,
            StatusCode::BAD_REQUEST,
        )
        .await;
        self.no_files(&id);

        // The second writer must lose before opening any path; cancellation
        // releases admission and removes the first writer's partial bytes.
        let (id, cap) = self.reserve().await;
        let task_app = self.app.clone();
        let upload_request = request("PUT", &format!("/v1/uploads/{id}"))
            .header("upload-capability", &cap)
            .header("content-type", "application/octet-stream")
            .body(Body::from_stream(stream::pending::<
                Result<Bytes, std::io::Error>,
            >()))
            .unwrap();
        let task = tokio::spawn(async move { task_app.oneshot(upload_request).await.unwrap() });
        tokio::time::timeout(Duration::from_secs(3), async {
            while !self.root.path().join(format!("{id}.part")).exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            self.upload(&id, &cap, Body::from("loser")).await.status(),
            StatusCode::CONFLICT
        );
        task.abort();
        assert!(task.await.unwrap_err().is_cancelled());
        self.no_files(&id);
        assert_eq!(
            self.upload(&id, &cap, Body::from("retry")).await.status(),
            StatusCode::CONFLICT
        );

        // Exercise the actual deadline without shortening production constants.
        let (id, cap) = self.reserve().await;
        let pending = Body::from_stream(stream::pending::<Result<Bytes, std::io::Error>>());
        let response =
            tokio::time::timeout(Duration::from_secs(19), self.upload(&id, &cap, pending))
                .await
                .unwrap();
        response_json(response, StatusCode::REQUEST_TIMEOUT).await;
        self.no_files(&id);
        assert_eq!(
            self.status(&id, &cap, StatusCode::OK).await["state"],
            "failed"
        );

        let (id, cap) = self.reserve().await;
        sqlx::query(
            "UPDATE media.jobs SET expires_at=clock_timestamp()-interval '1 second' WHERE id=$1",
        )
        .bind(&id)
        .execute(&self.admin)
        .await
        .unwrap();
        response_json(
            self.upload(&id, &cap, Body::from("expired")).await,
            StatusCode::NOT_FOUND,
        )
        .await;
        self.no_files(&id);

        // An opened quarantine whose directory disappears must fail closed.
        let absent = tempfile::tempdir().unwrap();
        let quarantine = Quarantine::new(absent.path()).unwrap();
        absent.close().unwrap();
        let app = router(AppState::new(self.store.clone(), quarantine, "a".repeat(64)).unwrap());
        let (id, cap) = self.reserve().await;
        let response = app
            .clone()
            .oneshot(
                request("PUT", &format!("/v1/uploads/{id}"))
                    .header("upload-capability", &cap)
                    .header("content-type", "application/octet-stream")
                    .body(Body::from("missing storage"))
                    .unwrap(),
            )
            .await
            .unwrap();
        response_json(response, StatusCode::SERVICE_UNAVAILABLE).await;
        assert_eq!(
            self.status(&id, &cap, StatusCode::OK).await["state"],
            "failed"
        );
        response_json(
            app.oneshot(request("GET", "/readyz").body(Body::empty()).unwrap())
                .await
                .unwrap(),
            StatusCode::SERVICE_UNAVAILABLE,
        )
        .await;

        // A database outage after bytes are complete must preserve the input.
        let (id, cap) = self.reserve().await;
        let store = self.store.clone();
        let chunk = stream::once(async move {
            store.close().await.unwrap();
            Ok::<_, std::io::Error>(Bytes::from_static(b"uncertain"))
        });
        response_json(
            self.upload(&id, &cap, Body::from_stream(chunk)).await,
            StatusCode::SERVICE_UNAVAILABLE,
        )
        .await;
        assert_eq!(
            std::fs::read(self.root.path().join(format!("{id}.input"))).unwrap(),
            b"uncertain"
        );
        assert!(!self.root.path().join(format!("{id}.part")).exists());
        let response = self
            .app
            .clone()
            .oneshot(request("GET", "/readyz").body(Body::empty()).unwrap())
            .await
            .unwrap();
        response_json(response, StatusCode::SERVICE_UNAVAILABLE).await;
        self.status(&id, &cap, StatusCode::SERVICE_UNAVAILABLE)
            .await;
    }
}

#[tokio::test]
async fn actual_intake_http_streams_and_fails_closed() {
    let store = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let admin = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let root = tempfile::tempdir().unwrap();
    let app = router(
        AppState::new(
            store.clone(),
            Quarantine::new(root.path()).unwrap(),
            "a".repeat(64),
        )
        .unwrap(),
    );
    let ids = Arc::new(Mutex::new(Vec::new()));
    let fixture = Fixture {
        app,
        store: store.clone(),
        admin: admin.clone(),
        root,
        ids: ids.clone(),
    };
    let result = tokio::spawn(async move { fixture.exercise().await }).await;
    let ids = ids.lock().unwrap().clone();
    sqlx::query("DELETE FROM media.jobs WHERE id = ANY($1)")
        .bind(ids)
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    store.close().await.unwrap();
    result.unwrap();
}
