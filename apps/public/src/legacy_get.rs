//! Finite legacy GET dispatcher. Reporting keeps its existing popup contract;
//! res redirects expose only public post/thread identity, never post bodies.
use crate::{AppState, handlers::AppError, legacy_report};
use axum::{
    Extension,
    extract::{OriginalUri, Path, Query, State, rejection::QueryRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResQuery {
    res: String,
}

pub(crate) async fn get(
    State(state): State<AppState>,
    Path(board): Path<String>,
    Extension(peer): Extension<crate::security::RequestPeer>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
    report_query: Result<Query<legacy_report::ReportQuery>, QueryRejection>,
) -> Result<Response, AppError> {
    let raw = uri.query().unwrap_or("");
    let mut reporting = false;
    let mut res = false;
    for (name, value) in url::form_urlencoded::parse(raw.as_bytes()) {
        reporting |= name == "mode" && value == "report";
        res |= name == "res";
    }
    // Preserve reporting's malformed-query shell too. Conflicting res/report
    // fields are rejected by ReportQuery rather than selecting a redirect.
    if reporting || !res {
        return legacy_report::get(
            State(state),
            Path(board),
            Extension(peer),
            headers,
            OriginalUri(uri),
            report_query,
        )
        .await;
    }
    let invalid = || AppError(StatusCode::BAD_REQUEST, "Invalid post lookup request.");
    // Even fully percent-encoded canonical keys/digits fit this fixed bound.
    if !legacy_report::safe_board(&board) || raw.len() > 128 {
        return Err(invalid());
    }
    let Query(query) = Query::<ResQuery>::try_from_uri(&uri).map_err(|_| invalid())?;
    let post = legacy_report::positive_id(&query.res).ok_or_else(invalid)?;
    let mut response = match board_store::legacy_res_thread(&state.pool, &board, post).await {
        Ok(thread) => {
            // Relative, same-origin, and built only from a validated board and
            // numeric identities. Never copy or downgrade from the Referer.
            let location = format!("/{board}/thread/{thread}#p{post}");
            let mut response = StatusCode::MOVED_PERMANENTLY.into_response();
            response.headers_mut().insert(
                header::LOCATION,
                HeaderValue::from_str(&location).map_err(|_| invalid())?,
            );
            response
        }
        Err(board_store::StoreError::NotFound) => {
            AppError(StatusCode::NOT_FOUND, "Post not found.").into_response()
        }
        Err(cause) => return Err(cause.into()),
    };
    // Source resredir uses the same short public cache for hits and misses.
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=2"),
    );
    Ok(response)
}
