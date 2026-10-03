use super::{board, catalog, time, views};
use askama::Template;
use axum::{Router, response::Html, routing::get};
use board_media::{ApprovedFiles, PublicationStore, Quarantine, ValidatedOutput};
use board_store::{Post, Thread, post_media::PostAttachment};

fn page(slug: &str, sorted: bool) -> String {
    let mut board = board();
    board.slug = slug.into();
    let mut threads = [
        (
            1_000_001,
            "Owned crane",
            "One sheet",
            "Avery",
            Some("!Origami"),
            None,
            Some("crane.png"),
            2,
        ),
        (
            1_000_002,
            "Owned boat",
            "Two folds",
            "Riley",
            Some("!Paper"),
            Some("mod"),
            Some("boat.png"),
            1,
        ),
        (
            1_000_003,
            "Feeling fold",
            "paper feeling",
            "Anonymous",
            None,
            None,
            None,
            0,
        ),
    ]
    .into_iter()
    .map(
        |(id, subject, comment, name, trip, capcode, filename, replies)| views::ThreadView {
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
                reply_count: replies,
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
                name: name.into(),
                trip: trip.map(str::to_owned),
                poster_id: None,
                json_op_poster_id: None,
                capcode: capcode.map(str::to_owned),
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
                attachment: filename.map(|filename| PostAttachment {
                    post_id: id,
                    asset_id: format!("{id:032x}"),
                    filename: filename.into(),
                    bytes: 70,
                    width: 1,
                    height: 1,
                    spoiler: false,
                    file_deleted: false,
                    tim: id,
                    md5: None,
                    thumbnail_width: None,
                    thumbnail_height: None,
                }),
            })],
            omitted: replies as usize,
            image_replies: 0,
        },
    )
    .collect::<Vec<_>>();
    if sorted {
        threads.sort_by_key(|view| std::cmp::Reverse(view.thread.id));
    }
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
        media_origin: "http://127.0.0.1:3000".into(),
    }
    .render()
    .expect("production catalog controls template")
}

pub async fn routes() -> Router {
    // Trusted owned RGBA bytes use the same bounded PNG encoder as the other
    // visual fixtures; this example never accepts an uploaded file.
    let directory = tempfile::tempdir().unwrap();
    let quarantine = Quarantine::new(directory.path().join("quarantine")).unwrap();
    let store = PublicationStore::new(directory.path().join("objects"), &quarantine).unwrap();
    let reader = ApprovedFiles::open(directory.path().join("objects")).unwrap();
    let guard = store.try_lock().unwrap();
    let mut frame = b"IBRGBA01".to_vec();
    frame.extend_from_slice(&1u32.to_be_bytes());
    frame.extend_from_slice(&1u32.to_be_bytes());
    frame.extend_from_slice(&[45, 112, 142, 255]);
    let output = ValidatedOutput::read(frame.as_slice())
        .await
        .unwrap()
        .encode()
        .unwrap();
    let id = "00000000000000000000000000000001".parse().unwrap();
    guard.install(id, &output).unwrap();
    let bytes = reader.read(id, output.sha256(), output.len()).unwrap();
    let mut router = Router::new()
        .route(
            "/filterui/catalog",
            get(|| async { Html(page("filterui", false)) }),
        )
        .route(
            "/settingsui/catalog",
            get(|| async { Html(page("settingsui", true)) }),
        );
    for (slug, id) in ["filterui", "settingsui"]
        .into_iter()
        .flat_map(|slug| [1_000_001, 1_000_002].map(|id| (slug, id)))
    {
        let bytes = bytes.clone();
        router = router.route(
            &format!("/{slug}/{id}.png"),
            get(move || {
                let bytes = bytes.clone();
                async move { ([("content-type", "image/png")], bytes) }
            }),
        );
    }
    router
}
