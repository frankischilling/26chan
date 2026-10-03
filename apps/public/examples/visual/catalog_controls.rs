use super::{board, catalog, time, views};
use askama::Template;
use board_store::{Post, Thread};

pub fn page() -> String {
    let mut board = board();
    board.slug = "controlui".into();
    let threads = [
        (1_000_001, "Owned crane", "One sheet"),
        (1_000_002, "Owned boat", "Two folds"),
    ]
    .into_iter()
    .map(|(id, subject, comment)| views::ThreadView {
        catalog_last_reply: None,
        tail_size: 0,
        latest_reply_id: None,
        thread: Thread {
            id,
            board: board.slug.clone(),
            created_at: time("2026-09-08T12:00:00Z"),
            bumped_at: time("2026-09-08T12:00:00Z"),
            modified_at: time("2026-09-08T12:00:00Z"),
            http_modified_at: time("2026-09-08T12:00:00Z"),
            reply_count: 0,
            sticky: false,
            permasage: false,
            permaage: false,
            undead: false,
            closed: false,
            deleted: false,
            archived_at: None,
            archive_expires_at: None,
        },
        posts: vec![views::PostView::new(Post {
            image_spoiler: false,
            comment_format: 0,
            staff_authorized_limits: false,
            wordfilter_payload: None,
            id,
            board: board.slug.clone(),
            thread_id: id,
            name: "Anonymous".into(),
            trip: None,
            poster_id: None,
            json_op_poster_id: None,
            capcode: None,
            country: None,
            country_name: None,
            board_flag: None,
            flag_name: None,
            subject: subject.into(),
            comment: comment.into(),
            dice_result: None,
            fortune_text: None,
            fortune_color: None,
            created_at: time("2026-09-08T12:00:00Z"),
            deleted: false,
            attachment: None,
        })],
        omitted: 0,
        image_replies: 0,
    })
    .collect();
    views::BoardPage {
        navigation_boards: crate::navigation_boards(),
        quote: String::new(),
        catalog_hidden: Vec::new(),
        board,
        threads,
        parent: 0,
        previous: String::new(),
        next: String::new(),
        catalog: true,
        catalog_options: catalog::Options::default(),
        media_origin: String::new(),
    }
    .render()
    .expect("production catalog controls template")
}
