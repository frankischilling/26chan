//! Board-scoped operator messages. The unread cookie is presentation state,
//! never authentication or permission to read a message.
use crate::{AppError, AppState, auth};
use askama::Template;
use axum::{
    Json,
    extract::{Path, Query, State},
    http::HeaderMap,
    response::{Html, IntoResponse, Response},
};
use serde::Deserialize;
use std::sync::Arc;

pub(crate) const READY_SQL: &str = r#"
SELECT EXISTS (
 SELECT 1 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace
 JOIN pg_roles r ON r.oid=p.proowner
 WHERE n.nspname='staff_identity' AND p.proname='staff_message_page'
  AND oidvectortypes(p.proargtypes)='bytea, bytea, integer, integer, bigint'
  AND p.prorettype='jsonb'::regtype AND p.prosecdef
  AND r.rolname='board_restriction_owner'
  AND has_function_privilege(current_user,p.oid,'EXECUTE')=(current_user='board_auth')
) AND EXISTS (
 SELECT 1 FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace
 WHERE n.nspname='restrictions' AND c.relname='staff_messages'
  AND NOT has_table_privilege(current_user,c.oid,'SELECT,INSERT,UPDATE,DELETE')
  AND NOT has_any_column_privilege(current_user,c.oid,'SELECT,INSERT,UPDATE')
)
"#;

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Context {
    #[serde(default)]
    offset: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedMessage {
    id: i64,
    created_at: i64,
    boards: Vec<String>,
    content: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedPage {
    messages: Vec<SavedMessage>,
    offset: u32,
    previous_offset: u32,
    next_offset: u32,
    unread_count: u32,
    read_at: i64,
}

struct Message {
    id: i64,
    created_at: String,
    boards: String,
    lines: Vec<String>,
    read_divider: bool,
}

#[derive(Template)]
#[template(path = "staff_messages.html")]
struct Page {
    messages: Vec<Message>,
    offset: u32,
    previous_offset: u32,
    next_offset: u32,
}

fn seen(headers: &HeaderMap, name: &str) -> i64 {
    let mut values = headers
        .get_all("cookie")
        .iter()
        .filter_map(|header| header.to_str().ok())
        .flat_map(|header| header.split(';'))
        .filter_map(|field| field.trim().split_once('='))
        .filter(|(key, _)| *key == name)
        .map(|(_, value)| value);
    let Some(value) = values.next() else {
        return 0;
    };
    if values.next().is_some()
        || value.len() > 19
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return 0;
    }
    value.parse().unwrap_or(0)
}

fn prepare(page: SavedPage, seen: i64) -> Result<Page, AppError> {
    let mut divider = seen > 0;
    let mut messages = Vec::with_capacity(page.messages.len());
    for message in page.messages {
        let created =
            chrono::DateTime::from_timestamp(message.created_at, 0).ok_or(AppError::Internal)?;
        let read_divider = divider && seen > message.created_at;
        if read_divider {
            divider = false;
        }
        messages.push(Message {
            id: message.id,
            created_at: created.format("%m/%d/%y %H:%M").to_string(),
            boards: message.boards.join(", "),
            lines: message
                .content
                .replace("\r\n", "\n")
                .split('\n')
                .map(str::to_owned)
                .collect(),
            read_divider,
        });
    }
    Ok(Page {
        messages,
        offset: page.offset,
        previous_offset: page.previous_offset,
        next_offset: page.next_offset,
    })
}

async fn load(
    state: &AppState,
    headers: &HeaderMap,
    offset: u32,
) -> Result<(SavedPage, i64), AppError> {
    if offset > 10000 {
        return Err(AppError::Invalid);
    }
    let mut authority = auth::guard(state, headers).await?;
    let csrf = auth::cookie(headers, &crate::handlers::csrf_cookie(state))?;
    auth::csrf(&authority.session, &csrf)?;
    let token = auth::hash(&auth::cookie(headers, state.config.cookie_name())?);
    let seen = seen(headers, &format!("{}-messages", state.config.cookie_name()));
    let sqlx::types::Json(page): sqlx::types::Json<SavedPage> =
        sqlx::query_scalar("SELECT staff_identity.staff_message_page($1,$2,$3,$4,$5)")
            .bind(token)
            .bind(auth::hash(&csrf))
            .bind(state.config.idle_timeout.as_secs() as i32)
            .bind(offset as i32)
            .bind(seen)
            .fetch_one(authority.connection())
            .await
            .map_err(|error| {
                match error
                    .as_database_error()
                    .and_then(|error| error.code())
                    .as_deref()
                {
                    Some("P134A") => AppError::Forbidden,
                    Some("P134C" | "55P03") => AppError::Capacity,
                    _ => AppError::Database(error),
                }
            })?;
    authority.ensure_current(false).await?;
    authority.finish().await?;
    Ok((page, seen))
}

pub(crate) async fn show(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(context): Query<Context>,
) -> Result<Response, AppError> {
    let (page, seen) = load(&state, &headers, context.offset).await?;
    let read_at = page.read_at;
    let html = prepare(page, seen)?
        .render()
        .map_err(|_| AppError::Internal)?;
    let mut response = Html(html).into_response();
    crate::handlers::set_cookie(
        &mut response,
        &state,
        &format!("{}-messages", state.config.cookie_name()),
        &read_at.to_string(),
        365 * 24 * 3600,
    )?;
    Ok(response)
}

pub(crate) async fn count(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    let (page, _) = load(&state, &headers, 0).await?;
    Ok(Json(serde_json::json!({"unread": page.unread_count})))
}

pub(crate) async fn stylesheet() -> impl IntoResponse {
    (
        [("content-type", "text/css; charset=utf-8")],
        include_str!("../static/reportqueue.v1.css"),
    )
}

pub(crate) async fn asset(Path(asset): Path<String>) -> Result<Response, AppError> {
    let (mime, bytes): (&str, &'static [u8]) = match asset.as_str() {
        "select-arrow.png" => ("image/png", include_bytes!("../static/select-arrow.png")),
        "select-arrow-light.png" => (
            "image/png",
            include_bytes!("../static/select-arrow-light.png"),
        ),
        "favicon-team.ico" => ("image/x-icon", include_bytes!("../static/favicon-team.ico")),
        "reportqueue.woff2" => ("font/woff2", include_bytes!("../static/reportqueue.woff2")),
        _ => return Err(AppError::NotFound),
    };
    Ok(([("content-type", mime)], bytes).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_first_strictly_older_message_gets_the_source_read_divider() {
        let messages = [120, 110, 100, 90]
            .into_iter()
            .enumerate()
            .map(|(id, created_at)| SavedMessage {
                id: id as i64 + 1,
                created_at,
                boards: vec!["a".into()],
                content: "<script>owned</script>\nsecond line".into(),
            })
            .collect();
        let page = prepare(
            SavedPage {
                messages,
                offset: 0,
                previous_offset: 0,
                next_offset: 25,
                unread_count: 1,
                read_at: 130,
            },
            110,
        )
        .unwrap();
        assert_eq!(
            page.messages
                .iter()
                .map(|message| message.read_divider)
                .collect::<Vec<_>>(),
            [false, false, true, false]
        );
        let html = page.render().unwrap();
        assert_eq!(html.matches("class=\"item-read\"").count(), 1);
        assert!(!html.contains("<script>"));
        assert!(html.contains("second line"));
        assert!(html.contains("/messages?offset=25"));
        assert!(!html.contains("Previous"));
    }

    #[test]
    fn unread_marker_accepts_only_bounded_unsigned_seconds() {
        for value in ["-1", "9223372036854775808", "123x", "1.5", ""] {
            let mut headers = HeaderMap::new();
            headers.insert("cookie", format!("staff-messages={value}").parse().unwrap());
            assert_eq!(seen(&headers, "staff-messages"), 0);
        }
        let mut headers = HeaderMap::new();
        headers.insert("cookie", "staff-messages=1700000000".parse().unwrap());
        assert_eq!(seen(&headers, "staff-messages"), 1700000000);
        headers.append("cookie", "staff-messages=1700000001".parse().unwrap());
        assert_eq!(seen(&headers, "staff-messages"), 0);
    }
}
