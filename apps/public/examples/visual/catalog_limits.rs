use super::{board, catalog, time, views};
use askama::Template;
use board_store::{Post, Thread};

pub fn page(disabled_images: bool) -> String {
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
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (lifetime, visible, images, subject))| {
        let id = 1_000_400 + index as i64;
        views::ThreadView {
            thread: Thread {
                id,
                board: board.slug.clone(),
                created_at: time("2026-09-08T12:00:00Z"),
                bumped_at: time("2026-09-08T12:00:00Z"),
                modified_at: time("2026-09-08T12:00:00Z"),
                reply_count: lifetime,
                sticky: false,
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
