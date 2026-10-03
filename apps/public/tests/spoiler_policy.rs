#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use board_store::{
    media::MediaQueue,
    media_assets::OutputMetadata,
    media_intake::{IntakeReservation, IntakeStore},
};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

const ORIGIN: &str = "http://127.0.0.1:3000";

#[tokio::test]
async fn spoiler_choice_uses_policy_after_an_observed_board_lock_wait() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT 'sl'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Owned spoiler lock','Synthetic policy wait',2000,100,100,100,10)").bind(&board).execute(&owner).await.unwrap();
    let op = board_store::create_post(
        &public,
        &board,
        0,
        &board_store::NewPost {
            name: "Owned".into(),
            subject: "Owned policy wait".into(),
            comment: "Owned OP".into(),
            deletion_hash: "owned-fixture-hash".into(),
            sage: false,
        },
    )
    .await
    .unwrap();
    let app = board_public::routers(public.clone(), ORIGIN.into(), false).0;
    let owned = owner.clone();
    let runtime = public.clone();
    let run_board = board.clone();
    let outcome=tokio::spawn(async move {
        for old in [false,true] {
            sqlx::query("UPDATE content.boards SET comment_spoiler_cleanup=$2 WHERE slug=$1").bind(&run_board).bind(old).execute(&owned).await.unwrap();
            let mut blocker=owned.begin().await.unwrap();
            let pid:i32=sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *blocker).await.unwrap();
            sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE").bind(&run_board).fetch_one(&mut *blocker).await.unwrap();
            let request=form(&run_board,"imgboard.php",&[("resto",op.to_string()),("pwd","owned-policy-wait-password".into()),("com","Owned policy wait reply".into()),("spoiler","on".into())],true);
            let sender=tokio::spawn(app.clone().oneshot(request));
            tokio::time::timeout(std::time::Duration::from_secs(10),async {
                loop {
                    if sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM pg_stat_activity a WHERE a.datname=current_database() AND pg_blocking_pids(a.pid) @> ARRAY[$1])").bind(pid).fetch_one(&owned).await.unwrap() {break;}
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            }).await.expect("Public post must be observed waiting on the held board row");
            sqlx::query("UPDATE content.boards SET comment_spoiler_cleanup=$2 WHERE slug=$1").bind(&run_board).bind(!old).execute(&mut *blocker).await.unwrap();
            blocker.commit().await.unwrap();
            let response=sender.await.unwrap().unwrap();assert_eq!(response.status(),StatusCode::OK);
            let value:Value=serde_json::from_slice(&to_bytes(response.into_body(),8192).await.unwrap()).unwrap();
            assert!(value.get("error").is_none());let id=value["pid"].as_i64().unwrap();
            assert_eq!(sqlx::query_scalar::<_,bool>("SELECT image_spoiler FROM content.posts WHERE id=$1").bind(id).fetch_one(&runtime).await.unwrap(),!old);
        }
    }).await;
    for sql in [
        "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(sql).bind(&board).execute(&owner).await.unwrap();
    }
    public.close().await;
    owner.close().await;
    outcome.unwrap();
}

async fn get(app: &Router, path: &str) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1_048_576).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}

async fn html(app: &Router, request: Request<Body>) -> String {
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    String::from_utf8(
        to_bytes(response.into_body(), 1_048_576)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

fn form(board: &str, action: &str, fields: &[(&str, String)], native: bool) -> Request<Body> {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(fields.iter().map(|(k, v)| (*k, v.as_str())))
        .finish();
    Request::post(format!("/{board}/{action}"))
        .header("origin", ORIGIN)
        .header(
            "accept",
            if native {
                "application/json"
            } else {
                "text/html"
            },
        )
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(body))
        .unwrap()
}

async fn approved(intake: &IntakeStore, queue: &MediaQueue, filename: &str) -> IntakeReservation {
    let upload = intake.reserve(filename).await.unwrap();
    intake
        .begin_upload(&upload.id, &upload.capability)
        .await
        .unwrap();
    intake
        .finish_upload(&upload.id, &upload.capability, 100)
        .await
        .unwrap();
    // Owned transport fixtures use explicit coordinator approval. This does
    // not qualify decoding or worker isolation.
    let claim = queue.claim().await.unwrap().unwrap();
    assert_eq!(claim.id, upload.id, "Requires an idle disposable queue");
    let token = claim.lease_token.unwrap();
    let output = queue
        .prepare_output(
            &claim.id,
            &token,
            &OutputMetadata {
                sha256: "a".repeat(64),
                bytes: 100,
                width: 1,
                height: 1,
            },
        )
        .await
        .unwrap();
    queue
        .approve_output(&claim.id, &token, &output.id)
        .await
        .unwrap();
    upload
}

#[tokio::test]
async fn public_spoilers_match_source_scalar_policy_and_attachment_independence() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let suffix: String =
        sqlx::query_scalar("SELECT substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    let boards = [format!("s0{suffix}"), format!("s1{suffix}")];
    for (enabled, board) in boards.iter().enumerate() {
        sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit,comment_spoiler_cleanup) VALUES($1,'Owned spoiler policy','Synthetic source comparison',2000,1000,1000,1000,10,1000,$2)")
            .bind(board).bind(enabled==1).execute(&owner).await.unwrap();
    }
    let intake = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let root = tempfile::tempdir().unwrap();
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
    let media = board_config::PublicMediaSettings::development(
        &address.to_string(),
        &"a".repeat(64),
        "http://localhost:3002",
    )
    .unwrap();
    // This matrix shares one transport peer. Default and non-default request
    // budgets are exercised separately by http_limits.rs.
    let limits = board_config::PublicRequestLimits::from_lookup(|name| {
        (name == "PUBLIC_WRITES_PER_MINUTE").then(|| "1000".into())
    })
    .unwrap();
    let (web, api) = board_public::routers_with_limits(
        public.clone(),
        ORIGIN.into(),
        false,
        Some(media),
        limits,
    );
    let run_boards = boards.clone();
    let run_owner = owner.clone();
    let run_public = public.clone();
    let filename = format!("owned-spoiler-{suffix}.png");
    let run_filename = filename.clone();
    let outcome = tokio::spawn(async move {
        let fixture: Value =
            serde_json::from_str(include_str!("fixtures/public-post-spoilers.json")).unwrap();
        let cases = fixture["cases"].as_array().unwrap();
        assert_eq!(cases.len(), 144);
        for case in cases {
            let enabled = case["enabled"].as_bool().unwrap();
            let requested = case["requested"].as_bool().unwrap();
            let attached = case["attachment"].as_bool().unwrap();
            let board = &run_boards[usize::from(enabled)];
            let mut fields = vec![
                ("mode", "regist".into()),
                ("pwd", "owned-spoiler-password".into()),
                ("sub", case["subject"].as_str().unwrap().into()),
                ("com", "Owned synthetic spoiler body".into()),
            ];
            if let Some(value) = case["raw_flag"].as_str() {
                fields.push(("spoiler", value.into()));
            }
            if attached {
                let receipt = approved(&intake, &queue, &run_filename).await;
                let receipt_fields = [
                    ("upload_id", receipt.id.clone()),
                    ("upload_capability", receipt.capability.clone()),
                    ("resto", "0".into()),
                ];
                let page = html(&web, form(board, "upload/status", &receipt_fields, false)).await;
                assert_eq!(page.contains("name=\"spoiler\""), enabled);
                assert!(page.contains(&format!("data-spoilers=\"{enabled}\"")));
                fields.extend(receipt_fields);
            }
            let response = web
                .clone()
                .oneshot(form(board, "imgboard.php", &fields, true))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            let result: Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap())
                    .unwrap();
            assert!(
                result.get("error").is_none(),
                "source scalar request must be accepted"
            );
            let id = result["pid"].as_i64().unwrap();
            let saved: (bool, String) = sqlx::query_as(
                "SELECT image_spoiler,subject FROM content.posts WHERE board=$1 AND id=$2",
            )
            .bind(board)
            .bind(id)
            .fetch_one(&run_public)
            .await
            .unwrap();
            assert_eq!(
                saved,
                (
                    enabled && requested,
                    case["subject"].as_str().unwrap().into()
                )
            );
            if attached {
                let saved = board_store::post_media::attachment(&run_public, id)
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(saved.spoiler, enabled && requested);
            }
            let path = format!("/{board}/thread/{id}.json");
            for app in [&web, &api] {
                let (status, body) = get(app, &path).await;
                assert_eq!(status, StatusCode::OK);
                let projected = &body["posts"][0];
                assert_eq!(
                    projected.get("spoiler"),
                    case["json_spoiler"].as_i64().map(|_| &case["json_spoiler"])
                );
                assert_eq!(
                    projected.get("sub").and_then(Value::as_str).unwrap_or(""),
                    case["json_subject"].as_str().unwrap()
                );
                assert_eq!(projected.get("tim").is_some(), attached);
            }
            let page = html(
                &web,
                Request::get(format!("/{board}/thread/{id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await;
            assert!(page.contains(&format!("data-spoilers=\"{enabled}\"")));
            if attached {
                board_store::post_media::delete_attachment(&run_public, board, id)
                    .await
                    .unwrap();
                let (_, body) = get(&web, &path).await;
                assert_eq!(body["posts"][0]["filedeleted"], 1);
                assert_eq!(
                    body["posts"][0].get("spoiler"),
                    case["json_spoiler"].as_i64().map(|_| &case["json_spoiler"])
                );
                assert!(body["posts"][0].get("tim").is_none());
            }
        }
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM content.posts WHERE board=ANY($1)")
                .bind(&run_boards)
                .fetch_one(&run_owner)
                .await
                .unwrap(),
            144
        );
    })
    .await;
    let _ = stop.send(());
    server.await.unwrap();
    for board in &boards {
        for sql in [
            "DELETE FROM content.post_media WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
            "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
            "DELETE FROM content.posts WHERE board=$1",
            "DELETE FROM content.threads WHERE board=$1",
            "DELETE FROM content.boards WHERE slug=$1",
        ] {
            sqlx::query(sql).bind(board).execute(&owner).await.unwrap();
        }
    }
    sqlx::query(
        "DELETE FROM media.assets WHERE job_id IN(SELECT id FROM media.jobs WHERE filename=$1)",
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
