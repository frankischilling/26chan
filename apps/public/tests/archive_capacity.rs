#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use sqlx::PgPool;
use tower::ServiceExt;

const ORIGIN: &str = "http://127.0.0.1:3000";

async fn response(app: &Router, path: &str) -> (StatusCode, String) {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, String::from_utf8(body.to_vec()).unwrap())
}

async fn insert_archives(owner: &PgPool, slug: &str, count: i32) -> Vec<i64> {
    // Owner-only bulk data deliberately exceeds normal retention policy so the
    // independent HTML read ceiling is exercised without thousands of posts.
    let ids = sqlx::query_scalar(
        "WITH inserted AS (
            INSERT INTO content.threads(board,bumped_at,archived_at,archive_expires_at)
            SELECT $1,date_trunc('day',transaction_timestamp()),
                transaction_timestamp()-interval '1 minute',
                transaction_timestamp()+interval '1 hour'
            FROM generate_series(1,$2::integer)
            RETURNING id,board
        ), posts AS (
            INSERT INTO content.posts(id,board,thread_id,name,subject,comment)
            SELECT id,board,id,'Anonymous',repeat('<',100),'Owned archive capacity fixture'
            FROM inserted RETURNING id
        ) SELECT id FROM posts ORDER BY id",
    )
    .bind(slug)
    .bind(count)
    .fetch_all(owner)
    .await
    .unwrap();
    // This bulk fixture changes table cardinality immediately, before automatic
    // analysis can run. Refresh owner-side statistics so the public query does
    // not plan thousands of OP rows as one and repeatedly scan the whole board.
    // The real public role's five-second statement deadline remains unchanged.
    for query in [
        "ANALYZE content.boards",
        "ANALYZE content.threads",
        "ANALYZE content.posts",
    ] {
        sqlx::query(query).execute(owner).await.unwrap();
    }
    ids
}

async fn capacity_contract(owner: PgPool, public: PgPool, slug: String) {
    let (web, _) = board_public::routers(public.clone(), ORIGIN.into(), false);
    let path = format!("/{slug}/archive");
    let mut ids = insert_archives(&owner, &slug, 1200).await;
    for extra in [0, 1802] {
        if extra != 0 {
            ids.extend(insert_archives(&owner, &slug, extra).await);
            // Use one root clock for all rows even if the fixture crossed midnight.
            sqlx::query("UPDATE content.threads SET bumped_at=date_trunc('day',transaction_timestamp()) WHERE board=$1")
                .bind(&slug).execute(&owner).await.unwrap();
        }
        let expected: Vec<_> = ids.iter().rev().take(3000).copied().collect();
        let snapshot = board_store::archive_page_snapshot(&public, &slug)
            .await
            .unwrap();
        assert_eq!(
            snapshot
                .snapshot
                .entries
                .iter()
                .map(|entry| entry.id)
                .collect::<Vec<_>>(),
            expected,
            "HTML reads beyond 1,000 rows, caps at 3,000 and breaks equal root clocks by descending ID"
        );
        let (status, html) = response(&web, &path).await;
        assert_eq!(status, StatusCode::OK);
        let prefix = format!("href=\"/{slug}/thread/");
        let rendered: Vec<i64> = html
            .split(&prefix)
            .skip(1)
            .map(|entry| entry.split_once('"').unwrap().0.parse().unwrap())
            .collect();
        assert_eq!(rendered, expected);
        assert!(html.contains("&#60;"));
    }

    // A larger HTML row limit cannot bypass configured output limits or publish
    // a truncated success response. No process-wide limit is raised for archives.
    let limits = board_config::PublicRequestLimits::from_lookup(|name| {
        (name == "PUBLIC_MAX_RESPONSE_BYTES").then(|| "4096".to_owned())
    })
    .unwrap();
    let (limited, _) =
        board_public::routers_with_limits(public, ORIGIN.into(), false, None, limits);
    let (status, body) = response(&limited, &path).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(!body.contains("archiveEntries"));
}

#[tokio::test]
async fn html_archive_preserves_source_capacity_and_response_budget() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let seed: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&owner)
        .await
        .unwrap();
    let slug = format!("ac{seed:x}");
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,archive_retention_seconds) VALUES($1,'Archive capacity fixture','Synthetic owned data',100,20,10,10,10,3600)")
        .bind(&slug).execute(&owner).await.unwrap();
    let result = tokio::spawn(capacity_contract(
        owner.clone(),
        public.clone(),
        slug.clone(),
    ))
    .await;
    for query in [
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(query)
            .bind(&slug)
            .execute(&owner)
            .await
            .unwrap();
    }
    public.close().await;
    owner.close().await;
    result.unwrap();
}
