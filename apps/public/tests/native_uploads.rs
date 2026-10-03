#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    response::Response,
};
use board_store::{
    media::{Failure, MediaQueue},
    media_assets::OutputMetadata,
    media_intake::IntakeStore,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;

const ORIGIN: &str = "http://127.0.0.1:3000";
const PASSWORD: &str = "owned-native-upload-password";

fn form(board: &str, action: &str, body: String) -> Request<Body> {
    Request::post(format!("/{board}/{action}"))
        .header("origin", ORIGIN)
        .header("accept", "application/json")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(body))
        .unwrap()
}

fn upload(board: &str, bytes: &[u8]) -> Request<Body> {
    let mut body = format!(
        "--owned\r\nContent-Disposition: form-data; name=\"resto\"\r\n\r\n0\r\n--owned\r\nContent-Disposition: form-data; name=\"upfile\"; filename=\"native-{board}.png\"\r\nContent-Type: image/png\r\n\r\n"
    ).into_bytes();
    body.extend_from_slice(bytes);
    body.extend_from_slice(b"\r\n--owned--\r\n");
    Request::post(format!("/{board}/upload"))
        .header("origin", ORIGIN)
        .header("accept", "application/json")
        .header("content-type", "multipart/form-data; boundary=owned")
        .body(Body::from(body))
        .unwrap()
}

async fn native(response: Response, status: StatusCode) -> Value {
    assert_eq!(response.status(), status);
    assert_eq!(response.headers()["content-type"], "application/json");
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert!(response.headers().get_all("vary").iter().any(|value| {
        value
            .to_str()
            .unwrap()
            .split(',')
            .any(|part| part.trim() == "Accept")
    }));
    assert!(!response.headers().contains_key("location"));
    let policy = response.headers()["content-security-policy"]
        .to_str()
        .unwrap();
    assert!(
        policy
            .split(';')
            .any(|part| part.trim() == "connect-src 'none'")
    );
    assert!(
        policy
            .split(';')
            .any(|part| part.trim() == "frame-src 'none'")
    );
    let bytes = to_bytes(response.into_body(), 1024).await.unwrap();
    let text = std::str::from_utf8(&bytes).unwrap();
    assert!(
        !text.contains(&"a".repeat(64)),
        "Intake service credentials stay private"
    );
    assert!(
        !text.contains("native-"),
        "The response does not echo the filename"
    );
    let value: Value = serde_json::from_slice(&bytes).unwrap();
    if status.is_client_error() || status.is_server_error() {
        assert_eq!(value.as_object().unwrap().len(), 1);
        assert!(
            value["error"]
                .as_str()
                .is_some_and(|error| !error.is_empty())
        );
    }
    value
}

fn receipt(value: &Value) -> String {
    assert_eq!(value.as_object().unwrap().len(), 4);
    let id = value["upload_id"].as_str().unwrap();
    let capability = value["upload_capability"].as_str().unwrap();
    for (text, length) in [(id, 32), (capability, 64)] {
        assert_eq!(text.len(), length);
        assert!(
            text.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        );
    }
    assert_eq!(value["resto"], "0");
    format!("upload_id={id}&upload_capability={capability}&resto=0")
}

struct Fixture {
    app: Router,
    owner: PgPool,
    public: PgPool,
    intake: IntakeStore,
    queue: MediaQueue,
    board: String,
    root: std::path::PathBuf,
}

impl Fixture {
    async fn send(&self, request: Request<Body>, status: StatusCode) -> Value {
        native(self.app.clone().oneshot(request).await.unwrap(), status).await
    }

    async fn status(&self, body: &str, expected: StatusCode) -> Value {
        self.send(form(&self.board, "upload/status", body.into()), expected)
            .await
    }

    async fn exercise(&self) {
        let bytes = b"Opaque synthetic bytes are never decoded in public or intake.".repeat(128);
        let pending = self.send(upload(&self.board, &bytes), StatusCode::OK).await;
        let body = receipt(&pending);
        assert_eq!(pending["state"], "queued");
        let id = pending["upload_id"].as_str().unwrap();
        assert_eq!(
            std::fs::read(self.root.join(format!("{id}.input"))).unwrap(),
            bytes
        );
        assert_eq!(self.status(&body, StatusCode::OK).await, pending);
        for action in ["upload/status", "upload/cancel"] {
            self.send(
                form(
                    &self.board,
                    action,
                    format!(
                        "upload_id={id}&upload_capability={}&resto=0",
                        "0".repeat(64)
                    ),
                ),
                StatusCode::NOT_FOUND,
            )
            .await;
        }
        let posting = format!("{body}&pwd={PASSWORD}&sub=Owned+native+image&com=&spoiler=on");
        let denied = self
            .app
            .clone()
            .oneshot(form(&self.board, "imgboard.php", posting.clone()))
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::OK);
        let value: Value =
            serde_json::from_slice(&to_bytes(denied.into_body(), 8192).await.unwrap()).unwrap();
        assert!(
            value["error"].is_string(),
            "Queued input cannot be published through Quick Reply"
        );

        // This tests transport and persisted authorization with synthetic
        // coordinator approval. Actual guest execution is qualified separately.
        let claim = self.queue.claim().await.unwrap().unwrap();
        assert_eq!(claim.id, id, "Requires this test's idle disposable queue");
        let token = claim.lease_token.unwrap();
        let mut expected = pending.clone();
        expected["state"] = "processing".into();
        assert_eq!(self.status(&body, StatusCode::OK).await, expected);
        let output = self
            .queue
            .prepare_output(
                id,
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
        self.queue
            .approve_output(id, &token, &output.id)
            .await
            .unwrap();
        expected["state"] = "approved".into();
        assert_eq!(self.status(&body, StatusCode::OK).await, expected);
        let posted = self
            .app
            .clone()
            .oneshot(form(&self.board, "imgboard.php", posting.clone()))
            .await
            .unwrap();
        assert_eq!(posted.status(), StatusCode::OK);
        let value: Value =
            serde_json::from_slice(&to_bytes(posted.into_body(), 8192).await.unwrap()).unwrap();
        assert!(
            value.get("error").is_none(),
            "Owned approved image post was rejected: {value}"
        );
        assert_eq!(value["tid"], 0);
        let post = value["pid"].as_i64().unwrap();
        let saved = board_store::find_post(&self.public, &self.board, post)
            .await
            .unwrap();
        assert!(saved.comment.is_empty());
        let attached = board_store::post_media::attachment(&self.public, post)
            .await
            .unwrap()
            .unwrap();
        assert!(attached.spoiler);
        self.status(&body, StatusCode::CONFLICT).await;
        self.send(
            form(&self.board, "upload/cancel", body.clone()),
            StatusCode::CONFLICT,
        )
        .await;
        let repeated = self
            .app
            .clone()
            .oneshot(form(&self.board, "imgboard.php", posting))
            .await
            .unwrap();
        let repeated: Value =
            serde_json::from_slice(&to_bytes(repeated.into_body(), 8192).await.unwrap()).unwrap();
        assert!(repeated["error"].is_string());
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1")
            .bind(&self.board)
            .fetch_one(&self.owner)
            .await
            .unwrap();
        assert_eq!(count, 1, "The capability remains single-use");

        let second = self
            .send(upload(&self.board, b"Owned second input"), StatusCode::OK)
            .await;
        let second_body = receipt(&second);
        let claim = self.queue.claim().await.unwrap().unwrap();
        assert_eq!(claim.id, second["upload_id"].as_str().unwrap());
        self.queue
            .fail(
                &claim.id,
                &claim.lease_token.unwrap(),
                Failure::Processing,
                false,
            )
            .await
            .unwrap();
        assert_eq!(
            self.status(&second_body, StatusCode::OK).await["state"],
            "failed"
        );
        assert_eq!(
            self.send(
                form(&self.board, "upload/cancel", second_body.clone()),
                StatusCode::OK
            )
            .await,
            json!({"cancelled":true})
        );
        self.status(&second_body, StatusCode::NOT_FOUND).await;
        assert!(matches!(
            self.intake
                .status(
                    second["upload_id"].as_str().unwrap(),
                    second["upload_capability"].as_str().unwrap()
                )
                .await,
            Err(board_store::StoreError::NotFound)
        ));

        let expired = self
            .send(upload(&self.board, b"Owned expired input"), StatusCode::OK)
            .await;
        let expired_body = receipt(&expired);
        sqlx::query(
            "UPDATE media.jobs SET created_at=clock_timestamp()-interval '3 hours' WHERE id=$1",
        )
        .bind(expired["upload_id"].as_str().unwrap())
        .execute(&self.owner)
        .await
        .unwrap();
        self.status(&expired_body, StatusCode::NOT_FOUND).await;
        self.send(
            form(&self.board, "upload/cancel", expired_body),
            StatusCode::NOT_FOUND,
        )
        .await;
        self.send(
            form(
                &self.board,
                "upload/status",
                "resto=0&unexpected=private-sentinel".into(),
            ),
            StatusCode::UNPROCESSABLE_ENTITY,
        )
        .await;
        self.send(upload(&self.board, b""), StatusCode::UNPROCESSABLE_ENTITY)
            .await;

        let incomplete = self
            .intake
            .reserve(&format!("native-{}.png", self.board))
            .await
            .unwrap();
        let incomplete_body = format!(
            "upload_id={}&upload_capability={}&resto=0",
            incomplete.id, incomplete.capability
        );
        assert_eq!(
            self.status(&incomplete_body, StatusCode::OK).await["state"],
            "incomplete"
        );
        for accept in ["*/*", "application/json;q=1", "application/json, text/html"] {
            let mut request = form(&self.board, "upload/status", incomplete_body.clone());
            request
                .headers_mut()
                .insert("accept", accept.parse().unwrap());
            let response = self.app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response.headers()["content-type"],
                "text/html; charset=utf-8"
            );
            assert_eq!(response.headers()["cache-control"], "private, no-store");
            let html = to_bytes(response.into_body(), 262_144).await.unwrap();
            assert!(
                std::str::from_utf8(&html)
                    .unwrap()
                    .contains("The upload is incomplete.")
            );
        }
        self.send(
            form(&self.board, "upload/cancel", incomplete_body),
            StatusCode::OK,
        )
        .await;

        for (path, allowed) in [
            (format!("/{}/", self.board), true),
            (format!("/{}/catalog", self.board), false),
        ] {
            let response = self
                .app
                .clone()
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let policy = response.headers()["content-security-policy"]
                .to_str()
                .unwrap();
            let connect: Vec<_> = policy
                .split(';')
                .find_map(|part| part.trim().strip_prefix("connect-src "))
                .unwrap()
                .split_ascii_whitespace()
                .collect();
            assert_eq!(
                connect.contains(&format!("{ORIGIN}/{}/upload", self.board).as_str()),
                allowed
            );
            assert_eq!(
                connect.contains(&format!("{ORIGIN}/{}/upload/", self.board).as_str()),
                allowed
            );
            assert!(!connect.contains(&"'self'"));
            assert!(!connect.contains(&format!("{ORIGIN}/fixture/upload/").as_str()));
        }
    }
}

#[tokio::test]
async fn native_uploads_preserve_isolated_intake_and_one_use_posting_authority() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT substr(replace(gen_random_uuid()::text,'-',''),1,10)")
            .fetch_one(&owner)
            .await
            .unwrap();
    let filename = format!("native-{board}.png");
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit,comment_spoiler_cleanup) VALUES($1,'Native upload','Owned synthetic data',2000,100,100,100,10,10,true)")
        .bind(&board).execute(&owner).await.unwrap();
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
    let fixture = Fixture {
        app: board_public::routers_with_limits(
            public.clone(),
            ORIGIN.into(),
            false,
            Some(settings),
            board_config::PublicRequestLimits::default(),
        )
        .0,
        owner: owner.clone(),
        public: public.clone(),
        intake,
        queue: MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
            .await
            .unwrap(),
        board: board.clone(),
        root: root.path().to_owned(),
    };
    let outcome = tokio::spawn(async move { fixture.exercise().await }).await;
    let _ = stop.send(());
    server.await.unwrap();
    for statement in [
        "DELETE FROM content.post_media WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(statement)
            .bind(&board)
            .execute(&owner)
            .await
            .unwrap();
    }
    sqlx::query(
        "DELETE FROM media.assets WHERE job_id IN (SELECT id FROM media.jobs WHERE filename=$1)",
    )
    .bind(&filename)
    .execute(&owner)
    .await
    .unwrap();
    sqlx::query("DELETE FROM media.jobs WHERE filename=$1")
        .bind(&filename)
        .execute(&owner)
        .await
        .unwrap();
    public.close().await;
    owner.close().await;
    outcome.unwrap();
}
