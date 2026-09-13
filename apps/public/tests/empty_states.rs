#![cfg(feature = "database-tests")]

use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use rand_core::{OsRng, RngCore};
use tower::ServiceExt;

#[tokio::test]
async fn empty_catalog_links_to_the_actual_board_posting_form() {
    let owner = sqlx::PgPool::connect(
        &std::env::var("MIGRATION_DATABASE_URL").expect("MIGRATION_DATABASE_URL required"),
    )
    .await
    .unwrap();
    let public = board_store::connect_public(
        &std::env::var("TEST_PUBLIC_DATABASE_URL").expect("TEST_PUBLIC_DATABASE_URL required"),
    )
    .await
    .unwrap();
    let slug = format!("e{:08x}", OsRng.next_u32());
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES ($1,'Empty test','Synthetic empty board',4000,100,75,100,10)")
        .bind(&slug).execute(&owner).await.unwrap();
    let app = board_public::router(public, "http://127.0.0.1:3000".into(), false);
    let mut pages = vec![];
    for suffix in ["", "catalog"] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/{slug}/{suffix}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        pages.push((status, String::from_utf8(bytes.to_vec()).unwrap()));
    }
    sqlx::query("DELETE FROM content.boards WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    let [(board_status, board), (catalog_status, catalog)] = pages.as_slice() else {
        panic!("two route responses required")
    };
    assert_eq!(*board_status, 200);
    assert_eq!(*catalog_status, 200);
    assert!(board.contains("id=\"postForm\""));
    assert!(board.contains("No threads yet. Start the first thread above."));
    assert!(!catalog.contains("id=\"postForm\""));
    assert!(!catalog.contains("Start the first thread above."));
    assert!(catalog.contains(&format!(
        "href=\"/{slug}/#postForm\">Start the first thread</a>"
    )));
}
