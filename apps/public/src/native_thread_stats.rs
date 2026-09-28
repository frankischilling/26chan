use crate::{AppState, handlers::AppError};
use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode, Uri},
    response::Response,
};
use serde::Serialize;

#[derive(Serialize)]
struct Statistics {
    version: u8,
    board: String,
    thread: String,
    replies: i64,
    images: i64,
    sticky: bool,
    closed: bool,
    archived: bool,
    bump_limited: bool,
    image_limited: bool,
    page: Option<i64>,
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
            "Invalid thread statistics options.",
        ));
    }
    let id = key
        .parse::<i64>()
        .ok()
        .filter(|id| *id > 0 && id.to_string() == key)
        .ok_or(AppError(StatusCode::NOT_FOUND, "Thread not found."))?;
    let source = board_store::thread_statistics(&state.pool, &board, id).await?;
    let stats = Statistics {
        version: 1,
        board: source.board,
        thread: source.id.to_string(),
        replies: source.replies,
        images: source.images,
        sticky: source.sticky,
        closed: source.closed,
        archived: source.archived,
        page: source.page,
        bump_limited: board_domain::bump::limited(
            source.sticky,
            source.permaage,
            source.replies as u64,
            source.bump_limit as u32,
        ),
        image_limited: board_domain::image_limit::json_limited(
            source.sticky,
            source.permaage,
            source.undead,
            source.images as u64,
            source.image_limit as u32,
        ),
    };
    let bytes = crate::output::json(state.limits.response_writer(1024), &stats)?;
    // Another thread can change this thread's page without changing its own
    // modified timestamp. Only the complete representation's ETag is sound.
    crate::api::bytes_response(bytes, None, &headers)
}

#[cfg(test)]
mod tests {
    use axum::{
        body::Body,
        http::{Request, StatusCode},
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn invalid_identity_and_options_are_rejected_before_storage_access() {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://unused:unused@127.0.0.1:1/unused")
            .unwrap();
        let app = crate::router(pool, "http://127.0.0.1:3000".into(), false);
        for key in ["0", "-1", "01", "+1", "1.0", "9223372036854775808"] {
            let response = app
                .clone()
                .oneshot(
                    Request::get(format!("/_watch/test/thread/{key}/stats"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        }
        for suffix in ["?", "?page=1", "?callback=bad"] {
            let response = app
                .clone()
                .oneshot(
                    Request::get(format!("/_watch/test/thread/1/stats{suffix}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        }
    }
}
