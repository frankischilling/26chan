#![cfg(feature = "database-tests")]
use serde_json::Value;
use sqlx::PgPool;
use std::time::Duration;

struct Event {
    id: i64,
    board: String,
    prefix: [u8; 32],
}
async fn refresh(
    pool: &PgPool,
    event: &Event,
    token: &[u8],
    minted: bool,
    now: i64,
) -> Result<bool, sqlx::Error> {
    sqlx::query_scalar(
        "SELECT content.refresh_automatic_range_cookie($1,$2,0,$3,'101020304',$4,$5,$6,$7,$8,$9)",
    )
    .bind(event.id)
    .bind(&event.board)
    .bind(event.prefix.as_slice())
    .bind(token)
    .bind([1_u8; 32].as_slice())
    .bind([2_u8; 32].as_slice())
    .bind([3_u8; 32].as_slice())
    .bind(minted)
    .bind(now)
    .fetch_one(pool)
    .await
}
async fn state(pool: &PgPool, token: &[u8]) -> Value {
    sqlx::query_scalar(
        "SELECT to_jsonb(s) FROM post_secrets.anonymous_sessions s WHERE token_hash=$1",
    )
    .bind(token)
    .fetch_one(pool)
    .await
    .unwrap()
}
fn code(error: &sqlx::Error) -> Option<&str> {
    error
        .as_database_error()
        .and_then(|error| error.code())
        .map(|value| match value {
            std::borrow::Cow::Borrowed(value) => value,
            std::borrow::Cow::Owned(_) => panic!("Expected PostgreSQL's borrowed SQLSTATE"),
        })
}

#[tokio::test]
async fn range_cookie_refresh_is_scoped_and_does_not_register_an_action() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = PgPool::connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT 'rc'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Range cookie','Synthetic',1000,100,100,100,10)")
        .bind(&board).execute(&owner).await.unwrap();
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT content.lock_automatic_range_policy(true)")
        .execute(&mut *tx)
        .await
        .unwrap();
    // Synthetic private event only; production still needs a qualified issuer.
    let id:i64=sqlx::query_scalar("INSERT INTO restrictions.automatic_ranges(board,parent,prefix,browser,created_at,source_ban_id) VALUES($1,0,$2,'101020304',floor(extract(epoch FROM clock_timestamp()))::bigint,1) RETURNING id")
        .bind(&board).bind([42_u8;32].as_slice()).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let mut event = Event {
        id,
        board,
        prefix: [42; 32],
    };
    let now: i64 =
        sqlx::query_scalar("SELECT floor(extract(epoch FROM clock_timestamp()))::bigint")
            .fetch_one(&owner)
            .await
            .unwrap();
    let token = [21_u8; 32];
    event.prefix = [43; 32];
    assert!(!refresh(&public, &event, &token, true, now).await.unwrap());
    let absent: bool = sqlx::query_scalar(
        "SELECT NOT EXISTS(SELECT 1 FROM post_secrets.anonymous_sessions WHERE token_hash=$1)",
    )
    .bind(token.as_slice())
    .fetch_one(&owner)
    .await
    .unwrap();
    assert!(absent);
    event.prefix = [42; 32];
    assert!(refresh(&public, &event, &token, true, now).await.unwrap());
    let fresh = state(&owner, &token).await;
    for field in ["created_at", "network_at", "address_at", "environment_at"] {
        assert_eq!(fresh[field], now);
    }
    for field in [
        "activity_at",
        "action_at",
        "posts",
        "images",
        "threads",
        "reports",
        "pending",
        "change_score",
        "verified_level",
    ] {
        assert_eq!(fresh[field], 0);
    }
    assert_eq!(fresh["expires_at"], now + 31536000);
    assert_eq!(
        code(
            &refresh(&public, &event, &token, true, now)
                .await
                .unwrap_err()
        ),
        Some("28000")
    );
    assert_eq!(
        code(
            &refresh(&public, &event, &[22; 32], false, now)
                .await
                .unwrap_err()
        ),
        Some("28000")
    );
    assert_eq!(
        code(
            &refresh(&public, &event, &token, false, now - 120)
                .await
                .unwrap_err()
        ),
        Some("23514")
    );
    assert_eq!(
        code(
            &refresh(&public, &event, &token, false, now + 120)
                .await
                .unwrap_err()
        ),
        Some("23514")
    );
    // Existing state resumes changed fingerprints without advancing activity.
    sqlx::query("UPDATE post_secrets.anonymous_sessions SET created_at=$2-3600,network_at=$2-3600,address_at=$2-3600,environment_at=$2-3600,activity_at=$2-100,action_at=$2-200,posts=5,images=4,threads=3,reports=2,pending=9,change_score=7,verified_level=2,network_hash=$3,address_hash=$3,environment_hash=$3,expires_at=$2+60 WHERE token_hash=$1")
        .bind(token.as_slice()).bind(now).bind([4_u8;32].as_slice()).execute(&owner).await.unwrap();
    assert!(refresh(&public, &event, &token, false, now).await.unwrap());
    let resumed = state(&owner, &token).await;
    assert_eq!(resumed["created_at"], now - 3600);
    assert_eq!(resumed["activity_at"], now - 100);
    assert_eq!(resumed["action_at"], now - 200);
    for (field, value) in [
        ("posts", 5),
        ("images", 4),
        ("threads", 3),
        ("reports", 2),
        ("pending", 9),
        ("change_score", 7),
        ("verified_level", 2),
    ] {
        assert_eq!(resumed[field], value);
    }
    for field in ["network_at", "address_at", "environment_at"] {
        assert_eq!(resumed[field], now);
    }
    assert_eq!(resumed["network_hash"], fresh["network_hash"]);
    assert_eq!(resumed["address_hash"], fresh["address_hash"]);
    assert_eq!(resumed["environment_hash"], fresh["environment_hash"]);
    assert_eq!(resumed["expires_at"], now + 31536000);
    // The idle boundary is inclusive, and renewal does not create a post/report.
    sqlx::query("UPDATE post_secrets.anonymous_sessions SET created_at=$2-604799,activity_at=0 WHERE token_hash=$1")
        .bind(token.as_slice()).bind(now).execute(&owner).await.unwrap();
    assert!(refresh(&public, &event, &token, false, now).await.unwrap());
    assert_eq!(state(&owner, &token).await["posts"], 5);
    sqlx::query(
        "UPDATE post_secrets.anonymous_sessions SET created_at=$2-604800 WHERE token_hash=$1",
    )
    .bind(token.as_slice())
    .bind(now)
    .execute(&owner)
    .await
    .unwrap();
    assert!(refresh(&public, &event, &token, false, now).await.unwrap());
    let reset = state(&owner, &token).await;
    for field in [
        "created_at",
        "network_at",
        "address_at",
        "environment_at",
        "action_at",
    ] {
        assert_eq!(reset[field], now);
    }
    for field in [
        "activity_at",
        "posts",
        "images",
        "threads",
        "reports",
        "pending",
        "change_score",
        "verified_level",
    ] {
        assert_eq!(reset[field], 0);
    }
    let memberships:i64=sqlx::query_scalar("SELECT (SELECT count(*) FROM post_secrets.anonymous_posts WHERE token_hash=$1)+(SELECT count(*) FROM post_secrets.anonymous_reports WHERE token_hash=$1)")
        .bind(token.as_slice()).fetch_one(&owner).await.unwrap();
    assert_eq!(memberships, 0);
    // An expiry after the request clock, reached while waiting for its row,
    // must not be revived by checking only the earlier request time.
    let now: i64 =
        sqlx::query_scalar("SELECT floor(extract(epoch FROM clock_timestamp()))::bigint")
            .fetch_one(&owner)
            .await
            .unwrap();
    let mut held = owner.begin().await.unwrap();
    sqlx::query(
        "SELECT token_hash FROM post_secrets.anonymous_sessions WHERE token_hash=$1 FOR UPDATE",
    )
    .bind(token.as_slice())
    .execute(&mut *held)
    .await
    .unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *held)
        .await
        .unwrap();
    let mut connection = public.acquire().await.unwrap();
    let waiting: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *connection)
        .await
        .unwrap();
    sqlx::raw_sql("SET lock_timeout='5s'; SET statement_timeout='10s'")
        .execute(&mut *connection)
        .await
        .unwrap();
    let board = event.board.clone();
    let waiter = tokio::spawn(async move {
        let result = sqlx::query_scalar::<_,bool>("SELECT content.refresh_automatic_range_cookie($1,$2,0,$3,'101020304',$4,$5,$6,$7,false,$8)")
            .bind(id).bind(board).bind([42_u8;32].as_slice()).bind(token.as_slice())
            .bind([1_u8;32].as_slice()).bind([2_u8;32].as_slice()).bind([3_u8;32].as_slice()).bind(now)
            .fetch_one(&mut *connection).await;
        connection.close().await.unwrap();
        result
    });
    tokio::time::timeout(Duration::from_secs(5),async {
        loop {
            let blocked:bool=sqlx::query_scalar("SELECT $1=ANY(pg_blocking_pids($2)) AND floor(extract(epoch FROM clock_timestamp()))::bigint>$3+1")
                .bind(blocker).bind(waiting).bind(now).fetch_one(&owner).await.unwrap();
            if blocked {break;}
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }).await.unwrap();
    assert!(!waiter.is_finished());
    sqlx::query("UPDATE post_secrets.anonymous_sessions SET expires_at=$2+1 WHERE token_hash=$1")
        .bind(token.as_slice())
        .bind(now)
        .execute(&mut *held)
        .await
        .unwrap();
    held.commit().await.unwrap();
    let error = tokio::time::timeout(Duration::from_secs(5), waiter)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(code(&error), Some("28000"));
    assert_eq!(state(&owner, &token).await["expires_at"], now + 1);
    assert!(
        sqlx::query("SELECT post_secrets.resume_anonymous_refusal($1,$1,$1,$1,true,$2)")
            .bind([22_u8; 32].as_slice())
            .bind(now)
            .execute(&public)
            .await
            .is_err()
    );
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("ALTER FUNCTION post_secrets.resume_anonymous_refusal(bytea,bytea,bytea,bytea,boolean,bigint) SECURITY INVOKER").execute(&mut *tx).await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let ready: bool = sqlx::query_scalar("SELECT content.restrictions_ready()")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert!(!ready);
    tx.rollback().await.unwrap();
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT content.lock_automatic_range_policy(true)")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM restrictions.automatic_ranges WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(
        !refresh(&public, &event, &[22; 32], true, now)
            .await
            .unwrap()
    );
    sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
        .bind(token.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.boards WHERE slug=$1")
        .bind(&event.board)
        .execute(&owner)
        .await
        .unwrap();
}
