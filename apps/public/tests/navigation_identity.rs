#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use sqlx::PgPool;
use tower::ServiceExt;

async fn get(app: &Router, path: &str) -> String {
    let response = app
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{path}");
    String::from_utf8(
        to_bytes(response.into_body(), 4_000_000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

#[tokio::test]
async fn source_navigation_identity_does_not_replace_page_or_discovery_titles() {
    let pool = PgPool::connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let identity: String = sqlx::query_scalar("SELECT current_user")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(identity, "board_public");
    let boards = board_store::boards(&pool).await.unwrap();
    let (web, api) = board_public::routers(pool.clone(), "http://127.0.0.1:3000".into(), false);
    for (slug, label) in [
        ("p", "Photo"),
        ("diy", "Do It Yourself"),
        ("lgbt", "LGBT"),
        ("s4s", "Shit 4chan Says"),
    ] {
        let board = boards
            .iter()
            .find(|b| b.slug == slug)
            .expect("configured public source control");
        let html = get(&web, &format!("/{slug}/")).await;
        assert!(html.contains(&format!("href=\"/{slug}/\" title=\"{label}\"")));
        let heading = if slug == "s4s" {
            format!("[{slug}] - {}", board.title)
        } else {
            format!("/{slug}/ - {}", board.title)
        };
        let opening = "<div class=\"boardTitle\" role=\"heading\" aria-level=\"1\">";
        let start = html.find(opening).unwrap();
        let end = start + html[start..].find("</div>").unwrap() + 6;
        let doc = roxmltree::Document::parse(&html[start..end]).unwrap();
        assert_eq!(doc.root_element().text().unwrap(), heading);
        assert_eq!(html.matches("data-public-board-group=").count(), 10);
        assert!(!html.contains("href=\"/j/\""));
        assert!(!html.contains("data-current-board-fallback"));
    }
    let home = get(&web, "/").await;
    for board in &boards {
        assert!(
            home.contains(&format!("href=\"/{}/\"", board.slug)),
            "directory lost {}",
            board.slug
        );
    }
    let watch: serde_json::Value =
        serde_json::from_str(&get(&web, "/_watch/boards").await).unwrap();
    assert_eq!(watch["version"], 1);
    let rows = watch["boards"].as_array().unwrap();
    assert_eq!(rows.len(), boards.len());
    for (row, board) in rows.iter().zip(&boards) {
        assert_eq!(row["board"], board.slug);
        assert_eq!(row["title"], board.title);
    }
    let metadata: serde_json::Value =
        serde_json::from_str(&get(&api, "/boards.json").await).unwrap();
    let rows = metadata["boards"].as_array().unwrap();
    let mut expected: Vec<_> = boards
        .iter()
        .filter(|board| board.json_enabled)
        .map(|board| board.slug.as_str())
        .collect();
    expected.sort_unstable();
    assert!(!expected.contains(&"j"));
    assert_eq!(rows.len(), expected.len());
    assert_eq!(
        rows.iter()
            .map(|row| row["board"].as_str().unwrap())
            .collect::<Vec<_>>(),
        expected
    );
    for row in metadata["boards"].as_array().unwrap() {
        let board = boards
            .iter()
            .find(|board| row["board"] == board.slug)
            .unwrap();
        assert_eq!(row["title"], board.title);
    }
    pool.close().await;
}
