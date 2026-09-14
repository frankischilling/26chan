#![forbid(unsafe_code)]
// Test-only renderer for screenshot CI. It compiles the actual production view
// module/templates. Persistence and real HTTP mutations have separate tests.
use board_public::catalog;
#[path = "visual/catalog_limits.rs"]
mod catalog_limits;
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
        comment_code_spacing: false,
        comment_sjis_spacing: false,
        comment_max_lines: 70,
        comment_spoiler_cleanup: false,
        require_subject: false,
        op_markup: false,
        forced_anon: false,
        text_only: false,
        reply_limit: 100,
        bump_limit: 75,
        permasage_hours: 0,
        op_bump_limit: true,
        op_bump_initial_seconds: 900,
        op_bump_repeat_seconds: 300,
        thread_limit: 100,
        threads_per_page: 10,
        worksafe: true,
        archive_retention_seconds: 0,
        archive_limit: 1000,
        image_limit: 0,
    }
}
fn page(catalog: bool) -> String {
    render_page(catalog, false, false, false)
}

fn render_page(catalog: bool, markup: bool, text_only: bool, forced_anon: bool) -> String {
    let mut board = board();
    board.text_only = text_only;
    board.forced_anon = forced_anon;
    if text_only {
        board.image_limit = 3;
    }
    let mut thread = Thread {
        id: 1000001,
        board: "demo".into(),
        created_at: time("2026-09-08T12:00:00Z"),
        bumped_at: time("2026-09-08T12:05:00Z"),
        modified_at: time("2026-09-08T12:05:00Z"),
        http_modified_at: time("2026-09-08T12:06:00Z"),
        reply_count: 1,
        sticky: false,
        permasage: false,
        permaage: false,
        undead: false,
        closed: false,
        deleted: false,
        archived_at: None,
        archive_expires_at: None,
    };
    let mut posts = vec![PostView::new(Post {
        comment_format: 0,
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
            comment_format: 0,
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
    if markup {
        // Prepared synthetic comments, each with its own posting-time policy.
        posts[0] = PostView::new(Post {
            comment_format: 9,
            comment: "before [spoiler]hidden\nsecond[/spoiler] after\n[spoiler]<img src=x onerror=bad()>[/spoiler]".into(),
            ..posts[0].post.clone()
        });
        posts[1] = PostView::new(Post {
            comment_format: 10,
            comment: "[code]first  line\nsecond <script>line</script>[/code]".into(),
            ..posts[1].post.clone()
        });
        posts.push(PostView::new(Post {
            id: 1_000_003,
            comment_format: 12,
            comment: "[sjis]a  b\n c[/sjis]".into(),
            ..posts[1].post.clone()
        }));
        posts.push(PostView::new(Post {
            id: 1_000_004,
            comment_format: 24,
            comment: "[b]bold[/b] [i]italic[/i]\n[red]red[/red] [green]green[/green] [blue]blue[/blue]\n[b]<script>text stays text</script>[/b]".into(),
            ..posts[1].post.clone()
        }));
        thread.reply_count = 3;
    }
    BoardPage {
        catalog_hidden: Vec::new(),
        board,
        threads: vec![ThreadView {
            catalog_last_reply: Some(board_store::CatalogReply {
                thread_id: 1_000_001,
                id: 1_000_002,
                name: "Synthetic reply author".into(),
                created_at: time("2026-09-08T12:05:00Z"),
            }),
            tail_size: 0,
            latest_reply_id: Some(1_000_002),
            thread,
            posts,
            omitted: usize::from(catalog),
            image_replies: 0,
        }],
        parent: 0,
        previous: String::new(),
        next: String::new(),
        catalog,
        catalog_options: catalog::Options::default(),
        media_origin: if text_only {
            "http://localhost:3004".into()
        } else {
            String::new()
        },
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

fn empty_page(catalog: bool) -> String {
    BoardPage {
        catalog_hidden: Vec::new(),
        board: Board {
            slug: "empty".into(),
            ..board()
        },
        threads: vec![],
        parent: 0,
        previous: String::new(),
        next: String::new(),
        catalog,
        catalog_options: catalog::Options::default(),
        media_origin: String::new(),
    }
    .render()
    .expect("production empty board template")
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
        http_modified_at: time("2026-09-08T13:00:00Z"),
        reply_count: 1,
        sticky: false,
        permasage: false,
        permaage: false,
        undead: false,
        closed: false,
        deleted: false,
        archived_at: Some(time("2026-09-08T13:00:00Z")),
        archive_expires_at: Some(time("2026-09-08T14:00:00Z")),
    };
    let posts = vec![
        PostView::new(Post {
            comment_format: 0,
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
            comment_format: 0,
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
        catalog_hidden: Vec::new(),
        parent: thread.id,
        board,
        threads: vec![ThreadView {
            catalog_last_reply: None,
            tail_size: 0,
            latest_reply_id: posts
                .iter()
                .filter(|post| post.post.id != thread.id)
                .map(|post| post.post.id)
                .max(),
            thread,
            posts,
            omitted: 0,
            image_replies: 0,
        }],
        previous: String::new(),
        next: String::new(),
        catalog: false,
        catalog_options: catalog::Options::default(),
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
    // The fallback uses actual public routing, middleware and error rendering.
    // Closing a lazy pool makes storage unavailable without touching a service,
    // making a connection or loading any database credential.
    let unavailable = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused@127.0.0.1:1/unused")
        .unwrap();
    unavailable.close().await;
    let app = Router::new()
        .merge(board_public::themes::routes(
            "http://127.0.0.1:3000".into(),
            false,
        ))
        .merge(media_fixture.routes())
        .route(
            "/",
            get(|| async { Html(views::Home { boards: vec![] }.render().unwrap()) }),
        )
        .route("/empty/", get(|| async { Html(empty_page(false)) }))
        .route("/empty/catalog", get(|| async { Html(empty_page(true)) }))
        .route("/demo/", get(|| async { Html(page(false)) }))
        .route("/markup/", get(|| async { Html(render_page(false, true, false, false)) }))
        .route("/forced-anonymous/", get(|| async { Html(render_page(false, false, false, true)) }))
        .route("/text-only/", get(|| async { Html(render_page(false, false, true, false)) }))
        .route("/demo/catalog", get(|| async { Html(page(true)) }))
        .route("/text-catalog/catalog", get(|| async { Html(catalog_limits::text_page()) }))
        .route("/preview-pages/catalog", get(|| async { Html(catalog_limits::preview_pages()) }))
        .route("/demo/upload/fixture", get(|| async {
            Html(views::UploadPage {
                board: board(),
                form: views::UploadForm { upload_id: "1".repeat(32), upload_capability: "2".repeat(64), resto: 1000001 },
                ready: true,
                message: "Synthetic approved reply fixture; not a real attachment capability.",
            }.render().unwrap())
        }))
        .route(
            "/limits/catalog",
            get(|| async { Html(catalog_limits::page(false)) }),
        )
        .route(
            "/limits/text/catalog",
            get(|| async { Html(catalog_limits::page(true)) }),
        )
        .route("/arc/archive", get(|| async { Html(archive_page(false)) }))
        .route("/limits/sticky/catalog", get(|| async { Html(catalog_limits::flagged_page(true, false, false)) }))
        .route("/limits/permaage/catalog", get(|| async { Html(catalog_limits::flagged_page(false, true, false)) }))
        .route("/limits/undead/catalog", get(|| async { Html(catalog_limits::flagged_page(false, false, true)) }))
        .route(
            "/emptyarc/archive",
            get(|| async { Html(archive_page(true)) }),
        )
        .route(
            "/arc/thread/1000101",
            get(|| async { Html(archived_thread()) }),
        )
        .route("/readyz", get(|| async { "synthetic fixture renderer" }))
        .fallback_service(board_public::router(
            unavailable,
            "http://127.0.0.1:3000".into(),
            false,
        ));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    tokio::select! {
        result = axum::serve(listener, app) => result.unwrap(),
        result = media_server => { result.unwrap(); panic!("fixture media server stopped"); }
    }
}
