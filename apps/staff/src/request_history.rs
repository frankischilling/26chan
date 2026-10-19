//! Pending-request references resolve identities only inside the scoped reader.
use crate::{AppError, AppState, auth};
use askama::Template;
use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    response::Html,
};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Cursor {
    #[serde(default)]
    before: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    id: i64,
    board: String,
    post_id: i64,
    global: bool,
    warning: bool,
    starts_at: i64,
    ends_at: Option<i64>,
    public_reason: String,
    private_reason: String,
    active: bool,
    retirement: Option<String>,
    retired_at: Option<i64>,
}
impl Record {
    fn start(&self) -> String {
        timestamp(self.starts_at)
    }
    fn end(&self) -> String {
        self.ends_at
            .map(timestamp)
            .unwrap_or_else(|| "Indefinite".into())
    }
    fn retired(&self) -> String {
        self.retired_at.map(timestamp).unwrap_or_default()
    }
}
fn timestamp(value: i64) -> String {
    chrono::DateTime::from_timestamp(value, 0)
        .map(|t| t.format("%Y-%m-%d %H:%M:%S UTC").to_string())
        .unwrap_or_else(|| "Unavailable".into())
}
#[derive(Deserialize, Template)]
#[serde(deny_unknown_fields)]
#[template(path = "request_history.html")]
struct Page {
    request_id: i64,
    board: String,
    rows: Vec<Record>,
    next_before: i64,
}
pub(crate) async fn show(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Path(id): Path<i64>,
    Query(cursor): Query<Cursor>,
) -> Result<Html<String>, AppError> {
    if id <= 0 || cursor.before < 0 {
        return Err(AppError::Invalid);
    }
    let mut authority = auth::guard(&state, &headers).await?;
    let csrf = auth::cookie(&headers, &crate::handlers::csrf_cookie(&state))?;
    auth::csrf(&authority.session, &csrf)?;
    let token = auth::hash(&auth::cookie(&headers, state.config.cookie_name())?);
    let sqlx::types::Json(page): sqlx::types::Json<Page> =
        sqlx::query_scalar("SELECT staff_identity.janitor_request_history($1,$2,$3,$4,$5)")
            .bind(token)
            .bind(auth::hash(&csrf))
            .bind(state.config.idle_timeout.as_secs() as i32)
            .bind(id)
            .bind(cursor.before)
            .fetch_one(authority.connection())
            .await
            .map_err(crate::janitor_requests::database)?;
    authority.ensure_current(false).await?;
    authority.finish().await?;
    Ok(Html(page.render().map_err(|_| AppError::Internal)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_reasons_are_escaped_and_empty_history_is_clear() {
        let mut page = Page {
            request_id: 7,
            board: "demo".into(),
            rows: vec![],
            next_before: 0,
        };
        assert!(
            page.render()
                .unwrap()
                .contains("No retained bans or warnings")
        );
        page.rows.push(Record {
            id: 9,
            board: "demo".into(),
            post_id: 11,
            global: false,
            warning: false,
            starts_at: 0,
            ends_at: None,
            public_reason: "<script>public</script>".into(),
            private_reason: "<img src=x onerror=alert(1)>".into(),
            active: true,
            retirement: None,
            retired_at: None,
        });
        let html = page.render().unwrap();
        assert!(!html.contains("<script>public</script>"));
        assert!(!html.contains("<img src=x"));
        assert!(html.contains("Indefinite"));
        assert!(html.contains("1970-01-01 00:00:00 UTC"));
        assert!(!html.contains("Older &raquo;"));
    }
}
