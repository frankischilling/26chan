#![cfg(feature = "database-tests")]
#[path = "support/posting.rs"]
mod posting;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use board_store::NewPost;
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

const ORIGIN: &str = "http://127.0.0.1:3000";
const COMMENT: &str = "[math]x^2[/math] [eqn]\\frac{1}{2}[/eqn]";
async fn get(app: &Router, path: &str) -> (StatusCode, HeaderMap, String) {
    let response = app
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    let (parts, body) = response.into_parts();
    (
        parts.status,
        parts.headers,
        String::from_utf8(to_bytes(body, 4_194_304).await.unwrap().to_vec()).unwrap(),
    )
}
fn authority(headers: &HeaderMap, enabled: bool) {
    let policy = headers["content-security-policy"].to_str().unwrap();
    assert_eq!(policy.contains("/static/native-math.v1.js"), enabled);
    assert_eq!(policy.contains("/static/native-math-worker.v1.js"), enabled);
    assert!(!policy.contains("unsafe-inline") && !policy.contains("unsafe-eval"));
    assert!(!policy.contains("font-src") && !policy.contains("cdn."));
}
async fn exercise(owner: &PgPool, public: &PgPool, slug: &str) {
    let (app, api) = posting::routers(public.clone(), slug, ORIGIN.into(), false);
    let reference: Value =
        serde_json::from_str(include_str!("../../../fixtures/board-reference.json")).unwrap();
    let (_, _, directory) = get(&api, "/boards.json").await;
    let directory: Value = serde_json::from_str(&directory).unwrap();
    for expected in reference["boards"].as_array().unwrap() {
        let source = expected["slug"].as_str().unwrap();
        let saved: bool = sqlx::query_scalar("SELECT math_tags FROM content.boards WHERE slug=$1")
            .bind(source)
            .fetch_one(owner)
            .await
            .unwrap();
        assert_eq!(saved, source == "sci", "/{source}/ policy");
        if let Some(board) = directory["boards"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["board"] == source)
        {
            if saved {
                assert_eq!(board["math_tags"], 1);
            } else {
                assert!(board.get("math_tags").is_none());
            }
        }
    }
    let id = posting::create_post(
        public,
        slug,
        0,
        &NewPost {
            name: "Anonymous".into(),
            subject: "Owned math projection".into(),
            comment: COMMENT.into(),
            deletion_hash: "owned-math-fixture-hash".into(),
            sage: false,
        },
    )
    .await
    .unwrap();
    for enabled in [false, true, false] {
        sqlx::query("UPDATE content.boards SET math_tags=$2 WHERE slug=$1")
            .bind(slug)
            .bind(enabled)
            .execute(owner)
            .await
            .unwrap();
        for path in [format!("/{slug}/"), format!("/{slug}/thread/{id}")] {
            let (status, headers, html) = get(&app, &path).await;
            assert_eq!(status, StatusCode::OK, "{path}");
            authority(&headers, enabled);
            assert_eq!(html.contains("data-math-tags=\"1\""), enabled);
            assert_eq!(html.contains("src=\"/static/native-math.v1.js\""), enabled);
            for tag in ["[math]", "[/math]", "[eqn]", "[/eqn]"] {
                assert!(html.contains(tag));
            }
        }
        let (status, headers, html) = get(&app, &format!("/{slug}/catalog")).await;
        assert_eq!(status, StatusCode::OK);
        authority(&headers, false);
        assert!(!html.contains("data-math-tags=\"1\"") && !html.contains("native-math.v1.js"));
        let (status, headers, _) = get(&app, &format!("/{slug}/thread/9223372036854775807")).await;
        assert_eq!(status, StatusCode::NOT_FOUND);
        authority(&headers, false);
        let (status, headers, raw) = get(&api, &format!("/{slug}/thread/{id}.json")).await;
        assert_eq!(status, StatusCode::OK);
        authority(&headers, false);
        let raw: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(raw["posts"][0]["com"], COMMENT);
        let saved: String = sqlx::query_scalar("SELECT comment FROM content.posts WHERE id=$1")
            .bind(id)
            .fetch_one(owner)
            .await
            .unwrap();
        assert_eq!(saved, COMMENT);
        let (_, _, raw) = get(&api, "/boards.json").await;
        let raw: Value = serde_json::from_str(&raw).unwrap();
        let board = raw["boards"]
            .as_array()
            .unwrap()
            .iter()
            .find(|b| b["board"] == slug)
            .unwrap();
        if enabled {
            assert_eq!(board["math_tags"], 1);
        } else {
            assert!(board.get("math_tags").is_none());
        }
    }
    assert!(
        sqlx::query("UPDATE content.boards SET math_tags=true WHERE slug=$1")
            .bind(slug)
            .execute(public)
            .await
            .is_err()
    );
}
#[tokio::test]
async fn math_display_policy_has_narrow_authority_and_does_not_rewrite_comments() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let slug: String =
        sqlx::query_scalar("SELECT 'mt'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) VALUES($1,'Owned math','Synthetic source-parity fixture',2000,100,100,100,10,0,0,0)")
        .bind(&slug).execute(&owner).await.unwrap();
    let owned = owner.clone();
    let runtime = public.clone();
    let board = slug.clone();
    let result = tokio::spawn(async move { exercise(&owned, &runtime, &board).await }).await;
    posting::cleanup_posting(&owner, &slug).await;
    for query in [
        "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
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
