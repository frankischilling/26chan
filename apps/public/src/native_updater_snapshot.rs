//! Release-owned rendering projection, not a replacement for the public JSON API.
use crate::{
    AppState,
    handlers::AppError,
    views::{PostFragment, PostView, ThreadView},
};
use askama::Template;
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode, Uri},
    response::Response,
};
use serde::Serialize;

const MAX_POSTS: usize = 1001;
const MAX_BYTES: usize = 4_194_304;

#[derive(Serialize)]
struct Snapshot {
    version: u8,
    board: String,
    thread: String,
    closed: bool,
    archived: bool,
    sticky: bool,
    replies: usize,
    images: usize,
    posts: Vec<RenderedPost>,
    tail_size: usize,
    tail_id: Option<String>,
}

#[derive(Serialize)]
struct RenderedPost {
    no: String,
    file_deleted: bool,
    html: String,
}

// Both rendering and JSON escaping consume a hard byte budget. No partially
// rendered/serialized success is returned when either writer reaches its limit.
struct LimitedOutput {
    bytes: Vec<u8>,
    limit: usize,
}

impl LimitedOutput {
    fn new(limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            limit,
        }
    }
}

impl std::io::Write for LimitedOutput {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > self.limit.saturating_sub(self.bytes.len()) {
            return Err(std::io::Error::other("updater response exceeds byte limit"));
        }
        self.bytes.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl std::fmt::Write for LimitedOutput {
    fn write_str(&mut self, value: &str) -> std::fmt::Result {
        std::io::Write::write_all(self, value.as_bytes()).map_err(|_| std::fmt::Error)
    }
}

fn unavailable() -> AppError {
    AppError(
        StatusCode::SERVICE_UNAVAILABLE,
        "Thread update is unavailable. Open the thread page to continue.",
    )
}

fn encode(
    snapshot: board_store::ThreadSnapshot,
    media_origin: &str,
    limit: usize,
) -> Result<Vec<u8>, AppError> {
    let board_store::ThreadSnapshot {
        board,
        thread,
        posts,
        replies,
        images,
        tail_size,
        tail_id,
    } = snapshot;
    if posts.is_empty()
        || posts.len() > MAX_POSTS
        || thread.id <= 0
        || thread.deleted
        || thread.board != board.slug
        || posts[0].id != thread.id
        || posts
            .iter()
            .any(|post| post.deleted || post.board != board.slug || post.thread_id != thread.id)
        || posts.windows(2).any(|pair| pair[0].id >= pair[1].id)
        || replies > 1000
        || images > replies
        || tail_size > 1000
        || (tail_size > 0 && replies < tail_size * 2)
        || posts.len()
            != 1 + if tail_id.is_some() {
                tail_size
            } else {
                replies
            }
        || tail_id.is_some_and(|boundary| {
            tail_size == 0 || boundary <= thread.id || posts.get(1).is_none_or(|p| p.id <= boundary)
        })
    {
        return Err(unavailable());
    }
    let omitted = replies - (posts.len() - 1);
    let view = ThreadView {
        tail_size,
        latest_reply_id: posts
            .last()
            .filter(|post| post.id != thread.id)
            .map(|post| post.id),
        thread,
        posts: posts.into_iter().map(PostView::new).collect(),
        omitted,
        image_replies: images as i64,
    };
    let mut rendered = Vec::with_capacity(view.posts.len());
    let mut remaining = limit;
    for item in &view.posts {
        let mut output = LimitedOutput::new(remaining);
        PostFragment {
            item,
            view: &view,
            board: &board,
            media_origin,
            catalog: false,
        }
        .render_into(&mut output)
        .map_err(|_| unavailable())?;
        remaining -= output.bytes.len();
        rendered.push(RenderedPost {
            no: item.post.id.to_string(),
            file_deleted: item
                .post
                .attachment
                .as_ref()
                .is_some_and(|file| file.file_deleted),
            html: String::from_utf8(output.bytes).map_err(|_| unavailable())?,
        });
    }
    let result = Snapshot {
        version: 2,
        board: board.slug,
        thread: view.thread.id.to_string(),
        closed: view.thread.closed,
        archived: view.thread.archived_at.is_some(),
        sticky: view.thread.sticky,
        replies,
        images,
        posts: rendered,
        tail_size,
        tail_id: tail_id.map(|id| id.to_string()),
    };
    let mut output = LimitedOutput::new(limit);
    serde_json::to_writer(&mut output, &result).map_err(|_| unavailable())?;
    Ok(output.bytes)
}

pub(crate) async fn get(
    State(state): State<AppState>,
    Path((board, key)): Path<(String, String)>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    selected(state, board, key, uri, headers, false).await
}

pub(crate) async fn get_tail(
    State(state): State<AppState>,
    Path((board, key)): Path<(String, String)>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    selected(state, board, key, uri, headers, true).await
}

async fn selected(
    state: AppState,
    board: String,
    key: String,
    uri: Uri,
    headers: HeaderMap,
    tail: bool,
) -> Result<Response, AppError> {
    if uri.query().is_some() {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "Invalid thread update options.",
        ));
    }
    let id = key
        .parse::<i64>()
        .ok()
        .filter(|id| *id > 0 && id.to_string() == key)
        .ok_or(AppError(StatusCode::NOT_FOUND, "Thread not found."))?;
    let snapshot = board_store::thread_snapshot_selection(&state.pool, &board, id, tail).await?;
    let modified = snapshot.thread.http_modified_at;
    let media_origin = state
        .media
        .as_ref()
        .map(|media| media.settings.origin.as_string())
        .unwrap_or_default();
    let bytes = encode(snapshot, &media_origin, MAX_BYTES)?;
    crate::api::bytes_response(bytes, Some(modified), &headers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use board_store::{Board, Post, Thread, ThreadSnapshot, post_media::PostAttachment};

    fn fixture() -> ThreadSnapshot {
        let now = chrono::DateTime::from_timestamp(1_767_225_600, 0).unwrap();
        let id = i64::MAX - 1;
        let board = Board {
            slug: "test".into(),
            title: "Test".into(),
            description: String::new(),
            max_comment_chars: 16_000,
            comment_code_spacing: true,
            comment_sjis_spacing: false,
            comment_max_lines: 100,
            comment_spoiler_cleanup: true,
            require_subject: false,
            op_markup: false,
            forced_anon: false,
            text_only: false,
            reply_limit: 1000,
            bump_limit: 300,
            permasage_hours: 0,
            op_bump_limit: true,
            op_bump_initial_seconds: 900,
            op_bump_repeat_seconds: 300,
            thread_limit: 10,
            threads_per_page: 10,
            worksafe: true,
            archive_retention_seconds: 0,
            archive_limit: 0,
            image_limit: 100,
        };
        let thread = Thread {
            id,
            board: board.slug.clone(),
            created_at: now,
            bumped_at: now,
            modified_at: now,
            http_modified_at: now,
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
        let posts = [id, id + 1]
            .into_iter()
            .map(|no| Post {
                comment_format: 0,
                id: no,
                board: board.slug.clone(),
                thread_id: id,
                name: "<img src=x onerror=alert(1)>".into(),
                subject: "<script>subject</script>".into(),
                comment: "<script>alert(1)</script>\n>>9223372036854775806".into(),
                created_at: now,
                deleted: false,
                attachment: None,
            })
            .collect();
        ThreadSnapshot {
            board,
            thread,
            posts,
            replies: 1,
            images: 0,
            tail_size: 0,
            tail_id: None,
        }
    }

    fn attachment() -> PostAttachment {
        PostAttachment {
            post_id: i64::MAX,
            asset_id: "private-asset-id".into(),
            filename: "<img>.jpg".into(),
            bytes: 100,
            width: 32,
            height: 24,
            spoiler: false,
            file_deleted: false,
            tim: i64::MAX,
            md5: Some("not-a-public-hash-field".into()),
            thumbnail_width: Some(32),
            thumbnail_height: Some(24),
        }
    }

    #[test]
    fn exact_identifiers_and_shared_escaped_markup() {
        let bytes = encode(fixture(), "", MAX_BYTES).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["version"], 2);
        assert_eq!(value["thread"], "9223372036854775806");
        assert_eq!(value["posts"][1]["no"], "9223372036854775807");
        assert_eq!(value["replies"], 1);
        let html = value["posts"][1]["html"].as_str().unwrap();
        assert!(!html.contains("class=\"subject\""));
        assert!(
            value["posts"][0]["html"]
                .as_str()
                .unwrap()
                .contains("class=\"subject\">&#60;script&#62;subject&#60;/script&#62;</span>")
        );
        assert!(html.contains("id=\"pc9223372036854775807\""));
        assert!(html.contains("/test/thread/9223372036854775806#p9223372036854775807"));
        assert!(html.contains("action=\"/test/delete\""));
        assert!(html.contains("action=\"/test/report\""));
        assert!(!html.contains("<script>"));
        assert!(!html.contains("<img src=x"));
        assert!(!html.contains("value=\"password"));
        assert!(value.get("password_hash").is_none());
    }

    #[test]
    fn snapshot_uses_each_post_stamp_not_current_board_policy() {
        for format in [0, 8, 9, 15] {
            let mut snapshot = fixture();
            snapshot.board.comment_spoiler_cleanup = format == 8;
            snapshot.posts[1].comment_format = format;
            snapshot.posts[1].comment = "[spoiler]<b>first</b>\nsecond[/spoiler]".into();
            let expected = crate::views::Comment {
                lines: &board_domain::parse_post_comment(&snapshot.posts[1].comment, format),
                board: "test",
            }
            .render()
            .unwrap();
            let bytes = encode(snapshot, "", MAX_BYTES).unwrap();
            let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            let html = value["posts"][1]["html"].as_str().unwrap();
            assert!(html.contains(&format!(
                "id=\"m9223372036854775807\">{expected}</blockquote>"
            )));
            assert!(!html.contains("<b>first"));
            assert_eq!(html.contains("<s>"), format & 1 != 0);
        }
    }

    #[test]
    fn media_projection_handles_normal_spoiler_deleted_and_disabled_media() {
        for (origin, spoiler, deleted) in [
            ("https://media.example", false, false),
            ("https://media.example", true, false),
            ("https://media.example", false, true),
            ("", false, false),
        ] {
            let mut snapshot = fixture();
            let mut file = attachment();
            file.spoiler = spoiler;
            file.file_deleted = deleted;
            snapshot.posts[1].attachment = Some(file);
            snapshot.images = usize::from(!deleted);
            snapshot.thread.closed = true;
            snapshot.thread.sticky = true;
            snapshot.thread.archived_at = Some(snapshot.thread.created_at);
            let bytes = encode(snapshot, origin, MAX_BYTES).unwrap();
            let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(value["closed"], true);
            assert_eq!(value["archived"], true);
            assert_eq!(value["sticky"], true);
            assert_eq!(value["images"], usize::from(!deleted));
            assert_eq!(value["posts"][1]["file_deleted"], deleted);
            let html = value["posts"][1]["html"].as_str().unwrap();
            assert_eq!(html.contains("File deleted."), deleted);
            assert_eq!(
                html.contains("Spoiler image"),
                spoiler && !deleted && !origin.is_empty()
            );
            assert_eq!(
                html.contains("<img "),
                !spoiler && !deleted && !origin.is_empty()
            );
            assert_eq!(
                html.contains("https://media.example/test/9223372036854775807.png"),
                !deleted && !origin.is_empty()
            );
            assert!(!html.contains("private-asset-id"));
            assert!(!html.contains("not-a-public-hash-field"));
        }
    }

    #[test]
    fn tail_projection_preserves_full_counts_and_exact_large_boundary_ids() {
        let mut snapshot = fixture();
        let base = i64::MAX - 10;
        snapshot.thread.id = base;
        snapshot.thread.reply_count = 6;
        let template = snapshot.posts[0].clone();
        snapshot.posts = [0, 5, 6]
            .into_iter()
            .map(|offset| {
                let mut post = template.clone();
                post.id = base + offset;
                post.thread_id = base;
                post
            })
            .collect();
        snapshot.replies = 6;
        snapshot.tail_size = 2;
        snapshot.tail_id = Some(base + 4);
        let bytes = encode(snapshot, "", MAX_BYTES).unwrap();
        let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["tail_id"], (base + 4).to_string());
        assert_eq!(value["replies"], 6);
        assert_eq!(value["posts"].as_array().unwrap().len(), 3);
        assert_eq!(value["posts"][1]["no"], (base + 5).to_string());
    }

    #[test]
    fn malformed_or_oversized_snapshots_fail_without_partial_success() {
        for variant in 0..7 {
            let mut snapshot = fixture();
            match variant {
                0 => snapshot.posts.clear(),
                1 => snapshot.posts[1].id = snapshot.posts[0].id,
                2 => snapshot.posts[0].deleted = true,
                3 => snapshot.posts[1].thread_id = 1,
                4 => snapshot.posts[1].board = "other".into(),
                5 => snapshot.posts = vec![snapshot.posts[0].clone(); MAX_POSTS + 1],
                _ => snapshot.thread.deleted = true,
            }
            assert_eq!(
                encode(snapshot, "", MAX_BYTES).unwrap_err().0,
                StatusCode::SERVICE_UNAVAILABLE
            );
        }
        let bytes = encode(fixture(), "", MAX_BYTES).unwrap();
        assert!(encode(fixture(), "", bytes.len()).is_ok());
        assert!(encode(fixture(), "", bytes.len() - 1).is_err());
        assert!(encode(fixture(), "", 8).is_err());
    }

    #[test]
    fn limits_count_utf8_and_json_escaping_without_truncation() {
        let mut writer = LimitedOutput::new(4);
        std::fmt::Write::write_str(&mut writer, "\u{1f642}").unwrap();
        assert!(std::fmt::Write::write_str(&mut writer, "x").is_err());
        assert_eq!(writer.bytes.len(), 4);
        let mut writer = LimitedOutput::new(3);
        assert!(serde_json::to_writer(&mut writer, "\n").is_err());
        assert!(writer.bytes.len() <= 3);
    }
}
