//! Read-only controlled-origin math qualification with production template and CSP.
#![forbid(unsafe_code)]
use axum::{Router, routing::get};
use board_public::{
    catalog,
    views::{BoardPage, PostView, ThreadView},
};
use board_store::{Board, Post, Thread};
use chrono::{DateTime, Utc};
const ORIGIN: &str = "http://127.0.0.1:3000";
fn time(value: &str) -> DateTime<Utc> {
    value.parse().expect("synthetic timestamp")
}
fn board() -> Board {
    Board {
        source_order: 1000,
        catalog_enabled: true,
        json_enabled: true,
        staff_only: false,
        meta_board: false,
        upload_board: false,
        rss_enabled: true,
        slug: "sci".into(),
        title: "Paper craft".into(),
        description: "Discuss paper models, folding, and works in progress.".into(),
        max_comment_chars: 4000,
        max_authorized_comment_chars: 10000,
        comment_code_spacing: false,
        comment_sjis_spacing: false,
        math_tags: true,
        oekaki: false,
        oekaki_replays: false,
        oekaki_width: 400,
        oekaki_height: 400,
        comment_max_lines: 70,
        comment_spoiler_cleanup: false,
        custom_spoiler_count: 0,
        spoiler_thumbnail_assets: vec!["spoiler.png".into()],
        require_subject: false,
        op_markup: false,
        forced_anon: false,
        strip_tripcode: false,
        user_ids: false,
        country_flags: false,
        board_flags: vec![],
        board_flag_type: "pol".into(),
        text_only: false,
        reply_limit: 100,
        bump_limit: 75,
        permasage_hours: 0,
        posting_reply_seconds: 0,
        posting_image_seconds: 0,
        posting_thread_seconds: 0,
        user_thread_limit: 5,
        user_thread_period_hours: 24,
        op_bump_limit: true,
        op_bump_initial_seconds: 900,
        op_bump_repeat_seconds: 300,
        thread_limit: 100,
        expire_neglected: true,
        threads_per_page: 10,
        worksafe: true,
        archive_retention_seconds: 0,
        archive_limit: 1000,
        image_limit: 0,
        dice_roll: false,
        fortune_trip: false,
        robot9000: false,
        robot9000_state_limit: 100000,
        word_filter_enabled: false,
        word_filter_profile: 0,
    }
}
fn fixture(enabled: bool, empty: bool, catalog: bool, index: bool) -> axum::response::Response {
    let mut board = board();
    board.math_tags = enabled;
    if !enabled {
        board.slug = "test".into();
    }
    let comment = if empty {
        "Literal source with no equation."
    } else {
        r"Before [math]x^2+\frac{1}{2}[/math] after
[eqn]\sum_{i=1}^{n} i=\frac{n(n+1)}{2}[/eqn]"
    };
    let thread = Thread {
        id: 1000001,
        board: board.slug.clone(),
        created_at: time("2026-09-08T12:00:00Z"),
        bumped_at: time("2026-09-08T12:05:00Z"),
        modified_at: time("2026-09-08T12:05:00Z"),
        http_modified_at: time("2026-09-08T12:06:00Z"),
        reply_count: if index { 2 } else { 1 },
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
        image_spoiler: false,
        comment_format: 0,
        staff_authorized_limits: false,
        wordfilter_payload: None,
        id: 1000001,
        board: board.slug.clone(),
        thread_id: 1000001,
        name: "Anonymous".into(),
        trip: None,
        poster_id: None,
        json_op_poster_id: None,
        capcode: None,
        country: None,
        country_name: None,
        board_flag: None,
        board_flag_type: "pol".into(),
        flag_name: None,
        subject: "What are you making?".into(),
        comment: comment.into(),
        dice_result: None,
        fortune_text: None,
        fortune_color: None,
        created_at: time("2026-09-08T12:00:00Z"),
        deleted: false,
        attachment: None,
    })];
    if !empty {
        posts.push(PostView::new(Post {
            id: 1000003,
            subject: String::new(),
            comment: ">>1000001\nReply with [math]y^2[/math]".into(),
            ..posts[0].post.clone()
        }));
    }
    let page = BoardPage {
        spoiler_thumbnail: "spoiler.png".into(),
        navigation_boards: vec![board.clone()],
        board,
        quote: String::new(),
        catalog_hidden: vec![],
        threads: vec![ThreadView {
            catalog_position: Some(0),
            catalog_last_reply: None,
            tail_size: 0,
            latest_reply_id: None,
            thread,
            posts,
            omitted: usize::from(index),
            image_replies: 0,
        }],
        parent: if index { 0 } else { 1000001 },
        previous: String::new(),
        next: String::new(),
        catalog,
        catalog_options: catalog::Options::default(),
        media_origin: String::new(),
    };
    board_public::math_fixture_response(&page, ORIGIN).expect("production math response")
}
#[tokio::main]
async fn main() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused@127.0.0.1:1/unused")
        .expect("lazy closed pool");
    pool.close().await;
    let app = Router::new()
        .route("/readyz", get(|| async { "ready" }))
        .route(
            "/sci/thread/1000001",
            get(|| async { fixture(true, false, false, false) }),
        )
        .route(
            "/sci/thread/1000002",
            get(|| async { fixture(true, true, false, false) }),
        )
        .route(
            "/test/thread/1000001",
            get(|| async { fixture(false, false, false, false) }),
        )
        .route("/sci/", get(|| async { fixture(true, false, false, true) }))
        .route(
            "/sci/catalog",
            get(|| async { fixture(true, false, true, false) }),
        )
        .fallback_service(board_public::router(pool, ORIGIN.into(), false));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .expect("fixture listener");
    axum::serve(listener, app).await.expect("fixture server");
}
