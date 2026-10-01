use crate::{
    AppState,
    handlers::AppError,
    views::{PostView, ThreadView},
};
use askama::Template;
use axum::{
    Router,
    extract::{Query, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use board_domain::Token;
use board_domain::comment_markup::Tag;
use board_domain::word_break::WordPart;
use board_store::Board;
use serde::Deserialize;
use serde_json::{Value, json};

pub(crate) const SCRIPT_PATH: &str = "/static/global-search.v1.js";
const RESPONSE_LIMIT: usize = 1_048_576;

#[derive(Template)]
#[template(path = "search.html")]
struct SearchPage {
    boards: Vec<Board>,
}

#[derive(Template)]
#[template(path = "search_thread.html")]
struct SearchThreadTemplate<'a> {
    board: &'a Board,
    view: &'a ThreadView,
    media_origin: &'a str,
    catalog: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SearchQuery {
    q: String,
    #[serde(default)]
    b: Option<String>,
    #[serde(default)]
    o: Option<i64>,
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/globalsearch.php", get(page))
        .route("/search/api", get(api))
        .route(SCRIPT_PATH, get(script))
}

async fn script() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/javascript; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=0, must-revalidate"),
        ],
        include_str!("../static/global-search.v1.js"),
    )
}

async fn page(State(state): State<AppState>) -> Result<Response, AppError> {
    let mut boards = board_store::boards(&state.pool).await?;
    boards.sort_by(|left, right| left.slug.cmp(&right.slug));
    crate::output::html(&state, &SearchPage { boards })
}

async fn api(
    State(state): State<AppState>,
    Query(params): Query<SearchQuery>,
) -> Result<Response, AppError> {
    let board = params.b.as_deref().filter(|board| !board.is_empty());
    let offset = params.o.unwrap_or(0);
    let result = board_store::search(&state.pool, &params.q, board, offset).await?;
    let media_origin = state
        .media
        .as_ref()
        .map(|media| media.settings.origin.as_string())
        .unwrap_or_default();
    let mut threads = Vec::with_capacity(result.threads.len());
    for hit in result.threads {
        let replies = usize::try_from(hit.thread.reply_count).map_err(|_| {
            AppError(
                StatusCode::SERVICE_UNAVAILABLE,
                "Search result is unavailable.",
            )
        })?;
        let images = usize::try_from(hit.visible_images).map_err(|_| {
            AppError(
                StatusCode::SERVICE_UNAVAILABLE,
                "Search result is unavailable.",
            )
        })?;
        let posts_json: Vec<Value> = hit
            .posts
            .iter()
            .cloned()
            .map(|post| crate::api::post_json(post, &hit.thread, &hit.board, replies, images, None))
            .collect::<Result<_, _>>()?;
        let latest_reply_id = hit
            .posts
            .iter()
            .rev()
            .find(|post| post.id != hit.thread.id)
            .map(|post| post.id);
        let view = ThreadView {
            catalog_last_reply: None,
            tail_size: 0,
            latest_reply_id,
            thread: hit.thread,
            posts: hit.posts.into_iter().map(PostView::new).collect(),
            omitted: 0,
            image_replies: hit.visible_images,
        };
        let html = SearchThreadTemplate {
            board: &hit.board,
            view: &view,
            media_origin: &media_origin,
            catalog: false,
        }
        .render()?;
        threads.push(json!({
            "board": hit.board.slug,
            "posts": posts_json,
            "html": html,
        }));
    }
    let value = json!({
        "threads": threads,
        "offset": result.offset,
        "nhits": result.nhits,
    });
    let bytes = crate::output::json(state.limits.response_writer(RESPONSE_LIMIT), &value)?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/json; charset=utf-8"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        bytes.into_body(),
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_query_rejects_unknown_fields_and_bad_offset_types() {
        assert!(serde_json::from_value::<SearchQuery>(json!({"q":"owned","b":"g","o":10})).is_ok());
        assert!(serde_json::from_value::<SearchQuery>(json!({"q":"owned","extra":1})).is_err());
        assert!(serde_json::from_value::<SearchQuery>(json!({"q":"owned","o":"next"})).is_err());
    }
}
