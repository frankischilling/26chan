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
    let boards: Vec<_> = board_store::boards(&state.pool).await?.into_iter().map(|board| json!({
        "board": board.slug, "title": board.title, "ws_board": i32::from(board.worksafe),
        "per_page": board.threads_per_page, "pages": (board.thread_limit + board.threads_per_page - 1) / board.threads_per_page,
        "max_filesize": 0, "max_webm_filesize": 0, "max_webm_duration": 0,
        "max_comment_chars": board.max_comment_bytes, "bump_limit": board.bump_limit, "image_limit": 0,
        "cooldowns": { "threads": 0, "replies": 0, "images": 0 },
        "meta_description": board.description, "text_only": 1
    })).collect();
    response(json!({"boards": boards}), None, &headers)
}

fn post_json(
    post: Post,
    thread: &Thread,
    board: &Board,
    replies: usize,
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
        value["images"] = json!(0);
        value["semantic_url"] = json!(semantic_url(&post.post.subject));
        if !post.post.subject.is_empty() {
            value["sub"] = json!(post.post.subject);
        }
        if thread.sticky {
            value["sticky"] = json!(1);
        }
        if thread.closed {
            value["closed"] = json!(1);
        }
        if thread.reply_count >= board.bump_limit {
            value["bumplimit"] = json!(1);
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
    posts
        .into_iter()
        .map(|post| post_json(post, thread, board, replies))
        .collect()
}

pub async fn thread(
    state: &AppState,
    slug: &str,
    id: i64,
    headers: &HeaderMap,
) -> Result<Response, AppError> {
    let board = board_store::board(&state.pool, slug).await?;
    let (thread, posts) = board_store::thread_snapshot(&state.pool, slug, id).await?;
    let posts = full_thread(&board, &thread, posts)?;
    response(json!({"posts": posts}), Some(thread.modified_at), headers)
}

async fn preview_thread(
    state: &AppState,
    board: &Board,
    thread: &Thread,
) -> Result<Vec<Value>, AppError> {
    let posts = board_store::preview_posts(&state.pool, &board.slug, thread.id, 5).await?;
    if posts.is_empty() {
        return Err(AppError(StatusCode::NOT_FOUND, "Thread not found."));
    }
    let replies = checked_reply_count(
        board_store::visible_post_count(&state.pool, &board.slug, thread.id).await?,
    )?;
    posts
        .into_iter()
        .map(|post| post_json(post, thread, board, replies))
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
    let board = board_store::board(&state.pool, slug).await?;
    let threads = board_store::threads(&state.pool, slug, 0, i64::from(board.thread_limit)).await?;
    let mut pages = Vec::new();
    for (index, chunk) in threads.chunks(board.threads_per_page as usize).enumerate() {
        let mut entries = Vec::new();
        for thread in chunk {
            let count = board_store::visible_post_count(&state.pool, slug, thread.id).await?;
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
    let board = board_store::board(&state.pool, slug).await?;
    let threads = board_store::threads(&state.pool, slug, 0, i64::from(board.thread_limit)).await?;
    let mut pages = Vec::new();
    for (index, chunk) in threads.chunks(board.threads_per_page as usize).enumerate() {
        let mut entries = Vec::new();
        for thread in chunk {
            let posts = match preview_thread(state, &board, thread).await {
                Ok(posts) => posts,
                Err(error) if error.0 == StatusCode::NOT_FOUND => continue,
                Err(error) => return Err(error),
            };
            let mut op = posts[0].clone();
            op["last_modified"] = json!(thread.modified_at.timestamp());
            if posts.len() > 1 {
                op["last_replies"] = json!(posts[posts.len().saturating_sub(5).max(1)..]);
            }
            entries.push(op);
        }
        pages.push(json!({"page": index+1, "threads": entries}));
    }
    response(json!(pages), None, headers)
}

pub async fn index(
    state: &AppState,
    slug: &str,
    page: i64,
    headers: &HeaderMap,
) -> Result<Response, AppError> {
    let board = board_store::board(&state.pool, slug).await?;
    let max_pages =
        i64::from((board.thread_limit + board.threads_per_page - 1) / board.threads_per_page);
    if !(1..=max_pages).contains(&page) {
        return Err(AppError(StatusCode::NOT_FOUND, "Page not found."));
    }
    let threads = board_store::threads(
        &state.pool,
        slug,
        (page - 1) * i64::from(board.threads_per_page),
        i64::from(board.threads_per_page),
    )
    .await?;
    let mut entries = Vec::new();
    for thread in threads {
        let mut posts = match preview_thread(state, &board, &thread).await {
            Ok(posts) => posts,
            Err(error) if error.0 == StatusCode::NOT_FOUND => continue,
            Err(error) => return Err(error),
        };
        let total = posts[0]["replies"].as_u64().unwrap_or(0) as usize + 1;
        if total > posts.len() {
            let omitted = total - posts.len();
            posts[0]["omitted_posts"] = json!(omitted);
            posts[0]["omitted_images"] = json!(0);
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
