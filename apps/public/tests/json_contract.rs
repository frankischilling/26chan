#![cfg(feature = "database-tests")]
use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

async fn get(app: &axum::Router, path: &str) -> Value {
    let response = app
        .clone()
        .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{path}");
    assert_eq!(response.headers()["content-type"], "application/json");
    assert_eq!(
        response.headers()["cache-control"],
        "public, max-age=0, must-revalidate"
    );
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

#[tokio::test]
async fn documented_text_contract_matches_synthetic_board_across_endpoints() {
    let url =
        std::env::var("TEST_PUBLIC_DATABASE_URL").expect("TEST_PUBLIC_DATABASE_URL is required");
    let pool = board_store::connect_public(&url).await.unwrap();
    let app = board_public::router(pool.clone(), "http://127.0.0.1:3000".into(), false);
    let boards = get(&app, "/boards.json").await;
    let demo = boards["boards"]
        .as_array()
        .unwrap()
        .iter()
        .find(|board| board["board"] == "demo")
        .unwrap();
    assert_eq!(demo["per_page"], 10);
    assert_eq!(demo["ws_board"], 1);
    assert_eq!(demo["text_only"], 1);
    assert_eq!(demo["max_filesize"], 0);
    assert!(demo.get("is_archived").is_none());
    let thread = get(&app, "/demo/thread/1000001.json").await;
    let op = &thread["posts"][0];
    assert_eq!(op["no"], 1000001);
    assert_eq!(op["resto"], 0);
    assert_eq!(op["now"], "09/08/26(Tue)08:00:00");
    assert_eq!(op["time"], 1788868800_i64);
    assert_eq!(op["semantic_url"], "what-are-you-making");
    assert_eq!(op["replies"], 1);
    assert_eq!(op["images"], 0);
    assert_eq!(thread["posts"][1]["resto"], 1000001);
    for field in ["replies", "images", "sub", "semantic_url"] {
        assert!(thread["posts"][1].get(field).is_none(), "{field}");
    }
    for field in [
        "sticky",
        "closed",
        "bumplimit",
        "tim",
        "filename",
        "unique_ips",
    ] {
        assert!(op.get(field).is_none(), "{field}");
    }
    assert_eq!(
        get(&app, "/demo/threads.json").await,
        json!([{"page":1,"threads":[{"no":1000001,"last_modified":1788869100_i64,"replies":1}]}])
    );
    let index = get(&app, "/demo/1.json").await;
    assert_eq!(index["threads"][0]["posts"], thread["posts"]);
    let catalog = get(&app, "/demo/catalog.json").await;
    assert_eq!(catalog[0]["page"], 1);
    assert_eq!(
        catalog[0]["threads"][0]["last_replies"][0],
        thread["posts"][1]
    );
    assert_eq!(catalog[0]["threads"][0]["last_modified"], 1788869100_i64);
    pool.close().await;
}
