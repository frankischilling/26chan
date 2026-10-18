#![cfg(feature = "database-tests")]
use sqlx::PgPool;
const ISSUE: &str = "SELECT content.record_country_classifier_refusal($1,$2,$2,$2,$3,$4,$5)";

async fn now(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT floor(extract(epoch FROM clock_timestamp()))::bigint")
        .fetch_one(pool)
        .await
        .unwrap()
}
async fn issue(
    pool: &PgPool,
    board: &str,
    token: &[u8],
    environment: &[u8],
    minted: bool,
    at: i64,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(ISSUE)
        .bind(board)
        .bind(token)
        .bind(environment)
        .bind(minted)
        .bind(at)
        .fetch_one(pool)
        .await
}
fn assert_code(error: sqlx::Error, expected: &str) {
    assert_eq!(
        error.as_database_error().and_then(|e| e.code()).as_deref(),
        Some(expected)
    );
}
#[tokio::test]
async fn country_refusal_requires_a_live_eligible_changed_session() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = PgPool::connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let token: Vec<u8> =
        sqlx::query_scalar("SELECT sha256(convert_to(gen_random_uuid()::text,'UTF8'))")
            .fetch_one(&owner)
            .await
            .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT 'cc'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,captcha_enabled) VALUES($1,'Country refusal','Owned fixture',1000,100,100,100,10,true)").bind(&board).execute(&owner).await.unwrap();
    let n = now(&owner).await;
    sqlx::query("INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,created_at,network_at,address_at,environment_at,activity_at,expires_at,posts,pending) VALUES($1,$1,$1,$1,$2-3600,$2-3600,$2-3600,$2-3600,$2-60,$2+3600,19,2)").bind(&token).bind(n).execute(&owner).await.unwrap();
    let original: serde_json::Value = sqlx::query_scalar(
        "SELECT to_jsonb(s) FROM post_secrets.anonymous_sessions s WHERE token_hash=$1",
    )
    .bind(&token)
    .fetch_one(&owner)
    .await
    .unwrap();
    let changed = [9_u8; 32];
    assert_code(
        issue(&public, &board, &token, &token, false, n)
            .await
            .unwrap_err(),
        "28000",
    );
    assert_code(
        issue(&public, &board, &token, &changed, false, n - 31)
            .await
            .unwrap_err(),
        "23514",
    );
    let before: serde_json::Value = sqlx::query_scalar(
        "SELECT to_jsonb(s) FROM post_secrets.anonymous_sessions s WHERE token_hash=$1",
    )
    .bind(&token)
    .fetch_one(&owner)
    .await
    .unwrap();
    assert_eq!(
        original, before,
        "Denied issuer must roll back resumed state"
    );
    for (verified, posts, pending) in [(1_i16, 0_i16, 0_i16), (0, 20, 0), (0, 19, 1)] {
        sqlx::query("UPDATE post_secrets.anonymous_sessions SET verified_level=$2,posts=$3,pending=$4 WHERE token_hash=$1")
            .bind(&token).bind(verified).bind(posts).bind(pending).execute(&owner).await.unwrap();
        assert_code(
            issue(&public, &board, &token, &changed, false, now(&owner).await)
                .await
                .unwrap_err(),
            "28000",
        );
        sqlx::query("UPDATE post_secrets.anonymous_sessions SET verified_level=0,posts=19,pending=2 WHERE token_hash=$1").bind(&token).execute(&owner).await.unwrap();
    }
    sqlx::query("UPDATE content.boards SET captcha_enabled=false WHERE slug=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    assert_code(
        issue(&public, &board, &token, &changed, false, now(&owner).await)
            .await
            .unwrap_err(),
        "28000",
    );
    sqlx::query("UPDATE content.boards SET captcha_enabled=true WHERE slug=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    assert!(
        issue(&public, &board, &token, &changed, false, now(&owner).await)
            .await
            .unwrap()
    );
    let event: i64 = sqlx::query_scalar("SELECT created_at FROM post_secrets.classifier_history WHERE token_hash=$1 AND kind='country_changed'").bind(&token).fetch_one(&owner).await.unwrap();
    assert!(
        issue(&public, &board, &token, &changed, false, now(&owner).await)
            .await
            .unwrap()
    );
    let capability: [u8; 32] = token.as_slice().try_into().unwrap();
    let session = board_store::anonymous_session::PostingSession {
        fingerprints: board_domain::anonymous_session::Fingerprints {
            token: capability,
            network: capability,
            address: capability,
            environment: changed,
        },
        minted: false,
        now: chrono::DateTime::from_timestamp(now(&owner).await, 0).unwrap(),
    };
    board_store::posting_classifiers::record_country_refusal(&public, &board, &session)
        .await
        .unwrap();
    let mut connection = public.acquire().await.unwrap();
    let observed = board_store::posting_classifiers::history(&mut connection, &board, &session)
        .await
        .unwrap();
    assert!(!observed.edge_blocked && observed.country_blocked);
    drop(connection);
    let posts: i64 = sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1")
        .bind(&board)
        .fetch_one(&owner)
        .await
        .unwrap();
    assert_eq!(posts, 0, "Refusal recorder left a public post");
    let repeated: i64 = sqlx::query_scalar("SELECT created_at FROM post_secrets.classifier_history WHERE token_hash=$1 AND kind='country_changed'").bind(&token).fetch_one(&owner).await.unwrap();
    assert_eq!(
        event, repeated,
        "Repeat refusal must not extend the source cooldown"
    );
    let (edge, country): (bool, bool) =
        sqlx::query_as("SELECT * FROM content.posting_classifier_history($1,$2,false)")
            .bind(&board)
            .bind(&token)
            .fetch_one(&public)
            .await
            .unwrap();
    assert!(!edge && country);
    let current: serde_json::Value = sqlx::query_scalar(
        "SELECT to_jsonb(s) FROM post_secrets.anonymous_sessions s WHERE token_hash=$1",
    )
    .bind(&token)
    .fetch_one(&owner)
    .await
    .unwrap();
    for field in [
        "created_at",
        "network_at",
        "address_at",
        "posts",
        "images",
        "threads",
        "reports",
        "pending",
        "activity_at",
        "action_at",
        "verified_level",
    ] {
        assert_eq!(current[field], original[field], "Refusal advanced {field}");
    }
    let fresh: Vec<u8> =
        sqlx::query_scalar("SELECT sha256(convert_to(gen_random_uuid()::text,'UTF8'))")
            .fetch_one(&owner)
            .await
            .unwrap();
    assert_code(
        issue(&public, &board, &fresh, &changed, true, now(&owner).await)
            .await
            .unwrap_err(),
        "28000",
    );
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM post_secrets.anonymous_sessions WHERE token_hash=$1)",
    )
    .bind(&fresh)
    .fetch_one(&owner)
    .await
    .unwrap();
    assert!(!exists, "Unjustified fresh refusal left a session");
    // An older request waiting behind a newer committed environment update
    // must not restore its stale fingerprint or timestamps.
    let old_at = now(&owner).await;
    let mut held = owner.begin().await.unwrap();
    sqlx::query(
        "SELECT token_hash FROM post_secrets.anonymous_sessions WHERE token_hash=$1 FOR UPDATE",
    )
    .bind(&token)
    .execute(&mut *held)
    .await
    .unwrap();
    let mut connection = public.acquire().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    let worker_board = board.clone();
    let worker_token = token.clone();
    let worker = tokio::spawn(async move {
        sqlx::query_scalar::<_, bool>(ISSUE)
            .bind(worker_board)
            .bind(worker_token)
            .bind([10_u8; 32].as_slice())
            .bind(false)
            .bind(old_at)
            .fetch_one(&mut *connection)
            .await
    });
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(5);
    let mut blocked = false;
    while tokio::time::Instant::now() < deadline {
        blocked = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM pg_locks WHERE pid=$1 AND NOT granted)",
        )
        .bind(pid)
        .fetch_one(&owner)
        .await
        .unwrap();
        if blocked {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
    let newer_at = now(&owner).await;
    sqlx::query("UPDATE post_secrets.anonymous_sessions SET environment_hash=$2,environment_at=$3 WHERE token_hash=$1")
        .bind(&token).bind([11_u8;32].as_slice()).bind(newer_at).execute(&mut *held).await.unwrap();
    held.commit().await.unwrap();
    let stale = tokio::time::timeout(std::time::Duration::from_secs(5), worker)
        .await
        .unwrap()
        .unwrap();
    assert!(blocked, "Older issuer never reached the session lock wait");
    assert!(newer_at > old_at);
    assert_code(stale.unwrap_err(), "28000");
    let (environment, at): (Vec<u8>,i64) = sqlx::query_as("SELECT environment_hash,environment_at FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
        .bind(&token).fetch_one(&owner).await.unwrap();
    assert_eq!(environment, [11_u8; 32]);
    assert_eq!(at, newer_at);
    sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
        .bind(&token)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.boards WHERE slug=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    public.close().await;
    owner.close().await;
}
