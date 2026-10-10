use super::{board, time, views};
use askama::Template;
use board_store::{Post, Thread};

// Synthetic, script-free pages rendered by the production Askama template.
// Each role appears on both an OP and a reply, with no posting authority.
pub fn page() -> String {
    page_with_worksafe(true)
}

pub fn page_with_worksafe(worksafe: bool) -> String {
    let capcodes = [
        "mod",
        "admin",
        "admin_highlight",
        "manager",
        "developer",
        "founder",
    ];
    let mut threads = Vec::new();
    for (index, capcode) in capcodes.into_iter().enumerate() {
        let id = 1_001_001 + index as i64 * 10;
        let post = Post {
            image_spoiler: false,
            comment_format: 0,
            staff_authorized_limits: false,
            wordfilter_payload: None,
            id,
            board: "demo".into(),
            thread_id: id,
            name: "Owned staff".into(),
            trip: None,
            poster_id: None,
            json_op_poster_id: None,
            capcode: Some(capcode.into()),
            country: None,
            country_name: None,
            board_flag: None,
            board_flag_type: "pol".into(),
            flag_name: None,
            subject: "Owned header subject".into(),
            comment: "Owned synthetic header text".into(),
            dice_result: None,
            fortune_text: None,
            fortune_color: None,
            drawing_time_seconds: None,
            drawing_source_post_id: None,
            created_at: time("2026-09-08T12:00:00Z"),
            deleted: false,
            attachment: None,
        };
        let reply = Post {
            id: id + 1,
            subject: String::new(),
            ..post.clone()
        };
        threads.push(views::ThreadView {
            catalog_position: None,
            catalog_last_reply: None,
            tail_size: 0,
            latest_reply_id: Some(id + 1),
            thread: Thread {
                id,
                board: "demo".into(),
                created_at: post.created_at,
                bumped_at: post.created_at,
                modified_at: post.created_at,
                http_modified_at: post.created_at,
                reply_count: 1,
                sticky: false,
                permasage: false,
                permaage: false,
                undead: false,
                closed: false,
                deleted: false,
                archived_at: None,
                archive_expires_at: None,
            },
            posts: vec![views::PostView::new(post), views::PostView::new(reply)],
            omitted: 0,
            image_replies: 0,
        });
    }
    let mut board = board();
    board.worksafe = worksafe;
    views::BoardPage {
        public_origin: String::new(),
        blotter: Vec::new(),
        spoiler_thumbnail: crate::views::spoilers::choose_thumbnail(&board),
        navigation_boards: crate::navigation_boards(),
        quote: String::new(),
        catalog_hidden: Vec::new(),
        board,
        threads,
        page_number: 1,
        parent: 0,
        previous: String::new(),
        next: String::new(),
        catalog: false,
        catalog_options: board_public::catalog::Options::default(),
        media_origin: String::new(),
    }
    .render()
    .expect("production header templates")
}
