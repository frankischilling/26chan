//! Synthetic pixels and metadata for production-template layout regression.
use super::{board, time, views};
use askama::Template;
use axum::{Router, response::Html, routing::get};
use board_media::{ApprovedFiles, PublicationStore, Quarantine, ValidatedOutput};
use board_store::{Post, Thread, post_media::PostAttachment};

#[derive(Clone)]
pub struct Fixture {
    files: Vec<PostAttachment>,
}

impl Fixture {
    pub async fn build() -> (Self, Router) {
        let directory = tempfile::tempdir().unwrap();
        let quarantine = Quarantine::new(directory.path().join("quarantine")).unwrap();
        let store = PublicationStore::new(directory.path().join("objects"), &quarantine).unwrap();
        let reader = ApprovedFiles::open(directory.path().join("objects")).unwrap();
        let guard = store.try_lock().unwrap();
        let mut files = vec![];
        let mut media = Router::new();
        for (index, (width, height)) in [(600u32, 360u32), (240, 600), (48, 32), (600, 360)]
            .into_iter()
            .enumerate()
        {
            let mut frame = b"IBRGBA01".to_vec();
            frame.extend_from_slice(&width.to_be_bytes());
            frame.extend_from_slice(&height.to_be_bytes());
            for y in 0..height {
                for x in 0..width {
                    let fold = x * height > y * width;
                    frame.extend_from_slice(if fold {
                        &[45, 112, 142, 255]
                    } else {
                        &[238, 181, 77, 255]
                    });
                }
            }
            let output = ValidatedOutput::read(frame.as_slice()).await.unwrap();
            let full = output.encode().unwrap();
            let thumbnail = output.thumbnail().unwrap();
            let id = format!("{:032x}", index + 1).parse().unwrap();
            guard.install(id, &full).unwrap();
            guard.install_thumbnail(id, &thumbnail).unwrap();
            let bytes = reader.read(id, full.sha256(), full.len()).unwrap();
            let thumb = reader
                .read_thumbnail(id, thumbnail.sha256(), thumbnail.len())
                .unwrap();
            let tim = 1_000_201 + index as i64;
            media = media
                .route(
                    &format!("/img/{tim}.png"),
                    get(move || {
                        let bytes = bytes.clone();
                        async move { ([("content-type", "image/png")], bytes) }
                    }),
                )
                .route(
                    &format!("/img/{tim}s.jpg"),
                    get(move || {
                        let thumb = thumb.clone();
                        async move { ([("content-type", "image/png")], thumb) }
                    }),
                );
            files.push(PostAttachment {
                post_id: tim,
                asset_id: id.to_string(),
                filename: if index == 0 {
                    format!("<b>fold & \"roof\"</b>-{}.png", "paper".repeat(20))
                } else {
                    format!("fold-{width}x{height}.png")
                },
                bytes: full.len() as i64,
                width: width as i32,
                height: height as i32,
                spoiler: false,
                file_deleted: false,
                tim,
                md5: (index != 3).then(|| full.md5().to_owned()),
                thumbnail_width: (index != 3).then_some(thumbnail.dimensions().0 as i32),
                thumbnail_height: (index != 3).then_some(thumbnail.dimensions().1 as i32),
            });
        }
        // No media route exists for hidden or removed fixtures. Browser tests
        // require zero requests for these IDs even after opening spoiler details.
        files.push(PostAttachment {
            post_id: 1_000_205,
            tim: 1_000_205,
            spoiler: true,
            filename: "hidden-fold.png".into(),
            ..files[0].clone()
        });
        files.push(PostAttachment {
            post_id: 1_000_206,
            tim: 1_000_206,
            file_deleted: true,
            filename: "removed-fold.png".into(),
            ..files[0].clone()
        });
        (Self { files }, media)
    }

    fn page(&self, kind: &str) -> String {
        let catalog = kind == "catalog";
        let archived = kind == "archived";
        let board = board_store::Board {
            slug: "img".into(),
            title: "Paper image fixtures".into(),
            description: "Synthetic folds, thumbnails and file states.".into(),
            image_limit: 10,
            archive_retention_seconds: 3600,
            ..board()
        };
        let thread = Thread {
            id: 1_000_201,
            board: board.slug.clone(),
            created_at: time("2026-09-08T12:00:00Z"),
            bumped_at: time("2026-09-08T12:05:00Z"),
            modified_at: time("2026-09-08T12:05:00Z"),
            reply_count: 5,
            sticky: false,
            closed: false,
            deleted: false,
            archived_at: archived.then(|| time("2026-09-08T13:00:00Z")),
            archive_expires_at: archived.then(|| time("2026-09-08T14:00:00Z")),
        };
        let posts: Vec<_> = self.files.iter().enumerate().map(|(index, file)| views::PostView::new(Post {
            id: file.post_id,
            board: board.slug.clone(),
            thread_id: if catalog { file.post_id } else { thread.id },
            name: "Anonymous".into(),
            subject: ["Landscape fold", "Portrait fold", "Small fold", "Legacy full-image preview", "Spoiler fold", "Removed file"][index].into(),
            comment: "Synthetic paper fold.\n>the original bytes stay unchanged\n[spoiler]A hidden crease.[/spoiler]".into(),
            created_at: thread.created_at,
            deleted: false,
            attachment: Some(file.clone()),
        })).collect();
        let threads = if catalog {
            posts
                .into_iter()
                .map(|post| views::ThreadView {
                    thread: Thread {
                        id: post.post.id,
                        reply_count: 0,
                        ..thread.clone()
                    },
                    posts: vec![post],
                    omitted: 0,
                    image_replies: 0,
                })
                .collect()
        } else {
            vec![views::ThreadView {
                thread,
                posts,
                omitted: 0,
                image_replies: 0,
            }]
        };
        views::BoardPage {
            board,
            threads,
            parent: if kind == "thread" || archived {
                1_000_201
            } else {
                0
            },
            previous: String::new(),
            next: String::new(),
            catalog,
            media_origin: "http://localhost:3004".into(),
        }
        .render()
        .unwrap()
    }

    pub fn routes(self) -> Router {
        let mut app = Router::new();
        for (path, kind) in [
            ("/img/", "board"),
            ("/img/thread/1000201", "thread"),
            ("/img/catalog", "catalog"),
            ("/img/archived/1000201", "archived"),
        ] {
            let fixture = self.clone();
            app = app.route(
                path,
                get(move || {
                    let fixture = fixture.clone();
                    async move { Html(fixture.page(kind)) }
                }),
            );
        }
        app
    }
}
