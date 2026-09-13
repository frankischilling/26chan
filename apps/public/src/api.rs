use crate::{
    AppState,
    handlers::AppError,
    views::{Comment, PostView},
};
use askama::Template;
use axum::{
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
};
use board_store::{Board, Post, Thread};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

fn response(
    value: Value,
    modified: Option<DateTime<Utc>>,
    headers: &HeaderMap,
) -> Result<Response, AppError> {
    let bytes = serde_json::to_vec(&value).map_err(|_| {
        AppError(
            StatusCode::INTERNAL_SERVER_ERROR,
            "JSON could not be rendered.",
        )
    })?;
    let etag = format!("\"{:x}\"", Sha256::digest(&bytes));
    let unchanged = if let Some(candidate) = headers.get("if-none-match") {
        candidate.to_str().is_ok_and(|c| {
            c.split(',').any(|tag| {
                let tag = tag.trim().trim_start_matches("W/");
                tag == etag || tag == "*"
            })
        })
    } else {
        // Strict comparison deliberately revalidates same-second changes.
        modified.is_some_and(|m| {
            headers
                .get("if-modified-since")
                .and_then(|h| h.to_str().ok())
                .and_then(|h| httpdate::parse_http_date(h).ok())
                .is_some_and(|since| std::time::SystemTime::from(m) < since)
        })
    };
    let mut response = if unchanged {
        StatusCode::NOT_MODIFIED.into_response()
    } else {
        ([("content-type", "application/json")], bytes).into_response()
    };
    response.headers_mut().insert(
        "etag",
        HeaderValue::from_str(&etag).map_err(|_| {
            AppError(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Invalid cache validator.",
            )
        })?,
    );
    response.headers_mut().insert(
        "cache-control",
        HeaderValue::from_static("public, max-age=0, must-revalidate"),
    );
    if let Some(modified) = modified {
        let value = httpdate::fmt_http_date(modified.into());
        response.headers_mut().insert(
            "last-modified",
            HeaderValue::from_str(&value).map_err(|_| {
                AppError(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Invalid modification date.",
                )
            })?,
        );
    }
    Ok(response)
}

pub async fn boards(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let boards: Vec<_> = board_store::boards(&state.pool).await?.into_iter().map(|board| {
        let mut value = json!({
        "board": board.slug, "title": board.title, "ws_board": i32::from(board.worksafe),
        "per_page": board.threads_per_page, "pages": (board.thread_limit + board.threads_per_page - 1) / board.threads_per_page,
        "max_filesize": 0, "max_webm_filesize": 0, "max_webm_duration": 0,
        "max_comment_chars": board.max_comment_chars, "bump_limit": board.bump_limit, "image_limit": 0,
        "cooldowns": { "threads": 0, "replies": 0, "images": 0 },
        "meta_description": board.description, "text_only": 1
        });
        if board.archive_retention_seconds > 0 {
            value["is_archived"] = json!(1);
        }
        if state.media.is_some() && board.image_limit > 0 {
            value.as_object_mut().expect("board object").remove("text_only");
            value["max_filesize"] = json!(8_388_608);
            value["image_limit"] = json!(board.image_limit);
            value["spoilers"] = json!(1);
        }
        value
    }).collect();
    response(json!({"boards": boards}), None, &headers)
}

fn post_json(
    post: Post,
    thread: &Thread,
    board: &Board,
    replies: usize,
    images: usize,
) -> Result<Value, AppError> {
    let post = PostView::new(post);
    let comment = Comment {
        lines: &post.lines,
        board: &board.slug,
    }
    .render()?;
    let op = post.post.id == thread.id;
    let mut value = json!({ "no": post.post.id, "resto": if op { 0 } else { thread.id },
        "now": post.now, "time": post.post.created_at.timestamp(), "name": post.post.name, "com": comment });
    if op {
        value["replies"] = json!(replies);
        value["images"] = json!(images);
        value["semantic_url"] = json!(semantic_url(&post.post.subject));
        if !post.post.subject.is_empty() {
            value["sub"] = json!(post.post.subject);
        }
        if thread.sticky {
            value["sticky"] = json!(1);
        }
        if thread.closed || thread.archived_at.is_some() {
            value["closed"] = json!(1);
        }
        if let Some(archived_at) = thread.archived_at {
            value["archived"] = json!(1);
            value["archived_on"] = json!(archived_at.timestamp());
        }
        if thread.reply_count >= board.bump_limit {
            value["bumplimit"] = json!(1);
        }
        if board.image_limit > 0 && images >= board.image_limit as usize {
            value["imagelimit"] = json!(1);
        }
    }
    if let Some(file) = post.post.attachment {
        if file.file_deleted {
            value["filedeleted"] = json!(1);
        } else {
            value["tim"] = json!(file.tim);
            value["filename"] = json!(
                file.filename
                    .rsplit_once('.')
                    .filter(|(stem, _)| !stem.is_empty())
                    .map_or(file.filename.as_str(), |(stem, _)| stem)
            );
            value["ext"] = json!(".png");
            value["fsize"] = json!(file.bytes);
            value["w"] = json!(file.width);
            value["h"] = json!(file.height);
            if let Some(md5) = file.md5 {
                value["md5"] = json!(md5);
            }
            if let (Some(width), Some(height)) = (file.thumbnail_width, file.thumbnail_height) {
                value["tn_w"] = json!(width);
                value["tn_h"] = json!(height);
            }
            if file.spoiler {
                value["spoiler"] = json!(1);
            }
        }
    }
    Ok(value)
}

fn semantic_url(subject: &str) -> String {
    subject
        .to_ascii_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .take(80)
        .collect()
}

fn full_thread(board: &Board, thread: &Thread, posts: Vec<Post>) -> Result<Vec<Value>, AppError> {
    if posts.is_empty() {
        return Err(AppError(StatusCode::NOT_FOUND, "Thread not found."));
    }
    let replies = posts.len() - 1;
    let images = posts
        .iter()
        .filter(|p| p.id != thread.id && p.attachment.as_ref().is_some_and(|a| !a.file_deleted))
        .count();
    posts
        .into_iter()
        .map(|post| post_json(post, thread, board, replies, images))
        .collect()
}

pub async fn thread(
    state: &AppState,
    slug: &str,
    id: i64,
    headers: &HeaderMap,
) -> Result<Response, AppError> {
    let board_store::ThreadSnapshot {
        board,
        thread,
        posts,
    } = board_store::thread_snapshot(&state.pool, slug, id).await?;
    let posts = full_thread(&board, &thread, posts)?;
    response(json!({"posts": posts}), Some(thread.modified_at), headers)
}

pub async fn archive(
    state: &AppState,
    slug: &str,
    headers: &HeaderMap,
) -> Result<Response, AppError> {
    let snapshot = board_store::archive_snapshot(&state.pool, slug).await?;
    let ids: Vec<_> = snapshot.entries.into_iter().map(|entry| entry.id).collect();
    response(json!(ids), None, headers)
}

fn preview_thread(
    board: &Board,
    preview: board_store::ThreadPreview,
) -> Result<Vec<Value>, AppError> {
    let posts = preview.posts;
    if posts.is_empty() {
        return Err(AppError(StatusCode::NOT_FOUND, "Thread not found."));
    }
    let replies = checked_reply_count(preview.visible_posts)?;
    let images = usize::try_from(preview.visible_images)
        .map_err(|_| AppError(StatusCode::SERVICE_UNAVAILABLE, "Invalid image count."))?;
    posts
        .into_iter()
        .map(|post| post_json(post, &preview.thread, board, replies, images))
        .collect()
}

fn checked_reply_count(total: i64) -> Result<usize, AppError> {
    usize::try_from(total)
        .ok()
        .and_then(|count| count.checked_sub(1))
        .ok_or(AppError(StatusCode::NOT_FOUND, "Thread not found."))
}

pub async fn thread_list(
    state: &AppState,
    slug: &str,
    headers: &HeaderMap,
) -> Result<Response, AppError> {
    let snapshot =
        board_store::board_snapshot(&state.pool, slug, board_store::BoardSelection::All, None)
            .await?;
    let board = snapshot.board;
    let threads = snapshot.threads;
    let mut pages = Vec::new();
    for (index, chunk) in threads.chunks(board.threads_per_page as usize).enumerate() {
        let mut entries = Vec::new();
        for preview in chunk {
            let thread = &preview.thread;
            let count = preview.visible_posts;
            if count == 0 {
                continue;
            }
            entries.push(json!({"no": thread.id, "last_modified": thread.modified_at.timestamp(), "replies": count-1}));
        }
        pages.push(json!({"page": index+1, "threads": entries}));
    }
    // An ETag covers removal of the most recently modified thread too.
    response(json!(pages), None, headers)
}

pub async fn catalog(
    state: &AppState,
    slug: &str,
    headers: &HeaderMap,
) -> Result<Response, AppError> {
    let snapshot =
        board_store::board_snapshot(&state.pool, slug, board_store::BoardSelection::All, Some(5))
            .await?;
    let board = snapshot.board;
    let mut pages = Vec::new();
    let mut threads = snapshot.threads.into_iter().peekable();
    while threads.peek().is_some() {
        let mut entries = Vec::new();
        for preview in threads.by_ref().take(board.threads_per_page as usize) {
            let modified = preview.thread.modified_at;
            let posts = match preview_thread(&board, preview) {
                Ok(posts) => posts,
                Err(error) if error.0 == StatusCode::NOT_FOUND => continue,
                Err(error) => return Err(error),
            };
            let mut op = posts[0].clone();
            op["last_modified"] = json!(modified.timestamp());
            if posts.len() > 1 {
                op["last_replies"] = json!(posts[posts.len().saturating_sub(5).max(1)..]);
            }
            entries.push(op);
        }
        pages.push(json!({"page": pages.len()+1, "threads": entries}));
    }
    response(json!(pages), None, headers)
}

pub async fn index(
    state: &AppState,
    slug: &str,
    page: i64,
    headers: &HeaderMap,
) -> Result<Response, AppError> {
    let snapshot = board_store::board_snapshot(
        &state.pool,
        slug,
        board_store::BoardSelection::Page(page),
        Some(5),
    )
    .await?;
    let board = snapshot.board;
    let mut entries = Vec::new();
    for preview in snapshot.threads {
        let mut posts = match preview_thread(&board, preview) {
            Ok(posts) => posts,
            Err(error) if error.0 == StatusCode::NOT_FOUND => continue,
            Err(error) => return Err(error),
        };
        let total = posts[0]["replies"].as_u64().unwrap_or(0) as usize + 1;
        if total > posts.len() {
            let omitted = total - posts.len();
            posts[0]["omitted_posts"] = json!(omitted);
            let images = posts[0]["images"].as_u64().unwrap_or(0);
            let shown = posts
                .iter()
                .skip(1)
                .filter(|p| p.get("ext").is_some())
                .count() as u64;
            posts[0]["omitted_images"] = json!(images.saturating_sub(shown));
        }
        entries.push(json!({"posts": posts}));
    }
    response(json!({"threads": entries}), None, headers)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn deletion_during_preview_does_not_wrap_reply_count() {
        assert!(checked_reply_count(0).is_err());
        assert_eq!(checked_reply_count(1).unwrap(), 0);
        assert_eq!(checked_reply_count(1001).unwrap(), 1000);
    }
    #[test]
    fn etag_precedes_date_and_same_second_dates_do_not_hide_changes() {
        let modified = DateTime::from_timestamp(1_700_000_000, 500_000_000).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert(
            "if-modified-since",
            HeaderValue::from_static("Tue, 14 Nov 2023 22:13:20 GMT"),
        );
        assert_eq!(
            response(json!({"posts": [1]}), Some(modified), &headers)
                .unwrap()
                .status(),
            StatusCode::OK
        );
        headers.insert("if-none-match", HeaderValue::from_static("\"old\""));
        headers.insert(
            "if-modified-since",
            HeaderValue::from_static("Tue, 14 Nov 2023 22:14:20 GMT"),
        );
        assert_eq!(
            response(json!({"posts": [1]}), Some(modified), &headers)
                .unwrap()
                .status(),
            StatusCode::OK
        );
    }
}
