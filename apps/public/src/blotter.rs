//! Local read-only blotter; source Atom/controller semantics remain unqualified.
use crate::{AppState, handlers::AppError};
use askama::Template;
use axum::{
    Router,
    extract::{RawQuery, State},
    http::StatusCode,
    response::Response,
    routing::get,
};
use board_store::BlotterMessage;

#[derive(Template)]
#[template(path = "blotter.html")]
struct Page {
    messages: Vec<BlotterMessage>,
    next_offset: Option<i64>,
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new().route("/blotter", get(page))
}

fn cursor(query: Option<&str>) -> Result<Option<i64>, AppError> {
    let invalid = || AppError(StatusCode::BAD_REQUEST, "Invalid blotter cursor.");
    let Some(query) = query.filter(|query| !query.is_empty()) else {
        return Ok(None);
    };
    if query.len() > 26 {
        return Err(invalid());
    }
    let Some(value) = query.strip_prefix("offset=") else {
        return Err(invalid());
    };
    if value.is_empty()
        || value.starts_with('0')
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(invalid());
    }
    value
        .parse::<i64>()
        .ok()
        .filter(|id| *id > 0)
        .map(Some)
        .ok_or_else(invalid)
}
async fn page(
    State(state): State<AppState>,
    RawQuery(query): RawQuery,
) -> Result<Response, AppError> {
    let page = board_store::blotter::blotter_page(&state.pool, cursor(query.as_deref())?).await?;
    crate::output::html(
        &state,
        &Page {
            messages: page.messages,
            next_offset: page.next_offset,
        },
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_is_bounded_canonical_and_unambiguous() {
        assert_eq!(cursor(None).unwrap(), None);
        assert_eq!(cursor(Some("offset=1")).unwrap(), Some(1));
        assert_eq!(
            cursor(Some("offset=9223372036854775807")).unwrap(),
            Some(i64::MAX)
        );
        for query in [
            "offset=",
            "offset=0",
            "offset=01",
            "offset=-1",
            "offset=+1",
            "offset=%31",
            "offset=1&offset=2",
            "offset=1&x=2",
            "atom",
            "offset=9223372036854775808",
        ] {
            assert!(cursor(Some(query)).is_err(), "{query}");
        }
    }
}
