#![cfg(feature = "database-tests")]

#[path = "support/posting.rs"]
mod posting;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    response::Response,
};
use board_store::{media::MediaQueue, media_assets::OutputMetadata, media_intake::IntakeStore};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;

const ORIGIN: &str = "http://127.0.0.1:3000";
const ERROR: &str = "Error: You may not post more than 2 active threads at a time.";

async fn submit(
    app: &Router,
    board: &str,
    parent: i64,
    legacy: bool,
    json_response: bool,
    upload: Option<&board_store::media_intake::IntakeReservation>,
) -> Response {
    let mut fields = vec![
        ("mode", "regist".to_owned()),
        ("resto", parent.to_string()),
        ("name", "Owned quota author".to_owned()),
        ("sub", "Owned quota draft".to_owned()),
        ("com", "Owned quota comment <preserved>".to_owned()),
        // Exercise production password validation and hashing unchanged.
        ("pwd", "owned-quota-password".to_owned()),
    ];
    if let Some(upload) = upload {
        fields.push(("upload_id", upload.id.clone()));
        fields.push(("upload_capability", upload.capability.clone()));
    }
    let (content_type, body) = if legacy {
        let mut body = String::new();
        for (name, value) in &fields {
            body.push_str(&format!("--owned-quota\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"));
        }
        body.push_str("--owned-quota--\r\n");
        ("multipart/form-data; boundary=owned-quota", body)
    } else {
        (
            "application/x-www-form-urlencoded",
            url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(fields.iter().map(|(name, value)| (*name, value)))
                .finish(),
        )
    };
    let route = if legacy { "imgboard.php" } else { "post" };
    app.clone()
        .oneshot(
            Request::post(format!("/{board}/{route}"))
                .header("origin", ORIGIN)
                .header(
                    "accept",
                    if json_response {
                        "application/json"
                    } else {
                        "text/html"
                    },
                )
                .header("content-type", content_type)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn accepted(response: Response, parent: i64, json_response: bool) -> i64 {
    if json_response {
        assert_eq!(response.status(), StatusCode::OK);
        let value: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap();
        assert_eq!(value.as_object().unwrap().len(), 2, "{value}");
        assert_eq!(value["tid"], parent);
        value["pid"].as_i64().unwrap()
    } else {
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        response.headers()["location"]
            .to_str()
            .unwrap()
            .split("#p")
            .nth(1)
            .unwrap()
            .parse()
            .unwrap()
    }
}

async fn rejected(response: Response, json_response: bool) {
    assert_eq!(
        response.status(),
        if json_response {
            StatusCode::OK
        } else {
            StatusCode::UNPROCESSABLE_ENTITY
        }
    );
    assert!(!response.headers().contains_key("location"));
    assert!(
        response.headers()["vary"]
            .to_str()
            .unwrap()
            .contains("Accept")
    );
    if json_response {
        assert_eq!(response.headers()["content-type"], "application/json");
    } else {
        assert_eq!(
            response.headers()["content-type"],
            "text/html; charset=utf-8"
        );
    }
    let bytes = to_bytes(response.into_body(), 32_768).await.unwrap();
    if json_response {
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value, json!({"error": ERROR}));
    } else {
        let html = std::str::from_utf8(&bytes).unwrap();
        assert!(html.contains(ERROR), "{html}");
        // The current rule_error handler renders a message-only page, not a
        // redisplayed posting form. Draft retention is not an HTML contract.
    }
}

async fn snapshot(owner: &PgPool, board: &str, filename: &str) -> Value {
    sqlx::query_scalar("SELECT jsonb_build_object('posts',(SELECT jsonb_agg(to_jsonb(p) ORDER BY id) FROM content.posts p WHERE board=$1),'threads',(SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM content.threads t WHERE board=$1),'history',(SELECT jsonb_agg(to_jsonb(h) ORDER BY post_id) FROM post_secrets.posting_history h WHERE board=$1),'actions',(SELECT jsonb_agg(to_jsonb(a) ORDER BY actor_hash) FROM post_secrets.posting_thread_actions a WHERE board=$1),'deletion',(SELECT jsonb_agg(to_jsonb(d) ORDER BY post_id) FROM post_secrets.deletion d WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)),'attachments',(SELECT jsonb_agg(to_jsonb(m) ORDER BY post_id) FROM content.post_media m WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)),'jobs',(SELECT jsonb_agg(to_jsonb(j) ORDER BY id) FROM media.jobs j WHERE filename=$2),'assets',(SELECT jsonb_agg(to_jsonb(a) ORDER BY id) FROM media.assets a WHERE job_id IN(SELECT id FROM media.jobs WHERE filename=$2)),'media_clock',(SELECT last_number FROM content.media_clock WHERE singleton),'post_number',(SELECT last_value FROM content.post_number))")
        .bind(board).bind(filename).fetch_one(owner).await.unwrap()
}

async fn exercise(owner: PgPool, public: PgPool, board: String, filename: String) {
    // Only posting already-approved attachment capabilities needs media enabled;
    // this fixture does not call the intake HTTP endpoint or decode image bytes.
    let media = board_config::PublicMediaSettings::development(
        "127.0.0.1:1",
        &"a".repeat(64),
        "http://localhost:3002",
    )
    .unwrap();
    let app =
        posting::routers_with_media(public.clone(), &board, ORIGIN.into(), false, Some(media)).0;
    let first = accepted(submit(&app, &board, 0, false, true, None).await, 0, true).await;
    let second = accepted(submit(&app, &board, 0, true, false, None).await, 0, false).await;
    assert_ne!(first, second);
    for legacy in [false, true] {
        for json_response in [false, true] {
            let before = snapshot(&owner, &board, &filename).await;
            rejected(
                submit(&app, &board, 0, legacy, json_response, None).await,
                json_response,
            )
            .await;
            assert_eq!(snapshot(&owner, &board, &filename).await, before);
            let reply = accepted(
                submit(&app, &board, first, legacy, json_response, None).await,
                first,
                json_response,
            )
            .await;
            let saved = board_store::find_post(&public, &board, reply)
                .await
                .unwrap();
            assert_eq!(saved.thread_id, first);
            assert_eq!(saved.comment, "Owned quota comment <preserved>");
        }
    }
    let intake = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let upload = intake.reserve(&filename).await.unwrap();
    intake
        .begin_upload(&upload.id, &upload.capability)
        .await
        .unwrap();
    intake
        .finish_upload(&upload.id, &upload.capability, 100)
        .await
        .unwrap();
    let claim = queue
        .claim()
        .await
        .unwrap()
        .expect("owned qualification queue is idle");
    assert_eq!(claim.id, upload.id);
    let lease = claim.lease_token.unwrap();
    let output = queue
        .prepare_output(
            &upload.id,
            &lease,
            &OutputMetadata {
                sha256: "a".repeat(64),
                bytes: 100,
                width: 10,
                height: 10,
            },
        )
        .await
        .unwrap();
    queue
        .approve_output(&upload.id, &lease, &output.id)
        .await
        .unwrap();
    let before = snapshot(&owner, &board, &filename).await;
    rejected(
        submit(&app, &board, 0, false, true, Some(&upload)).await,
        true,
    )
    .await;
    assert_eq!(snapshot(&owner, &board, &filename).await, before);
    // A denied OP must not consume the upload capability. The same trusted
    // actor can publish it as a reply while both active OP slots remain full.
    let reply = accepted(
        submit(&app, &board, first, true, true, Some(&upload)).await,
        first,
        true,
    )
    .await;
    let attachment = board_store::post_media::attachment(&public, reply)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(attachment.asset_id, output.id);
    let counts: (i64, i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM content.threads WHERE board=$1),(SELECT count(*) FROM content.posts WHERE board=$1),(SELECT count(*) FROM post_secrets.posting_history WHERE board=$1)")
        .bind(&board).fetch_one(&owner).await.unwrap();
    assert_eq!(counts, (2, 7, 7));
    intake.close().await.unwrap();
}

#[tokio::test]
async fn active_op_quota_preserves_public_and_legacy_response_contracts_and_atomicity() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let role: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&public)
        .await
        .unwrap();
    assert_eq!(role, "board_public");
    let board: String =
        sqlx::query_scalar("SELECT substr(replace(gen_random_uuid()::text,'-',''),1,10)")
            .fetch_one(&owner)
            .await
            .unwrap();
    let filename = format!("owned-quota-{board}.png");
    // Explicit synthetic policy isolates quota from cooldowns and image limits.
    // Imported source boards and public request/password limits are untouched.
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,user_thread_limit,user_thread_period_hours) VALUES($1,'Owned public quota','Synthetic quota response fixture',2000,100,100,100,10,10,0,0,0,2,24)")
        .bind(&board).execute(&owner).await.unwrap();
    let outcome = tokio::spawn(exercise(
        owner.clone(),
        public.clone(),
        board.clone(),
        filename.clone(),
    ))
    .await;
    public.close().await;
    // Cleanup occurs only after all assertions, including on a task panic.
    // No counted history is cleared to permit a subsequent assertion.
    for statement in [
        "DELETE FROM content.post_media WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
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
    for statement in [
        "DELETE FROM media.assets WHERE job_id IN(SELECT id FROM media.jobs WHERE filename=$1)",
        "DELETE FROM media.jobs WHERE filename=$1",
    ] {
        sqlx::query(statement)
            .bind(&filename)
            .execute(&owner)
            .await
            .unwrap();
    }
    owner.close().await;
    outcome.unwrap();
}
