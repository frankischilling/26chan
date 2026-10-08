#![cfg(feature = "database-tests")]

#[path = "support/posting.rs"]
mod posting_fixture;

use axum::{Router, body::Body, http::Request};
use board_store::NewPost;
use http_body_util::BodyExt;
use rand_core::{OsRng, RngCore};
use sqlx::PgPool;
use tower::ServiceExt;

async fn get(app: &Router, path: &str, status: u16) -> String {
    let response = app
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status().as_u16(), status, "{path}");
    String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap()
}

async fn exercise(owner: PgPool, public: PgPool, slug: String) {
    let draft = NewPost {
        name: "Owned <name>".into(),
        subject: "Owned post-number links".into(),
        comment: "Owned quote target".into(),
        deletion_hash: "owned-link-hash".into(),
        sage: false,
    };
    let thread = posting_fixture::create_post(&public, &slug, 0, &draft)
        .await
        .unwrap();
    let reply = posting_fixture::create_post(&public, &slug, thread, &draft)
        .await
        .unwrap();
    let other = posting_fixture::create_post(&public, &slug, 0, &draft)
        .await
        .unwrap();
    let (app, api) = posting_fixture::routers(public, &slug, "http://127.0.0.1:3000".into(), false);
    let path = format!("/{slug}/thread/{thread}");
    let before = get(&api, &format!("{path}.json"), 200).await;
    let links = format!(
        "<span class=\"postNum\"><a href=\"/{slug}/thread/{thread}#p{reply}\" title=\"Link to this post\">No.</a><a href=\"/{slug}/thread/{thread}?quote={reply}#reply\" title=\"Reply to this post\">{reply}</a></span>"
    );
    for route in [
        path.clone(),
        format!("/{slug}/"),
        format!("/_watch/{slug}/thread/{thread}/posts"),
        format!("/_watch/{slug}/post/{reply}"),
    ] {
        let output = get(&app, &route, 200).await;
        let html = if route.starts_with("/_watch/") {
            let value: serde_json::Value = serde_json::from_str(&output).unwrap();
            if value.get("posts").is_some() {
                value["posts"][1]["html"].as_str().unwrap().to_owned()
            } else {
                value["post"]["html"].as_str().unwrap().to_owned()
            }
        } else {
            output
        };
        assert!(html.contains(&links), "{route}");
        assert!(!html.contains("javascript:"));
    }
    for no in [thread, reply] {
        let html = get(&app, &format!("{path}?quote={no}"), 200).await;
        assert!(html.contains(&format!(
            "aria-describedby=\"postHelp\">&#62;&#62;{no}\n</textarea>"
        )));
        assert!(html.contains("<form id=\"reply\""));
    }
    let plain = get(&app, &path, 200).await;
    assert!(plain.contains("aria-describedby=\"postHelp\"></textarea>"));
    for value in [
        "",
        "0",
        "01",
        "-1",
        "%2B1",
        "1.0",
        "1e3",
        "9223372036854775808",
        "%3Cscript%3E",
    ] {
        get(&app, &format!("{path}?quote={value}"), 400).await;
    }
    get(&app, &format!("{path}?quote={reply}&quote={reply}"), 400).await;
    get(&app, &format!("{path}?quote={other}"), 404).await;
    assert_eq!(get(&api, &format!("{path}.json"), 200).await, before);
    // HTML query validation does not change the public API's query contract.
    assert_eq!(
        get(&app, &format!("{path}.json?quote=invalid"), 200).await,
        before
    );
    sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1")
        .bind(reply)
        .execute(&owner)
        .await
        .unwrap();
    get(&app, &format!("{path}?quote={reply}"), 404).await;
    for archived in [false, true] {
        sqlx::query("UPDATE content.threads SET closed=NOT $2,archived_at=CASE WHEN $2 THEN clock_timestamp() ELSE NULL END,archive_expires_at=CASE WHEN $2 THEN clock_timestamp()+interval '1 hour' ELSE NULL END WHERE id=$1")
            .bind(thread).bind(archived).execute(&owner).await.unwrap();
        get(&app, &format!("{path}?quote={thread}"), 400).await;
        let html = get(&app, &path, 200).await;
        assert!(!html.contains("?quote="));
        assert!(!html.contains("<form id=\"reply\""));
        assert!(html.contains(&format!(
            "#p{thread}\" title=\"Reply to this post\">{thread}</a>"
        )));
    }
}

#[tokio::test]
async fn post_links_and_quote_forms_use_visible_thread_posts_without_writes() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut random = [0u8; 5];
    OsRng.fill_bytes(&mut random);
    let slug: String = random.iter().map(|b| format!("{b:02x}")).collect();
    sqlx::query("INSERT INTO content.boards(posting_reply_seconds,posting_image_seconds,posting_thread_seconds,slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,archive_retention_seconds) VALUES(0,0,0,$1,'Post links','Owned fixture',1000,100,100,100,10,3600)")
        .bind(&slug).execute(&owner).await.unwrap();
    let result = tokio::spawn(exercise(owner.clone(), public.clone(), slug.clone())).await;
    public.close().await;
    sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)")
        .bind(&slug).execute(&owner).await.unwrap();
    for statement in [
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(statement)
            .bind(&slug)
            .execute(&owner)
            .await
            .unwrap();
    }
    owner.close().await;
    result.unwrap();
}
