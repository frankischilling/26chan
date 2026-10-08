#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    http::{Request, StatusCode},
};
use board_media::{
    ObjectId, Quarantine,
    paired::{INPUT_COMPLETION, InputHeader},
};
use board_media_intake::{AppState, paired, paired_qualification_router, router};
use board_store::media_intake::IntakeStore;
use futures_util::stream;
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{io, time::Duration};
use tower::ServiceExt;

type Chunk = Result<Bytes, io::Error>;

struct Fixture {
    state: AppState,
    app: Router,
    store: IntakeStore,
    admin: PgPool,
    root: tempfile::TempDir,
    ids: Vec<String>,
}
fn request(method: &str, path: &str) -> axum::http::request::Builder {
    Request::builder()
        .method(method)
        .uri(path)
        .header("authorization", format!("Bearer {}", "a".repeat(64)))
}
fn upload_request(id: &str, cap: &str, body: Body) -> Request<Body> {
    request("PUT", &format!("/v2/uploads/{id}"))
        .header("upload-capability", cap)
        .header("content-type", "application/octet-stream")
        .body(body)
        .unwrap()
}
fn envelope(id: &str, image: &[u8], replay: Option<&[u8]>) -> Vec<u8> {
    let id: ObjectId = id.parse().unwrap();
    let h = InputHeader::new(
        id.bytes(),
        image.len() as u64,
        replay.map(|r| r.len() as u64),
    )
    .unwrap();
    let mut bytes = h.bytes().to_vec();
    bytes.extend_from_slice(image);
    if let Some(replay) = replay {
        bytes.extend_from_slice(replay);
    }
    bytes.extend_from_slice(INPUT_COMPLETION);
    bytes
}
async fn response_json(response: axum::response::Response, expected: StatusCode) -> Value {
    assert_eq!(response.status(), expected);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    serde_json::from_slice(&to_bytes(response.into_body(), 2048).await.unwrap()).unwrap()
}
impl Fixture {
    async fn new() -> Self {
        let store = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let admin = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let root = tempfile::tempdir().unwrap();
        let state = AppState::new(
            store.clone(),
            Quarantine::new(root.path()).unwrap(),
            "a".repeat(64),
        )
        .unwrap();
        Self {
            app: paired_qualification_router(state.clone()),
            state,
            store,
            admin,
            root,
            ids: Vec::new(),
        }
    }
    async fn reserve(&mut self) -> (String, String) {
        let response = self
            .app
            .clone()
            .oneshot(
                request("POST", "/v2/reservations")
                    .header("content-type", "application/json")
                    .body(Body::from(json!({"filename":"drawing.png"}).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let result = response_json(response, StatusCode::CREATED).await;
        let id = result["id"].as_str().unwrap().to_owned();
        self.ids.push(id.clone());
        (id, result["capability"].as_str().unwrap().to_owned())
    }
    async fn upload(&self, id: &str, cap: &str, body: Body) -> axum::response::Response {
        self.app
            .clone()
            .oneshot(upload_request(id, cap, body))
            .await
            .unwrap()
    }
    async fn no_receipt(&self, id: &str) {
        let clean:bool=sqlx::query_scalar("SELECT state <> 'queued' AND input_bytes IS NULL AND input_sha256 IS NULL AND input_image_bytes IS NULL AND input_image_sha256 IS NULL AND input_replay_bytes IS NULL AND input_replay_sha256 IS NULL FROM media.jobs WHERE id=$1")
            .bind(id).fetch_one(&self.admin).await.unwrap();
        assert!(clean);
        assert!(!self.root.path().join(format!("{id}.input")).exists());
    }
    fn no_files(&self, id: &str) {
        for suffix in ["part", "input"] {
            assert!(!self.root.path().join(format!("{id}.{suffix}")).exists());
        }
    }
    async fn receipt(&self, id: &str, wire: &[u8], image: &[u8], replay: Option<&[u8]>) {
        let exact:bool=sqlx::query_scalar("SELECT state='queued' AND input_kind='paired-v2' AND input_bytes=octet_length($2::bytea) AND input_sha256=encode(sha256($2),'hex') AND input_image_bytes=octet_length($3::bytea) AND input_image_sha256=encode(sha256($3),'hex') AND input_replay_bytes IS NOT DISTINCT FROM octet_length($4::bytea)::bigint AND input_replay_sha256 IS NOT DISTINCT FROM encode(sha256($4),'hex') AND attempts=0 AND lease_token IS NULL AND output_sha256 IS NULL AND output_bytes IS NULL FROM media.jobs WHERE id=$1")
            .bind(id).bind(wire).bind(image).bind(replay).fetch_one(&self.admin).await.unwrap();
        assert!(
            exact,
            "Persisted hashes and counts must describe actual bytes independently of declarations"
        );
        assert_eq!(
            std::fs::read(self.root.path().join(format!("{id}.input"))).unwrap(),
            wire
        );
        assert!(!self.root.path().join(format!("{id}.part")).exists());
    }
    async fn cleanup(self) {
        sqlx::query("DELETE FROM media.jobs WHERE id=ANY($1)")
            .bind(&self.ids)
            .execute(&self.admin)
            .await
            .unwrap();
        self.store.close().await.unwrap();
        self.admin.close().await;
    }
}

#[tokio::test]
async fn paired_http_actual_descriptors_and_inactive_production_routes() {
    let mut f = Fixture::new().await;
    // Production registration stays unchanged even with a live paired store.
    for (method, path) in [
        ("POST", "/v2/reservations"),
        ("PUT", "/v2/uploads/00000000000000000000000000000000"),
    ] {
        let response = router(f.state.clone())
            .oneshot(request(method, path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        response_json(response, StatusCode::NOT_FOUND).await;
    }
    let before: i64 = sqlx::query_scalar("SELECT count(*) FROM media.jobs")
        .fetch_one(&f.admin)
        .await
        .unwrap();
    let response = f
        .app
        .clone()
        .oneshot(
            Request::post("/v2/reservations")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"filename":"denied.png"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    response_json(response, StatusCode::UNAUTHORIZED).await;
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM media.jobs")
            .fetch_one(&f.admin)
            .await
            .unwrap(),
        before
    );
    // Intake does not decode these deliberately synthetic component bytes.
    let image = b"\x89PNG\r\n\x1a\nactual-image-component";
    for replay in [None, Some(&b"actual-raw-replay-component"[..])] {
        let (id, cap) = f.reserve().await;
        let wire = envelope(&id, image, replay);
        let chunks = stream::iter(
            wire.iter()
                .map(|&b| Ok::<_, io::Error>(Bytes::from(vec![b])))
                .collect::<Vec<_>>(),
        );
        response_json(
            f.upload(&id, &cap, Body::from_stream(chunks)).await,
            StatusCode::ACCEPTED,
        )
        .await;
        f.receipt(&id, &wire, image, replay).await;
        response_json(
            f.upload(&id, &cap, Body::from("replacement")).await,
            StatusCode::CONFLICT,
        )
        .await;
        f.receipt(&id, &wire, image, replay).await;
        let response = router(f.state.clone())
            .oneshot(
                request("PUT", &format!("/v1/uploads/{id}"))
                    .header("upload-capability", &cap)
                    .header("content-type", "application/octet-stream")
                    .body(Body::from("misrouted"))
                    .unwrap(),
            )
            .await
            .unwrap();
        response_json(response, StatusCode::CONFLICT).await;
    }
    let legacy = f.store.reserve("legacy.png").await.unwrap();
    f.ids.push(legacy.id.clone());
    response_json(
        f.upload(
            &legacy.id,
            &legacy.capability,
            Body::from(envelope(&legacy.id, image, None)),
        )
        .await,
        StatusCode::CONFLICT,
    )
    .await;
    f.no_receipt(&legacy.id).await;
    f.no_files(&legacy.id);
    let (id, cap) = f.reserve().await;
    response_json(
        f.upload(&id, &"0".repeat(64), Body::from(envelope(&id, image, None)))
            .await,
        StatusCode::NOT_FOUND,
    )
    .await;
    f.no_receipt(&id).await;
    f.no_files(&id);
    f.store.abort_upload(&id, &cap).await.unwrap();
    f.cleanup().await;
}

#[tokio::test]
async fn paired_http_rejects_incomplete_malformed_and_late_error_streams() {
    let mut f = Fixture::new().await;
    for case in 0..8 {
        let (id, cap) = f.reserve().await;
        let mut wire = envelope(&id, b"image", Some(b"replay"));
        match case {
            0 => {
                wire.truncate(wire.len() - 8);
            }
            1 => {
                wire.push(0);
            }
            2 => {
                *wire.last_mut().unwrap() = b'X';
            }
            3 => {
                wire[32] ^= 1;
            }
            4 => {
                wire.truncate(51);
            }
            5 => {
                wire[15] = 2;
            }
            6 => {
                wire[24..32].fill(0);
            }
            7 => {}
            _ => unreachable!(),
        }
        let body = if case == 7 {
            Body::from_stream(stream::iter([
                Ok(Bytes::from(wire)),
                Err(io::Error::other(
                    "late HTTP body failure after completion marker",
                )),
            ]))
        } else {
            Body::from(wire)
        };
        response_json(f.upload(&id, &cap, body).await, StatusCode::BAD_REQUEST).await;
        f.no_receipt(&id).await;
        f.no_files(&id);
        assert_eq!(f.store.status(&id, &cap).await.unwrap().state, "failed");
    }
    f.cleanup().await;
}

#[tokio::test]
async fn paired_http_marker_does_not_commit_before_actual_eof_or_survive_cancellation() {
    let mut f = Fixture::new().await;
    for outcome in ["eof", "late_error", "cancel"] {
        let (id, cap) = f.reserve().await;
        let wire = envelope(&id, b"image", Some(b"replay"));
        let (sender, receiver) = tokio::sync::mpsc::channel::<Chunk>(2);
        sender.send(Ok(Bytes::from(wire.clone()))).await.unwrap();
        let chunks = stream::unfold(receiver, |mut receiver| async move {
            receiver.recv().await.map(|chunk| (chunk, receiver))
        });
        let app = f.app.clone();
        let req = upload_request(&id, &cap, Body::from_stream(chunks));
        let pending = tokio::spawn(async move { app.oneshot(req).await.unwrap() });
        let partial = f.root.path().join(format!("{id}.part"));
        tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                if std::fs::metadata(&partial).is_ok_and(|m| m.len() == wire.len() as u64 - 8) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("Entire component payload must be consumed before awaiting HTTP EOF");
        assert!(!pending.is_finished());
        f.no_receipt(&id).await;
        if outcome == "cancel" {
            pending.abort();
            assert!(pending.await.unwrap_err().is_cancelled());
            drop(sender);
            f.no_receipt(&id).await;
            f.no_files(&id);
            f.store.abort_upload(&id, &cap).await.unwrap();
        } else {
            if outcome == "late_error" {
                sender
                    .send(Err(io::Error::other("late body failure")))
                    .await
                    .unwrap();
            }
            drop(sender);
            response_json(
                pending.await.unwrap(),
                if outcome == "eof" {
                    StatusCode::ACCEPTED
                } else {
                    StatusCode::BAD_REQUEST
                },
            )
            .await;
            if outcome == "eof" {
                f.receipt(&id, &wire, b"image", Some(b"replay")).await;
            } else {
                f.no_receipt(&id).await;
                f.no_files(&id);
            }
        }
    }
    f.cleanup().await;
}

#[tokio::test]
async fn paired_http_preserves_completed_input_across_sql_outage_and_reconciles_exactly() {
    let mut f = Fixture::new().await;
    let (id, cap) = f.reserve().await;
    let wire = envelope(&id, b"image", Some(b"replay"));
    let sending = wire.clone();
    let store = f.store.clone();
    let chunks = stream::once(async move {
        // The handler has already authenticated and begun receiving when it polls.
        store.close().await.unwrap();
        Ok::<_, io::Error>(Bytes::from(sending))
    });
    response_json(
        f.upload(&id, &cap, Body::from_stream(chunks)).await,
        StatusCode::SERVICE_UNAVAILABLE,
    )
    .await;
    assert_eq!(
        std::fs::read(f.root.path().join(format!("{id}.input"))).unwrap(),
        wire
    );
    assert!(!f.root.path().join(format!("{id}.part")).exists());
    let unqueued: bool = sqlx::query_scalar(
        "SELECT state='receiving' AND input_bytes IS NULL FROM media.jobs WHERE id=$1",
    )
    .bind(&id)
    .fetch_one(&f.admin)
    .await
    .unwrap();
    assert!(unqueued);
    let fresh = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let state = AppState::new(
        fresh.clone(),
        Quarantine::new(f.root.path()).unwrap(),
        "a".repeat(64),
    )
    .unwrap();
    assert!(
        paired::reconcile(&state, &id, &"0".repeat(64))
            .await
            .is_err()
    );
    paired::reconcile(&state, &id, &cap).await.unwrap();
    f.receipt(&id, &wire, b"image", Some(b"replay")).await;
    let before: String = sqlx::query_scalar(
        "SELECT expires_at::text || '/' || updated_at::text FROM media.jobs WHERE id=$1",
    )
    .bind(&id)
    .fetch_one(&f.admin)
    .await
    .unwrap();
    paired::reconcile(&state, &id, &cap).await.unwrap();
    let after: String = sqlx::query_scalar(
        "SELECT expires_at::text || '/' || updated_at::text FROM media.jobs WHERE id=$1",
    )
    .bind(&id)
    .fetch_one(&f.admin)
    .await
    .unwrap();
    assert_eq!(before, after);
    f.receipt(&id, &wire, b"image", Some(b"replay")).await;
    fresh.close().await.unwrap();
    f.cleanup().await;
}
