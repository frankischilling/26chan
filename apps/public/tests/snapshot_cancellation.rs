#![cfg(feature = "database-tests")]

// Controlled local recovery probes, not evidence that an aborted browser fetch
// caused the hosted cleanup failure. Run this binary alone on a disposable DB:
// its content.posts barrier must not overlap other database/browser tests.
#[path = "support/posting.rs"]
mod posting;

use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use axum::{body::Body, http::Request, response::Response};
use board_store::{NewPost, StoreError};
use http_body_util::BodyExt;
use rand_core::OsRng;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{future::Future, time::Duration};
use tower::ServiceExt;

const ORIGIN: &str = "http://127.0.0.1:3000";
const PASSWORD: &str = "owned-snapshot-cancellation-password";
const BOUND: Duration = Duration::from_secs(5);
type ProbeResult<T = ()> = Result<T, &'static str>;

// Also abort the owned reader if the enclosing exercise hits its outer bound.
// Dropping its SQLx transaction queues rollback; pool cleanup below is bounded.
struct AbortReaderOnDrop(tokio::task::AbortHandle);
impl Drop for AbortReaderOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

// Never format SQLx errors, connection URLs, bound data or response bodies.
async fn checked<T, E>(
    stage: &'static str,
    operation: impl Future<Output = Result<T, E>>,
) -> ProbeResult<T> {
    tokio::time::timeout(BOUND, operation)
        .await
        .map_err(|_| stage)?
        .map_err(|_| stage)
}

async fn request(app: &axum::Router, path: &str, form: Option<String>) -> ProbeResult<Response> {
    let mut builder = Request::builder().uri(path);
    let body = if let Some(form) = form {
        builder = builder
            .method("POST")
            .header("origin", ORIGIN)
            .header("content-type", "application/x-www-form-urlencoded");
        Body::from(form)
    } else {
        Body::empty()
    };
    let request = builder.body(body).map_err(|_| "request construction")?;
    checked("request completion", app.clone().oneshot(request)).await
}

async fn expect_status(response: Response, stage: &'static str, expected: u16) -> ProbeResult {
    // Structured metadata only. The production warning retains its static
    // error_class when this status is the generic storage-error response.
    eprintln!("probe_stage={stage} status={}", response.status().as_u16());
    if response.status().as_u16() != expected {
        return Err(stage);
    }
    if expected == 404
        && response
            .headers()
            .get("cache-control")
            .and_then(|v| v.to_str().ok())
            != Some("no-store")
    {
        return Err("missing response cache policy");
    }
    let body = checked("response body completion", response.into_body().collect()).await?;
    if expected == 404
        && !String::from_utf8_lossy(&body.to_bytes()).contains("Board, thread, or post not found.")
    {
        return Err("missing response contract");
    }
    Ok(())
}

async fn cancelled_snapshot(owner: &PgPool, app: &axum::Router, path: &str) -> ProbeResult {
    let mut blocker = checked("begin blocker", owner.begin()).await?;
    let setup = async {
        checked(
            "blocker lock timeout",
            sqlx::query("SET LOCAL lock_timeout='3s'").execute(&mut *blocker),
        )
        .await?;
        let pid: i32 = checked(
            "blocker identity",
            sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *blocker),
        )
        .await?;
        checked(
            "acquire snapshot barrier",
            sqlx::query("LOCK TABLE content.posts IN ACCESS EXCLUSIVE MODE").execute(&mut *blocker),
        )
        .await?;
        Ok::<_, &'static str>(pid)
    }
    .await;
    let blocker_pid = match setup {
        Ok(pid) => pid,
        Err(error) => {
            checked("release failed barrier setup", blocker.rollback()).await?;
            return Err(error);
        }
    };
    let read_app = app.clone();
    let read_path = path.to_owned();
    // No wrapper request deadline here: this task is cancelled only after the
    // owner observer proves that it reached the intended SQL statement.
    let mut reader = tokio::spawn(async move {
        read_app
            .oneshot(
                Request::get(read_path)
                    .body(Body::empty())
                    .expect("valid owned request"),
            )
            .await
    });
    let _abort_reader = AbortReaderOnDrop(reader.abort_handle());
    let reached = tokio::time::timeout(BOUND, async {
        loop {
            if reader.is_finished() {
                return Err("snapshot completed before barrier");
            }
            let waiting: i64 = checked(
                "observe snapshot barrier",
                sqlx::query_scalar("SELECT count(*) FROM pg_locks l JOIN pg_stat_activity a USING(pid) WHERE a.usename='board_public' AND l.relation='content.posts'::regclass AND NOT l.granted AND $1=ANY(pg_blocking_pids(l.pid))")
                    .bind(blocker_pid).fetch_one(owner),
            ).await?;
            if waiting == 1 {
                return Ok(());
            }
            if waiting > 1 {
                return Err("unexpected concurrent public reader");
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await.map_err(|_| "snapshot barrier deadline").and_then(|result| result);

    // Always abort/join and release the blocker, even when observation failed.
    // Do not wait for SQLx's discarded query/rollback while retaining its lock.
    reader.abort();
    let cancelled = tokio::time::timeout(BOUND, &mut reader).await;
    let released = checked("release snapshot barrier", blocker.rollback()).await;
    reached?;
    released?;
    match cancelled {
        Ok(Err(error)) if error.is_cancelled() => Ok(()),
        _ => Err("snapshot task cancellation not established"),
    }
}

#[derive(Clone, Copy, Debug)]
enum Case {
    Healthy,
    CancelSingle,
    CancelProduction,
    ReadLimit,
}

async fn exercise(owner: PgPool, public: PgPool, board: String, case: Case) -> ProbeResult {
    let hash = Argon2::default()
        .hash_password(PASSWORD.as_bytes(), &SaltString::generate(&mut OsRng))
        .map_err(|_| "fixture password hash")?
        .to_string();
    let id = checked(
        "fixture posting",
        posting::create_post(
            &public,
            &board,
            0,
            &NewPost {
                name: "Anonymous".into(),
                subject: "Owned cancellation probe".into(),
                comment: "Nonempty owned snapshot body".into(),
                deletion_hash: hash,
                sage: false,
            },
        ),
    )
    .await?;
    let app = posting::routers(public.clone(), &board, ORIGIN.into(), false).0;
    let watcher = format!("/_watch/{board}/thread/{id}.json");
    expect_status(
        request(&app, &watcher, None).await?,
        "initial_snapshot",
        200,
    )
    .await?;
    match case {
        Case::Healthy => {}
        Case::CancelSingle | Case::CancelProduction => {
            cancelled_snapshot(&owner, &app, &watcher).await?;
        }
        Case::ReadLimit => {
            let result = tokio::time::timeout(
                BOUND,
                board_store::thread_snapshot_selection_bounded(&public, &board, id, false, 1),
            )
            .await
            .map_err(|_| "read-limit deadline")?;
            if !matches!(result, Err(StoreError::ReadLimit)) {
                return Err("read-limit early return not established");
            }
            eprintln!("probe_stage=early_return error_class=read_limit");
        }
    }
    // Keep these adjacent: no health query, pool draining, reset, retry or
    // recovery sleep between cancellation/early return and the 303 -> 404 pair.
    let deleted = request(
        &app,
        &format!("/{board}/delete"),
        Some(format!("no={id}&password={PASSWORD}")),
    )
    .await?;
    expect_status(deleted, "password_delete", 303).await?;
    let missing = request(&app, &format!("/{board}/thread/{id}.json"), None).await?;
    expect_status(missing, "first_missing_json", 404).await?;
    expect_status(request(&app, &watcher, None).await?, "missing_watcher", 404).await?;
    expect_status(
        request(&app, &format!("/{board}/thread/{id}"), None).await?,
        "missing_html",
        404,
    )
    .await?;
    Ok(())
}

async fn run_case(owner: &PgPool, public_url: &str, case: Case) -> ProbeResult {
    eprintln!("probe_case={case:?}");
    let public = if matches!(case, Case::CancelProduction) {
        checked(
            "production public pool",
            board_store::connect_public(public_url),
        )
        .await?
    } else {
        checked(
            "single public pool",
            PgPoolOptions::new()
                .max_connections(1)
                .acquire_timeout(Duration::from_secs(3))
                .connect(public_url),
        )
        .await?
    };
    let role: String = checked(
        "runtime role",
        sqlx::query_scalar("SELECT current_user::text").fetch_one(&public),
    )
    .await?;
    if role != "board_public" {
        return Err("unexpected runtime role");
    }
    let seed: i64 = checked(
        "owned fixture seed",
        sqlx::query_scalar("SELECT nextval('content.post_number')").fetch_one(owner),
    )
    .await?;
    let board = format!("sc{seed:x}");
    checked("owned board setup", sqlx::query("INSERT INTO content.boards(posting_reply_seconds,posting_image_seconds,posting_thread_seconds,slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,deletion_known_min_seconds,deletion_unknown_min_seconds) VALUES(0,0,0,$1,'Snapshot cancellation','Owned diagnostic fixture',200,100,100,100,10,0,0)")
        .bind(&board).execute(owner)).await?;

    // Catch assertion failures in existing fixture helpers, then clean up only
    // this case's rows. Normal completion explicitly releases the table lock;
    // outer cancellation drops it and lets the owner pool finish rollback.
    let mut work = tokio::spawn(exercise(owner.clone(), public.clone(), board.clone(), case));
    let outcome = match tokio::time::timeout(Duration::from_secs(90), &mut work).await {
        Ok(result) => result
            .map_err(|_| "fixture task failed")
            .and_then(|result| result),
        Err(_) => {
            work.abort();
            let joined = tokio::time::timeout(BOUND, &mut work).await;
            if joined.is_err() {
                return Err("fixture task cleanup deadline; discard disposable database");
            }
            Err("whole exercise deadline")
        }
    };
    if let Err(stage) = &outcome {
        eprintln!("probe_failure_stage={stage}");
    }
    // If draining cannot finish, stop this binary before attempting row teardown
    // or another case. The caller must discard this disposable database.
    tokio::time::timeout(BOUND, public.close())
        .await
        .map_err(|_| "public pool cleanup deadline; discard disposable database")?;
    let mut cleanup = checked("begin fixture cleanup", owner.begin()).await?;
    checked(
        "cleanup lock timeout",
        sqlx::query("SET LOCAL lock_timeout='3s'").execute(&mut *cleanup),
    )
    .await?;
    for statement in [
        "DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        checked(
            "owned fixture cleanup",
            sqlx::query(statement).bind(&board).execute(&mut *cleanup),
        )
        .await?;
    }
    let identity = posting::key(&board).public_deletion_rate_identity(posting::peer());
    checked(
        "owned quota cleanup",
        sqlx::query("DELETE FROM post_secrets.public_deletion_actors WHERE actor_hash=$1")
            .bind(identity.as_bytes().as_slice())
            .execute(&mut *cleanup),
    )
    .await?;
    checked("commit fixture cleanup", cleanup.commit()).await?;
    outcome
}

#[tokio::test]
async fn snapshot_cancellation_preserves_password_delete_and_missing_reads() {
    // Only this production module's static diagnostic fields are retained;
    // SQLx messages, queries, connection configuration and bodies are excluded.
    let _ = tracing_subscriber::fmt()
        .with_env_filter("off,board_public::handlers=warn")
        .with_ansi(false)
        .without_time()
        .try_init();
    let owner_url =
        std::env::var("MIGRATION_DATABASE_URL").expect("owned disposable database required");
    let public_url = std::env::var("TEST_PUBLIC_DATABASE_URL").expect("owned public role required");
    let owner = checked(
        "owner connection",
        PgPoolOptions::new()
            .max_connections(3)
            .acquire_timeout(Duration::from_secs(3))
            .connect(&owner_url),
    )
    .await
    .expect("owner setup");
    let result = async {
        for case in [
            Case::Healthy,
            Case::CancelSingle,
            Case::CancelProduction,
            Case::ReadLimit,
        ] {
            run_case(&owner, &public_url, case).await?;
        }
        Ok::<(), &'static str>(())
    }
    .await;
    let closed = tokio::time::timeout(BOUND, owner.close()).await;
    assert!(
        closed.is_ok(),
        "owner pool cleanup deadline; discard disposable database"
    );
    assert!(
        result.is_ok(),
        "snapshot probe failed at {}",
        result.err().unwrap_or("unknown stage")
    );
}
