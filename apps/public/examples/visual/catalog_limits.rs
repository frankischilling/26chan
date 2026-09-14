use super::{board, catalog, time, views};
use askama::Template;
use board_store::{Post, Thread};

pub fn page(disabled_images: bool) -> String {
    render(disabled_images, None)
}

pub fn flagged_page(sticky: bool, permaage: bool, undead: bool) -> String {
    render(false, Some((sticky, permaage, undead)))
}

fn render(disabled_images: bool, flags: Option<(bool, bool, bool)>) -> String {
    let mut board = board();
    board.slug = "limits".into();
    board.title = "Catalog limits".into();
    board.description = "Synthetic reply and image-count states.".into();
    board.bump_limit = 2;
    board.image_limit = if disabled_images { 0 } else { 2 };
    let threads = [
        (1, 1, 1, "Below both limits"),
        (2, 2, 2, "Both limits reached"),
        (3, 1, 1, "Replies deleted after bump limit"),
        (3, 3, 3, "Policy lowered below counts"),
        (0, 0, 0, "No replies"),
        (3, 3, 0, "Sticky above bump limit"),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (lifetime, visible, images, subject))| {
        let id = 1_000_400 + index as i64;
        views::ThreadView {
            tail_size: 0,
            latest_reply_id: (visible > 0).then_some(id + visible as i64),
            thread: Thread {
                id,
                board: board.slug.clone(),
                created_at: time("2026-09-08T12:00:00Z"),
                bumped_at: time("2026-09-08T12:00:00Z"),
                modified_at: time("2026-09-08T12:00:00Z"),
                reply_count: lifetime,
                sticky: flags.map_or(index == 5, |f| f.0),
                permasage: false,
                permaage: flags.is_some_and(|f| f.1),
                undead: flags.is_some_and(|f| f.2),
                closed: false,
                deleted: false,
                archived_at: None,
                archive_expires_at: None,
            },
            posts: vec![views::PostView::new(Post {
                id,
                board: board.slug.clone(),
                thread_id: id,
                name: "Anonymous".into(),
                subject: subject.into(),
                comment: "Synthetic counter fixture.".into(),
                created_at: time("2026-09-08T12:00:00Z"),
                deleted: false,
                attachment: None,
            })],
            omitted: visible,
            image_replies: images,
        }
    })
    .collect();
    views::BoardPage {
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
    .expect("production catalog limit template")
}
