use super::{board, catalog, time, views};
use askama::Template;
use board_store::{Post, Thread};

pub fn page(disabled_images: bool) -> String {
    render(disabled_images, None, false, false)
}

pub fn text_page() -> String {
    render(true, None, true, false)
}

pub fn preview_pages() -> String {
    render(false, None, false, true)
}

pub fn flagged_page(sticky: bool, permaage: bool, undead: bool) -> String {
    render(false, Some((sticky, permaage, undead)), false, false)
}

fn render(
    disabled_images: bool,
    flags: Option<(bool, bool, bool)>,
    text_only: bool,
    preview_pages: bool,
) -> String {
    let mut board = board();
    board.slug = "limits".into();
    board.text_only = text_only;
    if text_only {
        board.slug = "text-catalog".into();
    }
    if preview_pages {
        board.slug = "preview-pages".into();
        board.threads_per_page = 2;
    }
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
            catalog_last_reply: (visible > 0).then(|| board_store::CatalogReply {
                thread_id: id,
                id: id + visible as i64,
                name: "Synthetic reply author".into(),
                created_at: time("2026-09-08T12:05:00Z"),
            }),
            tail_size: 0,
            latest_reply_id: (visible > 0).then_some(id + visible as i64),
            thread: Thread {
                id,
                board: board.slug.clone(),
                created_at: time("2026-09-08T12:00:00Z"),
                bumped_at: time("2026-09-08T12:00:00Z"),
                modified_at: time("2026-09-08T12:00:00Z"),
                http_modified_at: time("2026-09-08T12:00:00Z"),
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
                comment_format: 0,
                id,
                board: board.slug.clone(),
                thread_id: id,
                name: "Anonymous".into(),
                subject: if text_only && index == 4 {
                    "<script>literal & subject</script>".into()
                } else {
                    subject.into()
                },
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
