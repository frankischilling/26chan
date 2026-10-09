//! Bounded, release-owned index projection for incremental native navigation.
use crate::{
    AppState,
    handlers::AppError,
    native_updater_snapshot::{RenderedPost, render_posts},
    views::{PostView, ThreadView},
};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode, Uri},
    response::Response,
};
use serde::Serialize;
use std::collections::BTreeSet;

const MAX_BYTES: usize = 4_194_304;

#[derive(Serialize)]
struct Page {
    version: u8,
    board: String,
    page: u16,
    next_page: Option<u16>,
    threads: Vec<Preview>,
}

#[derive(Serialize)]
struct Preview {
    thread: String,
    closed: bool,
    sticky: bool,
    archived: bool,
    replies: usize,
    images: usize,
    omitted: usize,
    posts: Vec<RenderedPost>,
}

fn unavailable() -> AppError {
    AppError(
        StatusCode::SERVICE_UNAVAILABLE,
        "The next page is unavailable. Open its ordinary board link.",
    )
}

fn encode(
    snapshot: board_store::BoardSnapshot,
    page: u16,
    limit: usize,
    media_origin: &str,
    writer: board_http::ResponseWriter,
) -> Result<board_http::EncodedResponse, AppError> {
    let board = snapshot.board;
    let spoiler_thumbnail = crate::views::spoilers::choose_thumbnail(&board);
    if page > 999 || snapshot.threads.len() > 20 || (page == 999 && snapshot.has_next) {
        return Err(unavailable());
    }
    let mut threads = Vec::with_capacity(snapshot.threads.len());
    let mut remaining = limit;
    let mut ids = BTreeSet::new();
    for preview in snapshot.threads {
        let thread = preview.thread;
        if thread.id <= 0
            || thread.deleted
            || thread.archived_at.is_some()
            || thread.board != board.slug
            || !(1..=1001).contains(&preview.visible_posts)
            || !(0..preview.visible_posts).contains(&preview.visible_images)
            || preview.posts.is_empty()
            || preview.posts.len() > 4
            || preview.posts[0].id != thread.id
            || preview.posts.len()
                != usize::try_from(preview.visible_posts.min(4)).map_err(|_| unavailable())?
            || preview.posts.iter().any(|post| {
                post.deleted
                    || post.board != board.slug
                    || post.thread_id != thread.id
                    || !ids.insert(post.id)
            })
            || preview
                .posts
                .windows(2)
                .any(|pair| pair[0].id >= pair[1].id)
        {
            return Err(unavailable());
        }
        let replies = preview.visible_posts as usize - 1;
        let images = preview.visible_images as usize;
        let omitted = replies - (preview.posts.len() - 1);
        let view = ThreadView {
            catalog_position: None,
            thread,
            posts: preview
                .posts
                .into_iter()
                .map(|post| PostView::resolved(post, &snapshot.quote_targets, None))
                .collect(),
            omitted,
            image_replies: images as i64,
            tail_size: 0,
            latest_reply_id: preview.latest_reply_id,
            catalog_last_reply: None,
        };
        let posts = render_posts(&view, &board, media_origin, &spoiler_thumbnail, remaining)?;
        for post in &posts {
            remaining = remaining
                .checked_sub(post.html_len())
                .ok_or_else(unavailable)?;
        }
        threads.push(Preview {
            thread: view.thread.id.to_string(),
            closed: view.thread.closed,
            sticky: view.thread.sticky,
            archived: false,
            replies,
            images,
            omitted,
            posts,
        });
    }
    let result = Page {
        version: 1,
        board: board.slug,
        page,
        next_page: snapshot.has_next.then_some(page + 1),
        threads,
    };
    crate::output::json(writer, &result).map_err(|_| unavailable())
}

pub(crate) async fn get(
    State(state): State<AppState>,
    Path((board, key)): Path<(String, String)>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    if uri.query().is_some() {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "Invalid board page options.",
        ));
    }
    let page = key
        .parse::<u16>()
        .ok()
        .filter(|page| *page <= 999 && page.to_string() == key)
        .ok_or(AppError(StatusCode::NOT_FOUND, "Page not found."))?;
    let snapshot = board_store::board_snapshot(
        &state.pool,
        &board,
        board_store::BoardSelection::Page(i64::from(page) + 1),
        Some(3),
    )
    .await?;
    let media = state
        .media
        .as_ref()
        .map(|media| media.settings.origin.as_string())
        .unwrap_or_default();
    let limit = state.limits.response_limit(MAX_BYTES);
    let encoded = encode(
        snapshot,
        page,
        limit,
        &media,
        state.limits.response_writer(limit),
    )?;
    crate::api::bytes_response(encoded, None, &headers)
}

#[derive(Serialize)]
struct BoardDirectory {
    version: u8,
    boards: Vec<BoardLink>,
}

#[derive(Serialize)]
struct BoardLink {
    board: String,
    title: String,
}

pub(crate) async fn directory(
    State(state): State<AppState>,
    uri: Uri,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    if uri.query().is_some() {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "Invalid navigation options.",
        ));
    }
    let rows = board_store::boards(&state.pool).await?;
    let boards = rows
        .into_iter()
        .map(|row| BoardLink {
            board: row.slug,
            title: row.title,
        })
        .collect();
    let limit = state.limits.response_limit(32768);
    let encoded = crate::output::json(
        state.limits.response_writer(limit),
        &BoardDirectory { version: 1, boards },
    )?;
    crate::api::bytes_response(encoded, None, &headers)
}

#[cfg(test)]
mod tests {
    use axum::{body::Body, http::Request};
    use tower::ServiceExt;

    #[tokio::test]
    async fn invalid_page_identifiers_and_options_fail_before_database_access() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
            .unwrap();
        let app = crate::router(pool, "http://127.0.0.1:3000".into(), false);
        for (path, code) in [
            ("/_watch/test/page/-1", 404),
            ("/_watch/test/page/01", 404),
            ("/_watch/test/page/1000", 404),
            ("/_watch/test/page/%2B1", 404),
            ("/_watch/test/page/0?extra=1", 400),
            ("/_watch/boards?extra=1", 400),
        ] {
            let result = app
                .clone()
                .oneshot(Request::get(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(result.status().as_u16(), code, "{path}");
        }
    }
}
