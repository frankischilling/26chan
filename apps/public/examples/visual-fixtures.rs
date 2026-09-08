#![forbid(unsafe_code)]
// Test-only renderer for screenshot CI. It compiles the actual production view
// module/templates. Persistence and real HTTP mutations have separate tests.
#[path = "../src/views.rs"]
mod views;
use askama::Template;
use axum::{Router, response::Html, routing::get};
use board_store::{Board, Post, Thread};
use chrono::{DateTime, Utc};
use views::{BoardPage, PostView, ThreadView};

fn time(value: &str) -> DateTime<Utc> {
    value.parse().expect("fixed synthetic timestamp")
}
fn page(catalog: bool) -> String {
    let board = Board {
        slug: "demo".into(),
        title: "Paper craft".into(),
        description: "Discuss paper models, folding, and works in progress.".into(),
        max_comment_bytes: 4000,
        reply_limit: 100,
        bump_limit: 75,
        thread_limit: 100,
        threads_per_page: 10,
        worksafe: true,
    };
    let thread = Thread {
        id: 1000001,
        board: "demo".into(),
        created_at: time("2026-09-08T12:00:00Z"),
        bumped_at: time("2026-09-08T12:05:00Z"),
        modified_at: time("2026-09-08T12:05:00Z"),
        reply_count: 1,
        sticky: false,
        closed: false,
        deleted: false,
    };
    let mut posts = vec![PostView::new(Post {
        id: 1000001,
        board: "demo".into(),
        thread_id: 1000001,
        name: "Anonymous".into(),
        subject: "What are you making?".into(),
        comment: ">start with a single sheet".into(),
        created_at: time("2026-09-08T12:00:00Z"),
        deleted: false,
    })];
    posts[0] = PostView::new(Post { comment: "Share your latest paper project.\n>start with a single sheet\n[spoiler]Mine is another crane.[/spoiler]".into(), ..posts[0].post.clone() });
    if !catalog {
        posts.push(PostView::new(Post {
            id: 1000002,
            board: "demo".into(),
            thread_id: 1000001,
            name: "Anonymous".into(),
            subject: String::new(),
            comment: ">>1000001\nA small paper lighthouse. Still working on the roof.".into(),
            created_at: time("2026-09-08T12:05:00Z"),
            deleted: false,
        }));
    }
    BoardPage {
        board,
        threads: vec![ThreadView {
            thread,
            posts,
            omitted: usize::from(catalog),
        }],
        parent: 0,
        previous: String::new(),
        next: String::new(),
        catalog,
    }
    .render()
    .expect("production templates")
}

#[tokio::main]
async fn main() {
    // Construct the other shared templates too; this keeps this include honest
    // when production views change and avoids unused-item warning suppression.
    let _ = views::Home { boards: vec![] }.render().unwrap();
    let _ = views::Message {
        title: "Fixture",
        message: "Synthetic data",
    }
    .render()
    .unwrap();
    let _ = views::Comment {
        lines: &[],
        board: "demo",
    }
    .render()
    .unwrap();
    let app = Router::new()
        .route("/demo/", get(|| async { Html(page(false)) }))
        .route("/demo/catalog", get(|| async { Html(page(true)) }))
        .route(
            "/static/board.css",
            get(|| async {
                (
                    [("content-type", "text/css")],
                    include_str!("../static/board.css"),
                )
            }),
        )
        .route("/readyz", get(|| async { "synthetic fixture renderer" }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    axum::serve(listener, app).await.unwrap();
}
