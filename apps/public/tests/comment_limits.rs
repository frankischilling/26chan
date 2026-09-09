#![cfg(feature = "database-tests")]

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use rand_core::{OsRng, RngCore};
use tower::ServiceExt;

fn form_request(path: &str, comment: &str, parent: i64) -> Request<Body> {
    let encoded: String = comment
        .as_bytes()
        .iter()
        .map(|byte| format!("%{byte:02X}"))
        .collect();
    Request::builder()
        .method("POST")
        .uri(path)
        .header("origin", "http://127.0.0.1:3000")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(format!(
            "name=Unicode&sub=Boundary&com={encoded}&password=unicode-password&resto={parent}"
        )))
        .unwrap()
}

async fn get(app: &axum::Router, path: &str) -> axum::response::Response {
    app.clone()
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn body_text(response: axum::response::Response) -> String {
    assert_eq!(response.status(), StatusCode::OK);
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

/// Exercise form encoding, both write routes, actual runtime grants, database
/// constraints, rendering and JSON together against a uniquely named board.
#[tokio::test]
async fn advertised_unicode_limit_survives_forms_storage_html_and_json() {
    let owner = sqlx::PgPool::connect(
        &std::env::var("MIGRATION_DATABASE_URL").expect("MIGRATION_DATABASE_URL is required"),
    )
    .await
    .unwrap();
    let public = board_store::connect_public(
        &std::env::var("TEST_PUBLIC_DATABASE_URL").expect("TEST_PUBLIC_DATABASE_URL is required"),
    )
    .await
    .unwrap();
    let mut random = [0_u8; 5];
    OsRng.fill_bytes(&mut random);
    let board: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Unicode HTTP boundary','Synthetic test',16000,10,10,10,10)")
        .bind(&board).execute(&owner).await.unwrap();
    let app = board_public::router(public.clone(), "http://127.0.0.1:3000".into(), false);
    let listing: serde_json::Value =
        serde_json::from_str(&body_text(get(&app, "/boards.json").await).await).unwrap();
    let advertised = listing["boards"]
        .as_array()
        .unwrap()
        .iter()
        .find(|value| value["board"] == board)
        .unwrap();
    assert_eq!(advertised["max_comment_chars"], 16_000);
    let board_html = body_text(get(&app, &format!("/{board}/")).await).await;
    assert!(board_html.contains("16000 characters"));

    let comment = "😀".repeat(16_000);
    let request = form_request(&format!("/{board}/post"), &comment, 0);
    assert!(!request.headers().contains_key("content-length"));
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let location = response.headers()["location"].to_str().unwrap().to_owned();
    let thread: i64 = location.split("#p").nth(1).unwrap().parse().unwrap();
    let json_path = format!("/{board}/thread/{thread}.json");
    let html_path = format!("/{board}/thread/{thread}");
    let original = get(&app, &json_path).await;
    let original_etag = original.headers()["etag"].clone();
    let original_json: serde_json::Value =
        serde_json::from_str(&body_text(original).await).unwrap();
    assert_eq!(original_json["posts"][0]["com"], comment);
    assert_eq!(original_json["posts"][0]["replies"], 0);
    assert!(
        body_text(get(&app, &html_path).await)
            .await
            .contains(&comment)
    );

    // The legacy write alias must enforce the same limit without changing
    // content, counters or cache validators when one character is added.
    let overflow = format!("{comment}a");
    for route in ["post", "imgboard.php"] {
        let denied = app
            .clone()
            .oneshot(form_request(
                &format!("/{board}/{route}"),
                &overflow,
                thread,
            ))
            .await
            .unwrap();
        assert_eq!(denied.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }
    let unchanged = get(&app, &json_path).await;
    assert_eq!(unchanged.headers()["etag"], original_etag);
    let unchanged_json: serde_json::Value =
        serde_json::from_str(&body_text(unchanged).await).unwrap();
    assert_eq!(unchanged_json, original_json);
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1")
        .bind(&board)
        .fetch_one(&public)
        .await
        .unwrap();
    assert_eq!(count, 1);

    let reply = app
        .clone()
        .oneshot(form_request(
            &format!("/{board}/imgboard.php"),
            &comment,
            thread,
        ))
        .await
        .unwrap();
    assert_eq!(reply.status(), StatusCode::SEE_OTHER);
    let changed = get(&app, &json_path).await;
    assert_ne!(changed.headers()["etag"], original_etag);
    let changed_json: serde_json::Value = serde_json::from_str(&body_text(changed).await).unwrap();
    assert_eq!(changed_json["posts"].as_array().unwrap().len(), 2);
    assert_eq!(changed_json["posts"][0]["replies"], 1);
    assert_eq!(changed_json["posts"][1]["com"], comment);
    let html = body_text(get(&app, &html_path).await).await;
    assert_eq!(html.matches(&comment).count(), 2);
    let stored: Vec<String> =
        sqlx::query_scalar("SELECT comment FROM content.posts WHERE board=$1 ORDER BY id")
            .bind(&board)
            .fetch_all(&public)
            .await
            .unwrap();
    assert_eq!(stored, vec![comment.clone(), comment]);

    // Remove only this test's isolated rows with the fixture owner.
    sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)").bind(&board).execute(&owner).await.unwrap();
    sqlx::query("DELETE FROM content.posts WHERE board=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.threads WHERE board=$1")
        .bind(&board)
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
