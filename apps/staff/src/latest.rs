use crate::{AppError, AppState, auth};
use axum::{
    Json,
    extract::{Query, State},
    http::HeaderMap,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Serialize)]
pub(crate) struct Latest {
    no: i64,
}

/// The source endpoint always polls the private janitor board, regardless of
/// which public board the caller is browsing.
pub(crate) async fn latest(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<Latest>, AppError> {
    let mut authority = auth::guard(&state, &headers).await?;
    let no = sqlx::query_scalar(
        "SELECT coalesce(max(p.id),0) FROM content.posts p \
         JOIN content.visible_threads t ON t.id=p.thread_id AND t.board=p.board \
         WHERE p.board='j' AND NOT p.deleted",
    )
    .fetch_one(&state.staff)
    .await?;
    authority.ensure_current(false).await?;
    authority.finish().await?;
    Ok(Json(Latest { no }))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Mode {
    mode: String,
}

pub(crate) async fn legacy(
    state: State<Arc<AppState>>,
    headers: HeaderMap,
    Query(query): Query<Mode>,
) -> Result<Json<Latest>, AppError> {
    if query.mode != "latest" {
        return Err(AppError::NotFound);
    }
    latest(state, headers).await
}
