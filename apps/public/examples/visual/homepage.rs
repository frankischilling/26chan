//! Owned synthetic content through the production homepage template.
use askama::Template;
use board_store::homepage::{Card, Snapshot, Statistics};
pub fn page() -> String {
    let source: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../fixtures/navigation-reference.json"
    ))
    .unwrap();
    let mut boards: Vec<_> = source["directory_labels"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            let mut board = super::board();
            board.slug = entry[0].as_str().unwrap().into();
            board.title = entry[1].as_str().unwrap().into();
            board
        })
        .collect();
    let mut custom = super::board();
    custom.slug = "ownedcustom".into();
    custom.title = "Owned custom <board>".into();
    boards.push(custom);
    let date = super::time("2020-01-02T03:04:00Z");
    let cards = ["popular", "latest", "images"]
        .iter()
        .map(|kind| Card {
            kind: (*kind).into(),
            slot: 1,
            id: 123,
            board: "g".into(),
            thread_id: 123,
            board_title: "Technology".into(),
            subject: "Owned thread <title>".into(),
            excerpt: "A synthetic conversation about old computers.\n<script> stays plain text."
                .into(),
            created_at: date,
            reply_count: 18,
            image_count: 2,
            tim: None,
            bytes: None,
            thumbnail_width: None,
            thumbnail_height: None,
        })
        .collect();
    super::views::Home {
        event_workflows: false,
        boards,
        media_origin: String::new(),
        news: vec![board_store::site_content::NewsItem {
            id: 1,
            published_at: date,
            title: "Owned local news".into(),
            body: "This fixture contains synthetic content only.".into(),
            author: "Local editor".into(),
        }],
        snapshot: Snapshot {
            statistics: Some(Statistics {
                generation: 1,
                generated_at: date,
                recorded_posts: 1234,
                active_content_bytes: 567890,
                active_sessions: 12,
            }),
            cards,
        },
    }
    .render()
    .unwrap()
}
