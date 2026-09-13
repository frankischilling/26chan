#![forbid(unsafe_code)]
// Test-only renderer for screenshot CI. It compiles the actual production view
// module/templates. Persistence and real HTTP mutations have separate tests.
#[path = "visual/media.rs"]
mod media;
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
fn board() -> Board {
    Board {
        slug: "demo".into(),
        title: "Paper craft".into(),
        description: "Discuss paper models, folding, and works in progress.".into(),
        max_comment_chars: 4000,
        reply_limit: 100,
        bump_limit: 75,
        thread_limit: 100,
        threads_per_page: 10,
        worksafe: true,
        archive_retention_seconds: 0,
        archive_limit: 1000,
        image_limit: 0,
    }
}
fn page(catalog: bool) -> String {
    let board = board();
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
        archived_at: None,
        archive_expires_at: None,
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
        attachment: None,
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
            attachment: None,
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
        media_origin: String::new(),
    }
    .render()
    .expect("production templates")
}

fn archive_board(slug: &str) -> Board {
    Board {
        slug: slug.into(),
        archive_retention_seconds: 3600,
        ..board()
    }
}

fn archive_page(empty: bool) -> String {
    let entries = if empty {
        vec![]
    } else {
        vec![
            board_store::ArchiveEntry {
                id: 1000101,
                subject: "<b>A paper lighthouse</b>".into(),
                archived_at: time("2026-09-08T13:00:00Z"),
            },
            board_store::ArchiveEntry {
                id: 1000102,
                subject: String::new(),
                archived_at: time("2026-09-08T13:05:00Z"),
            },
            board_store::ArchiveEntry {
                id: 1000103,
                // The maximum-length unbroken subject exercises mobile wrapping.
                subject: "Fold".repeat(30),
                archived_at: time("2026-09-08T13:10:00Z"),
            },
        ]
    };
    views::ArchivePage {
        board: archive_board(if empty { "emptyarc" } else { "arc" }),
        entries,
    }
    .render()
    .expect("production archive template")
}

fn archived_thread() -> String {
    let board = archive_board("arc");
    let thread = Thread {
        id: 1000101,
        board: board.slug.clone(),
        created_at: time("2026-09-08T12:00:00Z"),
        bumped_at: time("2026-09-08T12:05:00Z"),
        modified_at: time("2026-09-08T13:00:00Z"),
        reply_count: 1,
        sticky: false,
        closed: false,
        deleted: false,
        archived_at: Some(time("2026-09-08T13:00:00Z")),
        archive_expires_at: Some(time("2026-09-08T14:00:00Z")),
    };
    let posts = vec![
        PostView::new(Post {
            id: 1000101,
            board: board.slug.clone(),
            thread_id: thread.id,
            name: "Anonymous".into(),
            subject: "<b>A paper lighthouse</b>".into(),
            comment: "The completed paper lighthouse.\n>fold each edge carefully\n[spoiler]There is a tiny door at the back.[/spoiler]".into(),
            created_at: thread.created_at,
            deleted: false,
            attachment: None,
        }),
        PostView::new(Post {
            id: 1000104,
            board: board.slug.clone(),
            thread_id: thread.id,
            name: "Anonymous".into(),
            subject: String::new(),
            comment: ">>1000101\nThe roof looks good. Thanks for sharing your finished project.".into(),
            created_at: thread.bumped_at,
            deleted: false,
            attachment: None,
        }),
    ];
    BoardPage {
        parent: thread.id,
        board,
        threads: vec![ThreadView {
            thread,
            posts,
            omitted: 0,
        }],
        previous: String::new(),
        next: String::new(),
        catalog: false,
        media_origin: String::new(),
    }
    .render()
    .expect("production archived thread template")
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
    let _ = views::ArchivePage {
        board: board(),
        entries: vec![],
    }
    .render()
    .unwrap();
    let _ = views::Comment {
        lines: &[],
        board: "demo",
    }
    .render()
    .unwrap();
    let (media_fixture, media_app) = media::Fixture::build().await;
    let media_listener = tokio::net::TcpListener::bind("127.0.0.1:3004")
        .await
        .unwrap();
    let media_server =
        tokio::spawn(async move { axum::serve(media_listener, media_app).await.unwrap() });
    let app = Router::new()
        .merge(media_fixture.routes())
        .route("/demo/", get(|| async { Html(page(false)) }))
        .route("/demo/catalog", get(|| async { Html(page(true)) }))
        .route("/arc/archive", get(|| async { Html(archive_page(false)) }))
        .route(
            "/emptyarc/archive",
            get(|| async { Html(archive_page(true)) }),
        )
        .route(
            "/arc/thread/1000101",
            get(|| async { Html(archived_thread()) }),
        )
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
    tokio::select! {
        result = axum::serve(listener, app) => result.unwrap(),
        result = media_server => { result.unwrap(); panic!("fixture media server stopped"); }
    }
}
