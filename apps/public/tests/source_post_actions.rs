#![cfg(feature = "database-tests")]

#[path = "support/posting.rs"]
mod posting_fixture;

use axum::{Router, body::Body, http::Request};
use board_store::NewPost;
use http_body_util::BodyExt;
use rand_core::{OsRng, RngCore};
use sqlx::PgPool;
use tower::ServiceExt;

async fn get(app: &Router, path: &str, expected: u16) -> (String, String) {
    let response = app
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), expected, "{path}");
    let csp = response
        .headers()
        .get("content-security-policy")
        .unwrap()
        .to_str()
        .unwrap()
        .to_owned();
    let body = String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    (body, csp)
}

async fn exercise(owner: PgPool, public: PgPool, slug: String) {
    let draft = NewPost {
        name: "Owned selection".into(),
        subject: "Owned batch controls".into(),
        comment: "<input name=9 value=delete form=delform> is literal post text".into(),
        deletion_hash: "owned-selection-hash".into(),
        sage: false,
    };
    let thread = posting_fixture::create_post(&public, &slug, 0, &draft)
        .await
        .unwrap();
    let reply = posting_fixture::create_post(&public, &slug, thread, &draft)
        .await
        .unwrap();
    let (app, api) = posting_fixture::routers(public, &slug, "http://127.0.0.1:3000".into(), false);
    let json_path = format!("/{slug}/thread/{thread}.json");
    let api_before = get(&api, &json_path, 200).await.0;
    let selector = |id| {
        format!(
            "<input type=\"checkbox\" id=\"select{id}\" name=\"{id}\" value=\"delete\" form=\"delform\" aria-label=\"Select post No.{id}\">"
        )
    };
    for path in [format!("/{slug}/"), format!("/{slug}/thread/{thread}")] {
        let (html, csp) = get(&app, &path, 200).await;
        assert_eq!(html.matches("id=\"delform\"").count(), 1);
        assert!(html.contains(&format!(
            "action=\"/{slug}/imgboard.php\" method=\"post\" data-board=\"{slug}\""
        )));
        assert_eq!(html.matches(&selector(thread)).count(), 1);
        assert_eq!(html.matches(&selector(reply)).count(), 1);
        assert!(html.find(&selector(thread)).unwrap() < html.find(&selector(reply)).unwrap());
        assert!(html.contains("name=\"mode\" value=\"usrdel\""));
        assert!(html.contains("name=\"onlyimgdel\" value=\"on\""));
        assert!(html.contains("id=\"delPassword\" name=\"pwd\""));
        assert!(!html.contains("<input name=9"));
        assert!(html.contains("src=\"/static/source-post-actions.v1.js\""));
        assert!(csp.contains("http://127.0.0.1:3000/static/source-post-actions.v1.js"));
        assert!(!csp.contains("unsafe-inline"));
    }
    for path in [
        format!("/_watch/{slug}/thread/{thread}/posts"),
        format!("/_watch/{slug}/post/{reply}"),
    ] {
        let output = get(&app, &path, 200).await.0;
        let value: serde_json::Value = serde_json::from_str(&output).unwrap();
        let html = if value.get("posts").is_some() {
            value["posts"][1]["html"].as_str().unwrap()
        } else {
            value["post"]["html"].as_str().unwrap()
        };
        assert!(html.contains(&selector(reply)));
        assert!(!html.contains("id=\"delform\""));
    }
    let (catalog, csp) = get(&app, &format!("/{slug}/catalog"), 200).await;
    assert!(!catalog.contains("form=\"delform\"") && !catalog.contains("id=\"delform\""));
    assert!(
        !catalog.contains("source-post-actions.v1.js")
            && !csp.contains("source-post-actions.v1.js")
    );
    assert_eq!(get(&api, &json_path, 200).await.0, api_before);
    let (script, csp) = get(&app, "/static/source-post-actions.v1.js", 200).await;
    assert!(script.contains("mountSourcePostActions"));
    assert!(!csp.contains("script-src http://127.0.0.1:3000/static/source-post-actions.v1.js"));
    sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
        .bind(thread).execute(&owner).await.unwrap();
    let (archived, _) = get(&app, &format!("/{slug}/thread/{thread}"), 200).await;
    assert!(!archived.contains("form=\"delform\"") && !archived.contains("id=\"delform\""));
    assert!(!archived.contains("src=\"/static/source-post-actions.v1.js\""));
    sqlx::query("UPDATE content.boards SET staff_only=true WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    let (private, csp) = get(&app, &format!("/{slug}/thread/{thread}"), 404).await;
    assert!(!private.contains("delform") && !csp.contains("source-post-actions.v1.js"));
}

#[tokio::test]
async fn original_selection_controls_bind_only_visible_nonarchive_posts_without_changing_reads() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let slug = format!("sel{:06x}", OsRng.next_u32() & 0xffffff);
    sqlx::query("INSERT INTO content.boards(posting_reply_seconds,posting_image_seconds,posting_thread_seconds,slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,archive_retention_seconds) VALUES(0,0,0,$1,'Owned selection','Synthetic',2000,100,100,100,10,3600)")
        .bind(&slug).execute(&owner).await.unwrap();
    let result = tokio::spawn(exercise(owner.clone(), public.clone(), slug.clone())).await;
    public.close().await;
    sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)").bind(&slug).execute(&owner).await.unwrap();
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
    owner.close().await;
    result.unwrap();
}
