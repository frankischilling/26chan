use crate::{
    AppState,
    handlers::AppError,
    views::{PostFragment, PostView, ThreadView},
};
use askama::Template;
use axum::{
    Router,
    extract::{Query, State},
    http::header,
    response::{IntoResponse, Response},
    routing::get,
};
use board_store::Board;
use serde::Deserialize;
use serde_json::json;
mod excerpt;

pub(crate) const SCRIPT_PATH: &str = "/static/global-search.v1.js";
const RESPONSE_LIMIT: usize = 1_048_576;

#[derive(Template)]
#[template(path = "search.html")]
struct SearchPage {
    boards: Vec<Board>,
    media_origin: String,
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
    crate::output::html(
        &state,
        &SearchPage {
            boards,
            media_origin: state
                .media
                .as_ref()
                .map(|media| media.settings.origin.as_string())
                .unwrap_or_default(),
        },
    )
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
            posts: hit
                .posts
                .into_iter()
                .map(|post| {
                    let filtered = post.wordfilter_payload.is_some();
                    let mut view = PostView::new(post);
                    if filtered {
                        view.lines = excerpt::lines(&view.lines, &params.q, 1024);
                    }
                    view
                })
                .collect(),
            omitted: 0,
            image_replies: hit.visible_images,
        };
        let mut posts = Vec::with_capacity(view.posts.len());
        for item in &view.posts {
            posts.push(json!({
                "no": item.post.id.to_string(),
                "html": PostFragment {
                    item,
                    view: &view,
                    board: &hit.board,
                    media_origin: &media_origin,
                    catalog: false,
                }
                .render()?,
            }));
        }
        threads.push(json!({
            "board": hit.board.slug,
            "thread": view.thread.id.to_string(),
            "posts": posts,
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
