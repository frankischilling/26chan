#![cfg(feature = "database-tests")]
//! Real runtime-role HTTP coverage. Approval is synthetic coordinator output;
//! these tests do not qualify Firecracker or a browser.
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use board_staff::{AppState, Config, Limits, auth};
use board_store::{media::MediaQueue, media_assets::OutputMetadata, media_intake::IntakeStore};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use tower::ServiceExt;
use webauthn_rs::prelude::*;

static SERIAL: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn pool(key: &str) -> PgPool {
    PgPool::connect(
        &std::env::var(key).expect("explicit disposable runtime-role credential required"),
    )
    .await
    .unwrap()
}
struct Fixture {
    owner: PgPool,
    state: Arc<AppState>,
    board: String,
    account: i64,
    token: String,
    csrf: String,
    intake: IntakeStore,
    root: tempfile::TempDir,
    stop: tokio::sync::oneshot::Sender<()>,
    server: tokio::task::JoinHandle<()>,
}
impl Fixture {
    async fn new() -> Self {
        let owner = pool("MIGRATION_DATABASE_URL").await;
        let board = format!("u{}", &uuid::Uuid::new_v4().simple().to_string()[..9]);
        sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,image_limit,comment_spoiler_cleanup) VALUES($1,'Owned staff uploads','Synthetic',16000,100,100,100,10,0,0,0,100,true)")
            .bind(&board).execute(&owner).await.unwrap();
        let account: i64 = sqlx::query_scalar("INSERT INTO staff_identity.accounts(role,flags) VALUES('moderator',ARRAY['capcode','capcodename']) RETURNING id").fetch_one(&owner).await.unwrap();
        let credential = uuid::Uuid::new_v4().as_bytes().to_vec();
        sqlx::query(
            "INSERT INTO staff_identity.credentials(id,account_id,credential) VALUES($1,$2,'{}')",
        )
        .bind(&credential)
        .bind(account)
        .execute(&owner)
        .await
        .unwrap();
        let token = auth::token();
        let csrf = auth::token();
        sqlx::query("INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id) VALUES($1,$2,$3,$4)")
            .bind(auth::hash(&token)).bind(auth::hash(&csrf)).bind(account).bind(credential).execute(&owner).await.unwrap();
        let intake = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let root = tempfile::tempdir().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let intake_app = board_media_intake::router(
            board_media_intake::AppState::with_staff_token(
                intake.clone(),
                board_media::Quarantine::new(root.path()).unwrap(),
                "a".repeat(64),
                Some("b".repeat(64)),
            )
            .unwrap(),
        );
        let (stop, stopped) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            axum::serve(listener, intake_app)
                .with_graceful_shutdown(async {
                    let _ = stopped.await;
                })
                .await
                .unwrap();
        });
        let origin = Url::parse("http://localhost:3001").unwrap();
        let state = Arc::new(AppState {
            config: Config {
                proxy: None,
                poster_id_key: Some(Arc::new(
                    board_domain::poster_id::PosterIdKey::parse(
                        &uuid::Uuid::new_v4().simple().to_string().repeat(2),
                    )
                    .unwrap(),
                )),
                country_database: None,
                origin: origin.origin().ascii_serialization(),
                public_origin: "http://127.0.0.1:3000".into(),
                media_origin: "http://127.0.0.2:3002".into(),
                bind: "127.0.0.1:3001".parse().unwrap(),
                production: false,
                auth_database: String::new(),
                staff_database: String::new(),
                idle_timeout: Duration::from_secs(900),
                tripcode_key: None,
                media: board_staff::config::parse_staff_media(
                    Some("isolated-development"),
                    Some(&address.to_string()),
                    Some(&"b".repeat(64)),
                    false,
                )
                .unwrap(),
            },
            auth: pool("AUTH_DATABASE_URL").await,
            staff: pool("STAFF_DATABASE_URL").await,
            webauthn: WebauthnBuilder::new("localhost", &origin)
                .unwrap()
                .build()
                .unwrap(),
            limits: Limits::default(),
        });
        Self {
            owner,
            state,
            board,
            account,
            token,
            csrf,
            intake,
            root,
            stop,
            server,
        }
    }
    fn app(&self) -> Router {
        board_staff::router(self.state.clone())
    }
    fn request(&self, path: &str, content_type: &str, body: Body) -> Request<Body> {
        Request::post(path)
            .header("origin", &self.state.config.origin)
            .header("sec-fetch-site", "same-origin")
            .header(
                "cookie",
                format!("staff={}; staff-csrf={}", self.token, self.csrf),
            )
            .header("content-type", content_type)
            .extension(axum::extract::ConnectInfo(
                "192.0.2.77:41000".parse::<std::net::SocketAddr>().unwrap(),
            ))
            .body(body)
            .unwrap()
    }
    fn multipart(&self, board: &str, extra: &str, file_bytes: usize) -> Request<Body> {
        let mut body = format!("--staff-boundary\r\nContent-Disposition: form-data; name=\"csrf\"\r\n\r\n{}\r\n--staff-boundary\r\nContent-Disposition: form-data; name=\"board\"\r\n\r\n{board}\r\n--staff-boundary\r\nContent-Disposition: form-data; name=\"thread\"\r\n\r\n0\r\n--staff-boundary\r\nContent-Disposition: form-data; name=\"upfile\"; filename=\"{}.png\"\r\nContent-Type: image/png\r\n\r\n", self.csrf, self.board).into_bytes();
        body.extend(vec![42; file_bytes]);
        body.extend(format!("\r\n{extra}--staff-boundary--\r\n").as_bytes());
        let chunks: Vec<Result<axum::body::Bytes, std::io::Error>> = body
            .chunks(4096)
            .map(|chunk| Ok(axum::body::Bytes::copy_from_slice(chunk)))
            .collect();
        self.request(
            "/post/upload",
            "multipart/form-data; boundary=staff-boundary",
            Body::from_stream(futures_util::stream::iter(chunks)),
        )
    }
    fn form(&self, id: &str, capability: &str) -> String {
        url::form_urlencoded::Serializer::new(String::new())
            .append_pair("csrf", &self.csrf)
            .append_pair("board", &self.board)
            .append_pair("thread", "0")
            .append_pair("upload_id", id)
            .append_pair("upload_capability", capability)
            .finish()
    }
    async fn send_form(&self, path: &str, body: String) -> axum::response::Response {
        self.app()
            .oneshot(self.request(path, "application/x-www-form-urlencoded", Body::from(body)))
            .await
            .unwrap()
    }
    async fn upload(&self) -> (String, String) {
        let response = self
            .app()
            .oneshot(self.multipart(&self.board, "", 256))
            .await
            .unwrap();
        let page = html(response, StatusCode::OK).await;
        assert!(
            !page.contains("<img"),
            "Unapproved input must never be previewed"
        );
        assert!(
            !page.contains(&"b".repeat(64)),
            "Intake credential entered rendered HTML"
        );
        let id = hidden(&page, "upload_id");
        let cap = hidden(&page, "upload_capability");
        assert_eq!(
            std::fs::read(self.root.path().join(format!("{id}.input"))).unwrap(),
            vec![42; 256]
        );
        (id, cap)
    }
    async fn approve(&self, id: &str) {
        let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let job = queue.claim().await.unwrap().expect("owned queued upload");
        assert_eq!(job.id, id, "requires isolated disposable media queue");
        let lease = job.lease_token.unwrap();
        let asset = queue
            .prepare_output(
                id,
                &lease,
                &OutputMetadata {
                    sha256: "c".repeat(64),
                    bytes: 123,
                    width: 10,
                    height: 20,
                },
            )
            .await
            .unwrap();
        queue.approve_output(id, &lease, &asset.id).await.unwrap();
    }
    async fn jobs(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM media.jobs WHERE filename=$1")
            .bind(format!("{}.png", self.board))
            .fetch_one(&self.owner)
            .await
            .unwrap()
    }
    async fn cleanup(self) {
        let _ = self.stop.send(());
        tokio::time::timeout(Duration::from_secs(5), self.server)
            .await
            .unwrap()
            .unwrap();
        for sql in [
            "DELETE FROM content.moderation_audit WHERE board=$1",
            "DELETE FROM content.post_media WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
            "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
            "DELETE FROM content.posts WHERE board=$1",
            "DELETE FROM content.threads WHERE board=$1",
            "DELETE FROM content.boards WHERE slug=$1",
        ] {
            sqlx::query(sql)
                .bind(&self.board)
                .execute(&self.owner)
                .await
                .unwrap();
        }
        for sql in [
            "DELETE FROM media.assets WHERE job_id IN(SELECT id FROM media.jobs WHERE filename=$1)",
            "DELETE FROM media.jobs WHERE filename=$1",
        ] {
            sqlx::query(sql)
                .bind(format!("{}.png", self.board))
                .execute(&self.owner)
                .await
                .unwrap();
        }
        for sql in [
            "DELETE FROM staff_identity.sessions WHERE account_id=$1",
            "DELETE FROM staff_identity.credentials WHERE account_id=$1",
            "DELETE FROM staff_identity.accounts WHERE id=$1",
        ] {
            sqlx::query(sql)
                .bind(self.account)
                .execute(&self.owner)
                .await
                .unwrap();
        }
    }
}
async fn html(response: axum::response::Response, status: StatusCode) -> String {
    assert_eq!(response.status(), status);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert!(response.headers().get("location").is_none());
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
        .expect("receipt hidden field")
        .split('"')
        .next()
        .unwrap()
        .into()
}

#[tokio::test]
async fn authenticated_upload_status_badged_post_spoiler_and_one_use_receipt() {
    let _serial = SERIAL.lock().await;
    let fixture = Arc::new(Fixture::new().await);
    let case = fixture.clone();
    let result = tokio::spawn(async move {
        let (id, capability) = case.upload().await;
        assert_eq!(case.intake.status(&id, &capability).await.unwrap().state, "queued");
        let form = case.form(&id, &capability);
        let pending = html(case.send_form("/post/upload/status", form.clone()).await, StatusCode::OK).await;
        assert!(!pending.contains("<img"));
        // Staff final-post conflicts use the established Posting error (400).
        assert_eq!(case.send_form("/post", format!("{form}&name=Owned&subject=Owned&comment=Owned&badge=mod&spoiler=true")).await.status(), StatusCode::BAD_REQUEST);
        case.approve(&id).await;
        let approved = html(case.send_form("/post/upload/status", form.clone()).await, StatusCode::OK).await;
        assert!(approved.contains("action=\"/post\""));
        let post = format!("{form}&name=Owned&subject=Owned&comment=Owned+image&badge=mod&spoiler=true");
        let response = case.send_form("/post", post.clone()).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let location = response.headers()["location"].to_str().unwrap();
        assert!(!location.contains(&id)); assert!(!location.contains(&capability)); drop(response);
        let saved: (i64, bool, Option<String>) = sqlx::query_as("SELECT p.id,m.spoiler,p.capcode FROM content.posts p JOIN content.post_media m ON m.post_id=p.id WHERE p.board=$1").bind(&case.board).fetch_one(&case.owner).await.unwrap();
        assert!(saved.1); assert_eq!(saved.2.as_deref(), Some("mod"));
        let leaked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM content.posts p WHERE p.board=$1 AND to_jsonb(p)::text LIKE '%'||$2||'%') OR EXISTS(SELECT 1 FROM content.moderation_audit a WHERE a.board=$1 AND to_jsonb(a)::text LIKE '%'||$2||'%')").bind(&case.board).bind(&capability).fetch_one(&case.owner).await.unwrap();
        assert!(!leaked, "receipt capability was persisted in content or audit rows");
        let repeated = case.send_form("/post", post).await;
        assert!(!repeated.status().is_success() && repeated.status() != StatusCode::SEE_OTHER);
        let error = String::from_utf8(to_bytes(repeated.into_body(), 262_144).await.unwrap().to_vec()).unwrap();
        assert!(!error.contains(&capability)); assert!(!error.contains(&id));
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1").bind(&case.board).fetch_one(&case.owner).await.unwrap(); assert_eq!(count, 1);
    }).await;
    Arc::try_unwrap(fixture).ok().unwrap().cleanup().await;
    result.unwrap();
}

// A request that panics if its body is polled proves that header/session checks
// precede extraction, rather than merely observing a denial after buffering.
fn unpolled() -> Body {
    use http_body_util::BodyExt;
    Body::new(
        http_body_util::Full::new(axum::body::Bytes::from_static(b"must not be polled"))
            .map_frame(|_| panic!("rejected upload body was polled")),
    )
}

#[tokio::test]
async fn upload_auth_origin_recent_and_scope_reject_before_reservation() {
    let _serial = SERIAL.lock().await;
    let fixture = Arc::new(Fixture::new().await);
    let case = fixture.clone();
    let result = tokio::spawn(async move {
        for path in ["/post/upload", "/post/upload/status", "/post/upload/cancel"] {
            let mut request = case.request(path, "multipart/form-data; boundary=staff-boundary", unpolled());
            request.headers_mut().remove("cookie");
            assert_eq!(case.app().oneshot(request).await.unwrap().status(), StatusCode::UNAUTHORIZED);
            let mut request = case.request(path, "multipart/form-data; boundary=staff-boundary", unpolled());
            request.headers_mut().insert("origin", "https://attacker.invalid".parse().unwrap());
            assert_eq!(case.app().oneshot(request).await.unwrap().status(), StatusCode::FORBIDDEN);
        }
        sqlx::query("UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp()-interval '11 minutes' WHERE account_id=$1").bind(case.account).execute(&case.owner).await.unwrap();
        let response = case.app().oneshot(case.request("/post/upload", "multipart/form-data; boundary=staff-boundary", unpolled())).await.unwrap();
        assert!(!response.status().is_success()); drop(response);
        sqlx::query("UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp() WHERE account_id=$1").bind(case.account).execute(&case.owner).await.unwrap();
        for board in ["j", "missing", "invalid/board"] {
            let response = case.app().oneshot(case.multipart(board, "", 256)).await.unwrap();
            assert!(!response.status().is_success());
        }
        sqlx::query("UPDATE staff_identity.accounts SET deny_boards=ARRAY[$2] WHERE id=$1").bind(case.account).bind(&case.board).execute(&case.owner).await.unwrap();
        assert_eq!(case.app().oneshot(case.multipart(&case.board, "", 256)).await.unwrap().status(), StatusCode::FORBIDDEN);
        assert_eq!(case.jobs().await, 0);
    }).await;
    Arc::try_unwrap(fixture).ok().unwrap().cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn upload_cancel_bad_receipts_expiry_and_authority_revocation_are_private() {
    let _serial = SERIAL.lock().await;
    let fixture = Arc::new(Fixture::new().await);
    let case = fixture.clone();
    let result = tokio::spawn(async move {
        let (id, capability) = case.upload().await;
        for path in ["/post/upload/status", "/post/upload/cancel"] {
            let bad = case.send_form(path, case.form(&id, &"f".repeat(64))).await;
            assert!(!bad.status().is_success());
            let page = String::from_utf8(to_bytes(bad.into_body(), 262_144).await.unwrap().to_vec()).unwrap();
            assert!(!page.contains(&id)); assert!(!page.contains(&capability));
        }
        // Revoking board authority after intake must stop status, cancellation,
        // and posting; possession of a receipt does not restore staff authority.
        sqlx::query("UPDATE staff_identity.accounts SET deny_boards=ARRAY[$2] WHERE id=$1").bind(case.account).bind(&case.board).execute(&case.owner).await.unwrap();
        for path in ["/post/upload/status", "/post/upload/cancel", "/post"] {
            let mut form = case.form(&id, &capability);
            if path == "/post" { form.push_str("&name=Owned&subject=Owned&comment=Owned&badge=mod"); }
            let response = case.send_form(path, form).await;
            assert_eq!(response.status(), StatusCode::FORBIDDEN);
        }
        sqlx::query("UPDATE staff_identity.accounts SET deny_boards=ARRAY[]::text[] WHERE id=$1").bind(case.account).execute(&case.owner).await.unwrap();
        let response = case.send_form("/post/upload/cancel", case.form(&id, &capability)).await;
        assert!(response.status().is_success() || response.status() == StatusCode::SEE_OTHER);
        assert!(case.intake.status(&id, &capability).await.is_err(), "canceled capability must be revoked; file cleanup belongs to the intake worker");
        let (expired_id, expired_capability) = case.upload().await;
        sqlx::query("UPDATE media.jobs SET expires_at=clock_timestamp()-interval '1 second',created_at=clock_timestamp()-interval '3 hours' WHERE id=$1").bind(&expired_id).execute(&case.owner).await.unwrap();
        for path in ["/post/upload/status", "/post"] {
            let mut form = case.form(&expired_id, &expired_capability);
            if path == "/post" { form.push_str("&name=Owned&subject=Owned&comment=Owned&badge=mod"); }
            let response = case.send_form(path, form).await;
            assert!(!response.status().is_success() && response.status() != StatusCode::SEE_OTHER);
            let page = String::from_utf8(to_bytes(response.into_body(), 262_144).await.unwrap().to_vec()).unwrap(); assert!(!page.contains(&expired_capability));
        }
        assert_eq!(sqlx::query_scalar::<_, i64>("SELECT count(*) FROM content.posts WHERE board=$1").bind(&case.board).fetch_one(&case.owner).await.unwrap(), 0);
    }).await;
    Arc::try_unwrap(fixture).ok().unwrap().cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn upload_duplicate_extra_and_oversized_parts_fail_closed() {
    let _serial = SERIAL.lock().await;
    let fixture = Arc::new(Fixture::new().await);
    let case = fixture.clone();
    let result = tokio::spawn(async move {
        for name in ["upfile", "csrf", "board", "thread", "upload_id", "unexpected"] {
            let extra = format!("--staff-boundary\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\nunexpected\r\n");
            let response = case.app().oneshot(case.multipart(&case.board, &extra, 256)).await.unwrap();
            assert!(!response.status().is_success());
            let active: i64 = sqlx::query_scalar("SELECT count(*) FROM media.jobs j JOIN media_intake.handles h ON h.job_id=j.id WHERE j.filename=$1").bind(format!("{}.png", case.board)).fetch_one(&case.owner).await.unwrap();
            assert_eq!(active, 0, "malformed multipart left an active reservation");
        }
        let response = case.app().oneshot(case.multipart(&case.board, "", 17 * 1024 * 1024)).await.unwrap();
        assert!(!response.status().is_success());
        let request = case.multipart(&case.board, "", 256);
        let bytes = to_bytes(request.into_body(), 32 * 1024).await.unwrap();
        let body = String::from_utf8(bytes.to_vec()).unwrap().replace("Content-Type: image/png", &format!("X-Owned-Padding: {}\r\nContent-Type: image/png", "x".repeat(17 * 1024)));
        let response = case.app().oneshot(case.request("/post/upload", "multipart/form-data; boundary=staff-boundary", Body::from(body))).await.unwrap();
        assert!(!response.status().is_success(), "small file bypassed bounded multipart overhead");
    }).await;
    Arc::try_unwrap(fixture).ok().unwrap().cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn final_post_requires_paired_receipt() {
    let _serial = SERIAL.lock().await;
    let fixture = Arc::new(Fixture::new().await);
    let case = fixture.clone();
    let result = tokio::spawn(async move {
        for receipt in ["upload_id=owned-id", "upload_capability=owned-capability", "upload_id=&upload_capability=owned-capability", "upload_id=owned-id&upload_capability="] {
            let response = case.send_form("/post", format!("csrf={}&board={}&thread=0&name=Owned&subject=Owned&comment=Owned&badge=mod&{receipt}", case.csrf, case.board)).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        }
        assert_eq!(case.jobs().await, 0);
    }).await;
    Arc::try_unwrap(fixture).ok().unwrap().cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn upload_moderator_closed_thread_exception_does_not_enable_disabled_or_private_boards() {
    let _serial = SERIAL.lock().await;
    let fixture = Arc::new(Fixture::new().await);
    let case = fixture.clone();
    let result = tokio::spawn(async move {
        sqlx::query("UPDATE content.boards SET image_limit=0 WHERE slug=$1").bind(&case.board).execute(&case.owner).await.unwrap();
        for role in ["janitor", "moderator", "manager", "admin"] {
            sqlx::query("UPDATE staff_identity.accounts SET role=$2 WHERE id=$1").bind(case.account).bind(role).execute(&case.owner).await.unwrap();
            assert_eq!(case.app().oneshot(case.multipart(&case.board, "", 256)).await.unwrap().status(), StatusCode::FORBIDDEN);
        }
        assert_eq!(case.jobs().await, 0);
        sqlx::query("UPDATE content.boards SET image_limit=100 WHERE slug=$1").bind(&case.board).execute(&case.owner).await.unwrap();
        let thread: i64 = sqlx::query_scalar("INSERT INTO content.threads(board,closed) VALUES($1,true) RETURNING id").bind(&case.board).fetch_one(&case.owner).await.unwrap();
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Owned','','Owned closed thread')").bind(thread).bind(&case.board).execute(&case.owner).await.unwrap();
        for role in ["janitor", "moderator"] {
            sqlx::query("UPDATE staff_identity.accounts SET role=$2 WHERE id=$1").bind(case.account).bind(role).execute(&case.owner).await.unwrap();
            let request = case.multipart(&case.board, "", 256);
            let bytes = to_bytes(request.into_body(), 32_768).await.unwrap();
            let body = String::from_utf8(bytes.to_vec()).unwrap().replace("name=\"thread\"\r\n\r\n0\r\n", &format!("name=\"thread\"\r\n\r\n{thread}\r\n"));
            let response = case.app().oneshot(case.request("/post/upload", "multipart/form-data; boundary=staff-boundary", Body::from(body))).await.unwrap();
            if role == "janitor" { assert_eq!(response.status(), StatusCode::FORBIDDEN); }
            else {
                let page = html(response, StatusCode::OK).await;
                let (id, cap) = (hidden(&page,"upload_id"), hidden(&page,"upload_capability"));
                let form = case.form(&id,&cap).replace("thread=0", &format!("thread={thread}"));
                assert_eq!(case.send_form("/post/upload/cancel", form).await.status(), StatusCode::SEE_OTHER);
            }
        }
        sqlx::query("UPDATE content.boards SET staff_only=true WHERE slug=$1").bind(&case.board).execute(&case.owner).await.unwrap();
        assert_eq!(case.app().oneshot(case.multipart(&case.board, "", 256)).await.unwrap().status(), StatusCode::FORBIDDEN);
    }).await;
    Arc::try_unwrap(fixture).ok().unwrap().cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn upload_ordinary_attachment_preserves_password_identity_and_spoiler() {
    let _serial = SERIAL.lock().await;
    let fixture = Arc::new(Fixture::new().await);
    let case = fixture.clone();
    let result = tokio::spawn(async move {
        let (id, cap) = case.upload().await; case.approve(&id).await;
        let form = format!("{}&name=Owned&subject=Owned&comment=Owned+ordinary+image&badge=none&password=owned-upload-password&spoiler=true", case.form(&id, &cap));
        let response = case.send_form("/post", form).await;
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let saved: (Option<String>, bool, bool) = sqlx::query_as("SELECT p.capcode,m.spoiler,EXISTS(SELECT 1 FROM post_secrets.deletion d WHERE d.post_id=p.id) FROM content.posts p JOIN content.post_media m ON m.post_id=p.id WHERE p.board=$1").bind(&case.board).fetch_one(&case.owner).await.unwrap();
        assert_eq!(saved, (None, true, true));
    }).await;
    Arc::try_unwrap(fixture).ok().unwrap().cleanup().await;
    result.unwrap();
}

struct StreamLifetime(Arc<std::sync::atomic::AtomicBool>);
impl Drop for StreamLifetime {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}
fn stalled_body(dropped: Arc<std::sync::atomic::AtomicBool>) -> Body {
    let lifetime = StreamLifetime(dropped);
    Body::from_stream(futures_util::stream::unfold(
        lifetime,
        |lifetime| async move {
            std::future::pending::<()>().await;
            Some((Ok::<_, std::io::Error>(axum::body::Bytes::new()), lifetime))
        },
    ))
}

#[tokio::test]
async fn upload_receive_deadline_is_bounded_and_distinct_from_ordinary_staff_timeout() {
    let _serial = SERIAL.lock().await;
    let fixture = Arc::new(Fixture::new().await);
    let case = fixture.clone();
    let result = tokio::spawn(async move {
        for (path, content_type, minimum, maximum) in [
            ("/post", "application/x-www-form-urlencoded", 9, 14),
            (
                "/post/upload",
                "multipart/form-data; boundary=staff-boundary",
                30,
                37,
            ),
        ] {
            let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let start = std::time::Instant::now();
            let response = tokio::time::timeout(
                Duration::from_secs(maximum),
                case.app()
                    .oneshot(case.request(path, content_type, stalled_body(dropped.clone()))),
            )
            .await
            .expect("request exceeded its bounded deadline")
            .unwrap();
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
            assert!(
                start.elapsed() >= Duration::from_secs(minimum),
                "ordinary timeout was incorrectly applied to upload"
            );
            assert!(
                dropped.load(std::sync::atomic::Ordering::SeqCst),
                "timed-out producer was detached"
            );
        }
        assert_eq!(case.jobs().await, 0);
    })
    .await;
    Arc::try_unwrap(fixture).ok().unwrap().cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn partial_upload_disconnect_drops_joined_producer_and_revokes_known_reservation() {
    use futures_util::StreamExt;
    let _serial = SERIAL.lock().await;
    let fixture = Arc::new(Fixture::new().await);
    let case = fixture.clone();
    let result = tokio::spawn(async move {
        let full = case.multipart(&case.board, "", 256);
        let bytes = to_bytes(full.into_body(), 1024 * 1024).await.unwrap();
        let truncated = bytes.slice(..bytes.len()-24);
        let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let lifetime = StreamLifetime(dropped.clone());
        let stream = futures_util::stream::once(async move { Ok::<_, std::io::Error>(truncated) })
            .chain(futures_util::stream::once(async move {
                let _lifetime = lifetime;
                tokio::time::sleep(Duration::from_millis(100)).await;
                Err(std::io::Error::new(std::io::ErrorKind::ConnectionReset, "owned synthetic disconnect"))
            }));
        let response = tokio::time::timeout(Duration::from_secs(8), case.app().oneshot(case.request("/post/upload", "multipart/form-data; boundary=staff-boundary", Body::from_stream(stream)))).await.unwrap().unwrap();
        assert!(!response.status().is_success());
        assert!(dropped.load(std::sync::atomic::Ordering::SeqCst));
        let handles: i64 = sqlx::query_scalar("SELECT count(*) FROM media_intake.handles h JOIN media.jobs j ON j.id=h.job_id WHERE j.filename=$1").bind(format!("{}.png", case.board)).fetch_one(&case.owner).await.unwrap();
        assert_eq!(handles, 0);
    }).await;
    Arc::try_unwrap(fixture).ok().unwrap().cleanup().await;
    result.unwrap();
}

/// Explicit browser qualification, separate from the ordinary HTTP suite.
/// Run with the disposable database roles, installed Playwright Chromium, and
/// `cargo test -p board-staff --features database-tests --test uploads
/// script_disabled_browser -- --ignored --exact`.
#[tokio::test]
#[ignore = "requires installed Playwright Chromium; synthetic coordinator, not Firecracker"]
async fn script_disabled_browser() {
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
    let _serial = SERIAL.lock().await;
    let mut fixture = Fixture::new().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    Arc::get_mut(&mut fixture.state).unwrap().config.origin = origin.clone();
    let app = fixture.app();
    let (stop, stopped) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(async {
            let _ = stopped.await;
        })
        .await
        .unwrap();
    });
    let fixture = Arc::new(fixture);
    let case = fixture.clone();
    let result = tokio::spawn(async move {
        let script = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/browser/staff-upload-script-free.mjs");
        let mut child = tokio::process::Command::new("node").arg(script).stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::inherit()).kill_on_drop(true).spawn().unwrap();
        use futures_util::FutureExt;
        let browser = std::panic::AssertUnwindSafe(async {
            let mut input = child.stdin.take().unwrap();
            input.write_all(format!("{}\n", serde_json::json!({"origin":origin,"board":case.board,"token":case.token,"csrf":case.csrf})).as_bytes()).await.unwrap();
            let mut output = tokio::io::BufReader::new(child.stdout.take().unwrap()).lines();
            let line = tokio::time::timeout(Duration::from_secs(30), output.next_line()).await.unwrap().unwrap().expect("browser upload result missing");
            let event: serde_json::Value = serde_json::from_str(&line).unwrap();
            let id = event["upload"].as_str().unwrap();
            case.approve(id).await;
            input.write_all(b"approved\n").await.unwrap();
            drop(input);
            assert!(tokio::time::timeout(Duration::from_secs(30), child.wait()).await.unwrap().unwrap().success(), "script-disabled browser failed");
        }).catch_unwind().await;
        if browser.is_err() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        browser.unwrap();
        let saved: (i64, bool) = sqlx::query_as("SELECT count(*),bool_and(m.spoiler) FROM content.posts p JOIN content.post_media m ON m.post_id=p.id WHERE p.board=$1").bind(&case.board).fetch_one(&case.owner).await.unwrap();
        assert_eq!(saved, (1, true));
    }).await;
    let _ = stop.send(());
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();
    Arc::try_unwrap(fixture).ok().unwrap().cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn queued_cancellation_wait_does_not_hold_account_authority_and_rechecks_revocation() {
    let _serial = SERIAL.lock().await;
    let fixture = Arc::new(Fixture::new().await);
    let case = fixture.clone();
    let result = tokio::spawn(async move {
        let (id, cap) = case.upload().await;
        let mut blocker = case.owner.begin().await.unwrap();
        let blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *blocker).await.unwrap();
        sqlx::query("SELECT id FROM media.jobs WHERE id=$1 FOR UPDATE").bind(&id).fetch_one(&mut *blocker).await.unwrap();
        let caller = case.clone();
        let form = case.form(&id, &cap);
        let mut cancel = tokio::spawn(async move { caller.send_form("/post/upload/cancel", form).await });
        let waiting = tokio::time::timeout(Duration::from_secs(3), async {
            loop {
                let blocked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity a WHERE a.usename='board_staff' AND $1=ANY(pg_blocking_pids(a.pid)))").bind(blocker_pid).fetch_one(&case.owner).await.unwrap();
                if blocked { break; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await;
        // Updating this owned account must complete while media is locked. An
        // account-first cancellation would block this update and create a cycle
        // against the posting consumer's media-first lock ordering.
        let revoked = tokio::time::timeout(Duration::from_secs(2), sqlx::query("UPDATE staff_identity.accounts SET deny_boards=ARRAY[$2] WHERE id=$1").bind(case.account).bind(&case.board).execute(&case.owner)).await;
        blocker.rollback().await.unwrap();
        let canceled = tokio::time::timeout(Duration::from_secs(5), &mut cancel).await;
        if canceled.is_err() { cancel.abort(); let _ = cancel.await; }
        assert!(waiting.is_ok(), "did not observe actual cancellation media lock wait");
        assert!(revoked.is_ok_and(|value| value.is_ok()), "cancellation held account authority while awaiting media lock");
        let response = canceled.expect("cancellation task failed to terminate").unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN, "queued cancellation failed to recheck board revocation");
        assert!(case.intake.status(&id, &cap).await.is_ok(), "denied cancellation consumed the receipt");
    }).await;
    Arc::try_unwrap(fixture).ok().unwrap().cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn csrf_and_board_authority_reject_before_file_stream_polling() {
    use futures_util::StreamExt;
    let _serial = SERIAL.lock().await;
    let fixture = Arc::new(Fixture::new().await);
    let case = fixture.clone();
    let result = tokio::spawn(async move {
        for (csrf, board) in [("invalid".to_owned(), case.board.clone()), (case.csrf.clone(), "j".into()), (case.csrf.clone(), "missing".into())] {
            let head = format!("--staff-boundary\r\nContent-Disposition: form-data; name=\"csrf\"\r\n\r\n{csrf}\r\n--staff-boundary\r\nContent-Disposition: form-data; name=\"board\"\r\n\r\n{board}\r\n--staff-boundary\r\nContent-Disposition: form-data; name=\"thread\"\r\n\r\n0\r\n--staff-boundary\r\nContent-Disposition: form-data; name=\"upfile\"; filename=\"{}.png\"\r\nContent-Type: image/png\r\n\r\n", case.board);
            let stream = futures_util::stream::once(async move { Ok::<_, std::io::Error>(axum::body::Bytes::from(head)) })
                .chain(futures_util::stream::once(async { panic!("file stream was polled before CSRF/board authorization") }));
            let response = case.app().oneshot(case.request("/post/upload", "multipart/form-data; boundary=staff-boundary", Body::from_stream(stream))).await.unwrap();
            assert!(!response.status().is_success());
        }
        assert_eq!(case.jobs().await, 0);
    }).await;
    Arc::try_unwrap(fixture).ok().unwrap().cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn disabled_upload_routes_reject_without_polling_bodies() {
    let _serial = SERIAL.lock().await;
    let mut fixture = Fixture::new().await;
    Arc::get_mut(&mut fixture.state).unwrap().config.media = None;
    let fixture = Arc::new(fixture);
    let case = fixture.clone();
    let result = tokio::spawn(async move {
        for path in ["/post/upload", "/post/upload/status", "/post/upload/cancel"] {
            assert_eq!(
                case.app()
                    .oneshot(case.request(path, "application/x-www-form-urlencoded", unpolled()))
                    .await
                    .unwrap()
                    .status(),
                StatusCode::NOT_FOUND
            );
        }
        assert_eq!(case.jobs().await, 0);
    })
    .await;
    Arc::try_unwrap(fixture).ok().unwrap().cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn receipt_render_wait_rechecks_authority_without_holding_account_locks() {
    use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};
    let _serial = SERIAL.lock().await;
    for status_request in [false, true] {
        for revoked in ["account", "board", "recent"] {
            let mut fixture = Fixture::new().await;
            let existing = if status_request {
                Some(fixture.upload().await)
            } else {
                None
            };
            let reached = Arc::new(tokio::sync::Notify::new());
            let resume = Arc::new(tokio::sync::Notify::new());
            let releases = Arc::new(AtomicUsize::new(0));
            let reader_pid = Arc::new(AtomicI32::new(0));
            // The migrator cannot see another role's query text. Observe from
            // a separate real staff connection, without new monitoring grants.
            let observer = fixture.state.staff.clone();
            // A lazy one-connection real-role pool has no setup release. The
            // second return is the final settings read (upload), or the receipt
            // check (status). The next staff query is the rendering board list.
            let staff = sqlx::postgres::PgPoolOptions::new()
                .min_connections(0)
                .max_connections(1)
                .acquire_timeout(Duration::from_secs(2))
                .after_connect({
                    let reader_pid = reader_pid.clone();
                    move |connection, _| {
                        let reader_pid = reader_pid.clone();
                        Box::pin(async move {
                            let (role, pid): (String, i32) =
                                sqlx::query_as("SELECT current_user,pg_backend_pid()")
                                    .fetch_one(connection)
                                    .await?;
                            assert_eq!(role, "board_staff");
                            reader_pid.store(pid, Ordering::SeqCst);
                            Ok(())
                        })
                    }
                })
                .after_release({
                    let reached = reached.clone();
                    let resume = resume.clone();
                    let releases = releases.clone();
                    move |_, _| {
                        let pause = releases.fetch_add(1, Ordering::SeqCst) == 1;
                        let reached = reached.clone();
                        let resume = resume.clone();
                        Box::pin(async move {
                            if pause {
                                reached.notify_one();
                                tokio::time::timeout(Duration::from_secs(5), resume.notified())
                                    .await
                                    .map_err(|_| sqlx::Error::PoolTimedOut)?;
                            }
                            Ok(true)
                        })
                    }
                })
                .connect_lazy(&std::env::var("STAFF_DATABASE_URL").unwrap())
                .unwrap();
            Arc::get_mut(&mut fixture.state).unwrap().staff = staff;
            let fixture = Arc::new(fixture);
            let case = fixture.clone();
            let result = tokio::spawn(async move {
                let caller = case.clone(); let original = existing.clone();
                let mut request = tokio::spawn(async move {
                    if let Some((id, cap)) = original {
                        caller.send_form("/post/upload/status", caller.form(&id, &cap)).await
                    } else {
                        caller.app().oneshot(caller.multipart(&caller.board, "", 256)).await.unwrap()
                    }
                });
                let barrier = tokio::time::timeout(Duration::from_secs(5), reached.notified()).await;
                if barrier.is_err() { request.abort(); let _ = request.await; panic!("did not reach owned render barrier"); }
                let mut blocker = case.owner.begin().await.unwrap();
                let blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *blocker).await.unwrap();
                sqlx::query("LOCK TABLE content.boards IN ACCESS EXCLUSIVE MODE").execute(&mut *blocker).await.unwrap();
                resume.notify_one();
                let rendering = tokio::time::timeout(Duration::from_secs(2), async {
                    loop {
                        let blocked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity a WHERE a.pid=$2 AND a.query LIKE 'SELECT slug,title,%' AND $1=ANY(pg_blocking_pids(a.pid)))").bind(blocker_pid).bind(reader_pid.load(Ordering::SeqCst)).fetch_one(&observer).await.unwrap();
                        if blocked { break; }
                        tokio::task::yield_now().await;
                    }
                }).await;
                let sql = match revoked {
                    "account" => "UPDATE staff_identity.accounts SET revoked_at=clock_timestamp() WHERE id=$1",
                    "board" => "UPDATE staff_identity.accounts SET deny_boards=ARRAY[$2] WHERE id=$1",
                    _ => "UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp()-interval '11 minutes' WHERE account_id=$1",
                };
                let mut query = sqlx::query(sql).bind(case.account);
                if revoked == "board" { query = query.bind(&case.board); }
                let changed = tokio::time::timeout(Duration::from_secs(2), query.execute(&case.owner)).await;
                blocker.rollback().await.unwrap();
                let response = tokio::time::timeout(Duration::from_secs(5), &mut request).await;
                if response.is_err() { request.abort(); let _ = request.await; }
                assert!(rendering.is_ok(), "did not observe real receipt render SQL wait");
                assert!(changed.is_ok_and(|value| value.is_ok()), "rendering retained an account/session authority lock");
                let response = response.expect("rendering request failed to terminate").unwrap();
                assert_eq!(response.status(), if revoked == "account" { StatusCode::UNAUTHORIZED } else { StatusCode::FORBIDDEN }, "late render denial must reflect current authority, not a database timeout");
                let page = String::from_utf8(to_bytes(response.into_body(), 262_144).await.unwrap().to_vec()).unwrap();
                assert!(!page.contains("name=\"upload_capability\""));
                assert!(!page.contains("name=\"upload_id\""));
                if let Some((id, cap)) = existing {
                    assert!(!page.contains(&cap));
                    assert!(case.intake.status(&id, &cap).await.is_ok(), "denied status revoked an existing receipt");
                } else {
                    assert_eq!(case.jobs().await, 1, "late-denial test never created its upload");
                    let handles: i64 = sqlx::query_scalar("SELECT count(*) FROM media_intake.handles h JOIN media.jobs j ON j.id=h.job_id WHERE j.filename=$1").bind(format!("{}.png", case.board)).fetch_one(&case.owner).await.unwrap();
                    assert_eq!(handles, 0, "late upload denial did not revoke its newly-created receipt");
                }
            }).await;
            Arc::try_unwrap(fixture).ok().unwrap().cleanup().await;
            result.unwrap();
        }
    }
}

#[tokio::test]
async fn operational_intake_failures_are_unavailable_and_preserve_existing_receipt() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let _serial = SERIAL.lock().await;
    let mut fixture = Fixture::new().await;
    use futures_util::FutureExt;
    let result = std::panic::AssertUnwindSafe(async {
        let (id, cap) = fixture.upload().await;
        for fault in ["unavailable", "malformed", "disconnect", "timeout"] {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            Arc::get_mut(&mut fixture.state).unwrap().config.media = board_staff::config::parse_staff_media(Some("isolated-development"), Some(&address.to_string()), Some(&"b".repeat(64)), false).unwrap();
            // Fault injection only. The receipt and all auth/content queries above
            // and below are created and checked by the actual runtime-role services.
            let mut server = tokio::spawn(async move {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut request = Vec::new();
                loop {
                    let byte = socket.read_u8().await.unwrap(); request.push(byte);
                    if request.ends_with(b"\r\n\r\n") { break; }
                    assert!(request.len() <= 16_384);
                }
                match fault {
                    "unavailable" => socket.write_all(b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").await.unwrap(),
                    "malformed" => socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}").await.unwrap(),
                    "timeout" => { assert_eq!(socket.read(&mut [0]).await.unwrap(), 0, "timed-out client did not close its connection"); }
                    _ => {},
                }
            });
            let started = std::time::Instant::now();
            let response = tokio::time::timeout(Duration::from_secs(23), fixture.send_form("/post/upload/status", fixture.form(&id, &cap))).await;
            if response.is_err() { server.abort(); let _ = server.await; panic!("intake fault exceeded bounded staff deadline"); }
            let response = response.unwrap();
            let joined = tokio::time::timeout(Duration::from_secs(3), &mut server).await;
            if joined.is_err() { server.abort(); let _ = server.await; }
            joined.expect("fault server was detached").unwrap();
            assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
            if fault == "timeout" { assert!(started.elapsed() >= Duration::from_secs(17)); }
            let page = String::from_utf8(to_bytes(response.into_body(), 262_144).await.unwrap().to_vec()).unwrap();
            assert!(!page.contains(&id)); assert!(!page.contains(&cap));
            assert!(fixture.intake.status(&id, &cap).await.is_ok(), "operational failure revoked a valid receipt");
        }
        fixture.state.staff.close().await;
        let response = fixture.send_form("/post/upload/status", fixture.form(&id, &cap)).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE, "closed content pool was misreported as a missing receipt");
    }).catch_unwind().await;
    fixture.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn multipart_control_staging_preserves_split_and_coalesced_file_bytes() {
    let _serial = SERIAL.lock().await;
    let fixture = Arc::new(Fixture::new().await);
    let case = fixture.clone();
    let result = tokio::spawn(async move {
        // One-byte frames split every boundary/header terminator. The large
        // frame coalesces all controls, file headers and file bytes together.
        for chunk_size in [1, 2, 7, 65_536] {
            let bytes = to_bytes(case.multipart(&case.board, "", 256).into_body(), 32_768)
                .await
                .unwrap();
            let chunks: Vec<Result<axum::body::Bytes, std::io::Error>> = bytes
                .chunks(chunk_size)
                .map(|chunk| Ok(axum::body::Bytes::copy_from_slice(chunk)))
                .collect();
            let body = Body::from_stream(futures_util::stream::iter(chunks));
            let response = case
                .app()
                .oneshot(case.request(
                    "/post/upload",
                    "multipart/form-data; boundary=staff-boundary",
                    body,
                ))
                .await
                .unwrap();
            let page = html(response, StatusCode::OK).await;
            let (id, cap) = (
                hidden(&page, "upload_id"),
                hidden(&page, "upload_capability"),
            );
            assert_eq!(
                std::fs::read(case.root.path().join(format!("{id}.input"))).unwrap(),
                vec![42; 256],
                "control staging duplicated, truncated or changed uploaded bytes"
            );
            assert_eq!(
                case.send_form("/post/upload/cancel", case.form(&id, &cap))
                    .await
                    .status(),
                StatusCode::SEE_OTHER
            );
        }
    })
    .await;
    Arc::try_unwrap(fixture).ok().unwrap().cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn malformed_ordered_controls_reject_before_reservation() {
    let _serial = SERIAL.lock().await;
    let fixture = Arc::new(Fixture::new().await);
    let case = fixture.clone();
    let result = tokio::spawn(async move {
        let original = String::from_utf8(
            to_bytes(case.multipart(&case.board, "", 256).into_body(), 32_768)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        let thread_start = original
            .find("--staff-boundary\r\nContent-Disposition: form-data; name=\"thread\"")
            .unwrap();
        let variants = [
            original.replace("name=\"csrf\"", "name=\"board\""),
            original.replace("name=\"board\"", "name=\"csrf\""),
            original.replace("name=\"csrf\"", "name=\"csrf\"; filename=\"forged.txt\""),
            original[..thread_start].to_owned(),
            original.replace(
                "name=\"thread\"\r\n\r\n0\r\n--staff-boundary\r\n",
                "name=\"thread\"\r\n\r\n0\r\n--staff-boundaryX\r\n",
            ),
        ];
        for body in variants {
            let response = case
                .app()
                .oneshot(case.request(
                    "/post/upload",
                    "multipart/form-data; boundary=staff-boundary",
                    Body::from(body),
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            assert_eq!(
                case.jobs().await,
                0,
                "malformed control envelope reserved intake work"
            );
        }
    })
    .await;
    Arc::try_unwrap(fixture).ok().unwrap().cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn attachment_spoiler_request_cannot_override_disabled_board_policy() {
    let _serial = SERIAL.lock().await;
    for ordinary in [false, true] {
        let fixture = Arc::new(Fixture::new().await);
        let case = fixture.clone();
        let result = tokio::spawn(async move {
            // 0028 defaults this policy off;0080 and0110 require it before
            // persisting an attachment spoiler, even for authorized staff.
            sqlx::query("UPDATE content.boards SET comment_spoiler_cleanup=false WHERE slug=$1").bind(&case.board).execute(&case.owner).await.unwrap();
            let (id, cap) = case.upload().await; case.approve(&id).await;
            let extra = if ordinary { "badge=none&password=owned-upload-password" } else { "badge=mod" };
            let response = case.send_form("/post", format!("{}&name=Owned&subject=Owned&comment=Owned+board+spoiler+policy&spoiler=true&{extra}", case.form(&id,&cap))).await;
            assert_eq!(response.status(), StatusCode::SEE_OTHER);
            let saved: bool = sqlx::query_scalar("SELECT m.spoiler FROM content.posts p JOIN content.post_media m ON m.post_id=p.id WHERE p.board=$1").bind(&case.board).fetch_one(&case.owner).await.unwrap();
            assert!(!saved, "staff attachment overrode the board's disabled spoiler policy");
        }).await;
        Arc::try_unwrap(fixture).ok().unwrap().cleanup().await;
        result.unwrap();
    }
}

#[tokio::test]
async fn real_intake_receives_bytes_before_ready_source_frames_are_exhausted() {
    use futures_util::StreamExt;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let _serial = SERIAL.lock().await;
    let fixture = Arc::new(Fixture::new().await);
    let case = fixture.clone();
    let result = tokio::spawn(async move {
        let file_size = 4 * 1024 * 1024;
        let bytes = to_bytes(
            case.multipart(&case.board, "", file_size).into_body(),
            file_size + 32_768,
        )
        .await
        .unwrap();
        let chunks: Vec<Result<axum::body::Bytes, std::io::Error>> = bytes
            .chunks(4096)
            .map(|chunk| Ok(axum::body::Bytes::copy_from_slice(chunk)))
            .collect();
        let total = chunks.len();
        let consumed = Arc::new(AtomicUsize::new(0));
        let observed_at = Arc::new(AtomicUsize::new(usize::MAX));
        let root = case.root.path().to_owned();
        // Every source frame is immediately ready. Do not add a producer sleep
        // or gate: that would hide a parser that eagerly drains the full input.
        let stream = futures_util::stream::iter(chunks).inspect({
            let consumed = consumed.clone();
            let observed_at = observed_at.clone();
            move |_| {
                let index = consumed.fetch_add(1, Ordering::SeqCst);
                let received = std::fs::read_dir(&root).unwrap().any(|entry| {
                    let entry = entry.unwrap();
                    entry
                        .path()
                        .extension()
                        .is_some_and(|extension| extension == "part")
                        && entry.metadata().unwrap().len() > 0
                });
                if received {
                    observed_at.fetch_min(index, Ordering::SeqCst);
                }
            }
        });
        let response = case
            .app()
            .oneshot(case.request(
                "/post/upload",
                "multipart/form-data; boundary=staff-boundary",
                Body::from_stream(stream),
            ))
            .await
            .unwrap();
        let page = html(response, StatusCode::OK).await;
        assert!(
            observed_at.load(Ordering::SeqCst) < total.saturating_sub(1),
            "all ready source frames were polled before real intake received any bytes"
        );
        assert_eq!(consumed.load(Ordering::SeqCst), total);
        let (id, cap) = (
            hidden(&page, "upload_id"),
            hidden(&page, "upload_capability"),
        );
        assert_eq!(
            std::fs::read(case.root.path().join(format!("{id}.input"))).unwrap(),
            vec![42; file_size]
        );
        assert_eq!(
            case.send_form("/post/upload/cancel", case.form(&id, &cap))
                .await
                .status(),
            StatusCode::SEE_OTHER
        );
    })
    .await;
    Arc::try_unwrap(fixture).ok().unwrap().cleanup().await;
    result.unwrap();
}
