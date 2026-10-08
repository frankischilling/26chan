//! Bounded ordinary report-group clearing and immutable clear provenance.
use crate::{AppError, AppState, auth};
use askama::Template;
use axum::{
    body::Bytes,
    extract::{RawQuery, State},
    http::HeaderMap,
    response::Html,
};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Template)]
#[template(path = "report_group_clear.html")]
struct Cleared {
    count: i64,
    board: String,
}

#[derive(Debug)]
struct Input {
    csrf: String,
    board: String,
    post_id: i64,
}

fn decode(raw: &[u8]) -> Result<String, AppError> {
    let mut bytes = Vec::with_capacity(raw.len());
    let mut pos = 0;
    while pos < raw.len() {
        match raw[pos] {
            b'%' => {
                let pair = raw.get(pos + 1..pos + 3).ok_or(AppError::Invalid)?;
                let high = (pair[0] as char).to_digit(16).ok_or(AppError::Invalid)?;
                let low = (pair[1] as char).to_digit(16).ok_or(AppError::Invalid)?;
                bytes.push((high * 16 + low) as u8);
                pos += 3;
            }
            b'+' => {
                bytes.push(b' ');
                pos += 1;
            }
            byte => {
                bytes.push(byte);
                pos += 1;
            }
        }
    }
    String::from_utf8(bytes).map_err(|_| AppError::Invalid)
}

fn parse(raw: &[u8]) -> Result<Input, AppError> {
    if raw.is_empty() || raw.len() > 4096 {
        return Err(AppError::Invalid);
    }
    let mut fields = BTreeMap::new();
    for pair in raw.split(|byte| *byte == b'&') {
        let separator = pair
            .iter()
            .position(|byte| *byte == b'=')
            .ok_or(AppError::Invalid)?;
        let name = decode(&pair[..separator])?;
        let value = decode(&pair[separator + 1..])?;
        if !["csrf", "board", "post_id"].contains(&name.as_str())
            || fields.insert(name, value).is_some()
        {
            return Err(AppError::Invalid);
        }
    }
    let csrf = fields.remove("csrf").ok_or(AppError::Invalid)?;
    if csrf.is_empty() || csrf.len() > 256 {
        return Err(AppError::Invalid);
    }
    let board = fields.remove("board").ok_or(AppError::Invalid)?;
    board_domain::BoardSlug::parse(&board).map_err(|_| AppError::Invalid)?;
    let post_id = fields.remove("post_id").ok_or(AppError::Invalid)?;
    if post_id.is_empty() || post_id.len() > 19 || !post_id.bytes().all(|b| b.is_ascii_digit()) {
        return Err(AppError::Invalid);
    }
    let post_id = post_id.parse::<i64>().map_err(|_| AppError::Invalid)?;
    if post_id <= 0 {
        return Err(AppError::Invalid);
    }
    Ok(Input {
        csrf,
        board,
        post_id,
    })
}

pub(crate) async fn submit(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Html<String>, AppError> {
    auth::origin(&headers, &state.config.origin)?;
    if headers.get_all("sec-fetch-site").iter().count() != 1 {
        return Err(AppError::Forbidden);
    }
    if headers.get_all("content-type").iter().count() != 1
        || headers
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.split(';').next())
            .map(str::trim)
            != Some("application/x-www-form-urlencoded")
    {
        return Err(AppError::Invalid);
    }
    let mut authority = auth::guard(&state, &headers).await?;
    let input = parse(&body)?;
    let session = &authority.session;
    auth::csrf(session, &input.csrf)?;
    if !session.at_least(crate::access::Level::Janitor) || !session.permissions.allows(&input.board)
    {
        return Err(AppError::Forbidden);
    }
    if !session.recent {
        return Err(AppError::Recent);
    }
    let mut tx = state.staff.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
        .execute(&mut *tx)
        .await?;
    // The ownership function serializes this board-scoped clear on the board lock.
    let count: Option<i64> = sqlx::query_scalar("SELECT content.clear_report_group($1,$2,$3)")
        .bind(&input.board)
        .bind(input.post_id)
        .bind(session.account_id)
        .fetch_one(&mut *tx)
        .await
        .map_err(clear_error)?;
    let count = count.ok_or(AppError::NotFound)?;
    if !(0..=10000).contains(&count) {
        return Err(AppError::Internal);
    }
    if count == 0 {
        authority.ensure_current(true).await?;
        return Err(AppError::ReportGroupAlreadyCleared);
    }
    sqlx::query("INSERT INTO content.moderation_audit(account_id,board,target_id,action,group_clear_count) VALUES ($1,$2,$3,'report-group-clear',$4)")
        .bind(session.account_id).bind(&input.board).bind(input.post_id).bind(count)
        .execute(&mut *tx).await?;
    let html = Cleared {
        count,
        board: input.board,
    }
    .render()
    .map_err(|_| AppError::Internal)?;
    authority.ensure_current(true).await?;
    tx.commit().await?;
    authority.finish().await?;
    Ok(Html(html))
}

fn clear_error(error: sqlx::Error) -> AppError {
    if error.as_database_error().and_then(|e| e.code()).as_deref() == Some("P0108") {
        AppError::ReportGroupWeights
    } else {
        AppError::Database(error)
    }
}

#[derive(Template)]
#[template(path = "report_group_history.html")]
struct History {
    board: String,
    reports: Vec<crate::store::ClearedReport>,
}

fn history_board(query: Option<&str>) -> Result<String, AppError> {
    let raw = query.ok_or(AppError::Invalid)?;
    if raw.len() > 256 || raw.contains('&') {
        return Err(AppError::Invalid);
    }
    let (key, value) = raw.split_once('=').ok_or(AppError::Invalid)?;
    if decode(key.as_bytes())? != "board" {
        return Err(AppError::Invalid);
    }
    let board = decode(value.as_bytes())?;
    board_domain::BoardSlug::parse(&board).map_err(|_| AppError::Invalid)?;
    Ok(board)
}

pub(crate) async fn history(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Result<Html<String>, AppError> {
    let mut authority = auth::guard(&state, &headers).await?;
    let board = history_board(query.as_deref())?;
    let reports = crate::store::cleared_reports(&state.staff, &authority.session, &board).await?;
    let html = History { board, reports }
        .render()
        .map_err(|_| AppError::Internal)?;
    authority.ensure_current(false).await?;
    authority.finish().await?;
    Ok(Html(html))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_bounded_strict_group_forms_are_accepted() {
        let input = parse(b"csrf=token&board=test&post_id=42").unwrap();
        assert_eq!(input.post_id, 42);
        assert_eq!(input.board, "test");
        for raw in [
            "csrf=token&board=test&post_id=0",
            "csrf=token&board=test&post_id=-1",
            "csrf=token&board=test&post_id=+1",
            "csrf=token&board=test&post_id=1x",
            "csrf=token&board=test&post_id=9223372036854775808",
            "csrf=token&board=test&post_id=42&post_id=43",
            "csrf=token&board=test&post_id=42&%63srf=other",
            "csrf=token&board=test&post_id=42&ip=127.0.0.1",
            "csrf=token&board=test&post_id%5B%5D=42",
            "csrf=token&board=test&post_id=42&",
            "csrf=token&board=test&post_id=%FF",
            "csrf=token&board=test&post_id=%0G",
            "csrf=token&board=test&post_id=%",
            "csrf=token&board=test",
            "csrf=&board=test&post_id=42",
            "csrf=token&board=../test&post_id=42",
        ] {
            assert!(parse(raw.as_bytes()).is_err(), "{raw}");
        }
        assert!(parse(&vec![b'x'; 4097]).is_err());
    }

    #[test]
    fn cleared_history_requires_one_exact_board_parameter() {
        assert_eq!(history_board(Some("board=test")).unwrap(), "test");
        assert_eq!(history_board(Some("%62oard=test")).unwrap(), "test");
        assert!(history_board(None).is_err());
        for raw in [
            "",
            "board=",
            "board=../test",
            "board=test&board=other",
            "board=test&limit=1000",
            "other=test",
            "board=%FF",
            "board=%",
            "board=test&",
        ] {
            assert!(history_board(Some(raw)).is_err(), "{raw}");
        }
    }

    #[test]
    fn cleared_history_escapes_evidence_and_keeps_disposition_separate() {
        let html = History {
            board: "test".into(),
            reports: vec![crate::store::ClearedReport {
                id: 1,
                post_id: 42,
                reason: "<script>alert(1)</script>".into(),
                category_id: Some(3),
                state: "open".into(),
                group_cleared_at: chrono::DateTime::from_timestamp(1_700_000_000, 0).unwrap(),
                group_cleared_by: 7,
                group_clear_inherited: true,
            }],
        }
        .render()
        .unwrap();
        assert!(html.contains("Original report disposition: open"));
        assert!(html.contains("Originating clear account: 7"));
        assert!(html.contains("inherited clear: true"));
        assert!(html.contains("not a saved post snapshot"));
        assert!(!html.contains("<script>"));
        assert!(!html.contains("<form"));
        assert!(!html.contains("<img"));
    }

    #[test]
    fn success_uses_the_committed_count_and_board_history_link() {
        let html = Cleared {
            count: 37,
            board: "test".into(),
        }
        .render()
        .unwrap();
        assert!(html.contains("Cleared 37 reports"));
        assert!(html.contains("href=\"/reports\""));
        assert!(html.contains("href=\"/reports/cleared?board=test\""));
        let escaped = Cleared {
            count: 1,
            board: "\"><script>alert(1)</script>".into(),
        }
        .render()
        .unwrap();
        assert!(!escaped.contains("<script>"));
    }
}
