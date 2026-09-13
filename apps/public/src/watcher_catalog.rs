use axum::{
    extract::{Path, State},
    http::HeaderMap,
    response::IntoResponse,
};

use crate::{AppState, api};

pub(crate) async fn get(
    State(state): State<AppState>,
    Path(board): Path<String>,
    headers: HeaderMap,
) -> impl IntoResponse {
    api::catalog(&state, &board, &headers).await
}
