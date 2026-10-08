//! Reporter-wide queue clearing through the private ownership function.
use crate::{AppError, AppState, auth};
use askama::Template;
use axum::{body::Bytes, extract::State, http::HeaderMap, response::Html};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Template)]
#[template(path = "reporter_clear.html")]
struct Cleared {
    count: i64,
}

#[derive(Debug)]
struct Input {
    csrf: String,
    board: String,
    report_id: i64,
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
        if !["csrf", "board", "report_id"].contains(&name.as_str())
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
    let report_id = fields.remove("report_id").ok_or(AppError::Invalid)?;
    if report_id.is_empty()
        || report_id.len() > 19
        || !report_id.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(AppError::Invalid);
    }
    let report_id = report_id.parse::<i64>().map_err(|_| AppError::Invalid)?;
    if report_id <= 0 {
        return Err(AppError::Invalid);
    }
    Ok(Input {
        csrf,
        board,
        report_id,
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
    if !session.permissions.can_clear_reporter(&session.role) {
        return Err(AppError::Forbidden);
    }
    if !session.recent {
        return Err(AppError::Recent);
    }
    let mut tx = state.staff.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
        .execute(&mut *tx)
        .await?;
    // The function locks all affected boards before the admission gate.
    let count: Option<i64> = sqlx::query_scalar("SELECT content.clear_reporter($1,$2)")
        .bind(&input.board)
        .bind(input.report_id)
        .fetch_one(&mut *tx)
        .await?;
    let count = count.ok_or(AppError::NotFound)?;
    if !(1..=10000).contains(&count) {
        return Err(AppError::Internal);
    }
    sqlx::query("INSERT INTO content.moderation_audit(account_id,board,target_id,action,reporter_clear_count) VALUES ($1,$2,$3,'reporter-clear',$4)")
        .bind(session.account_id).bind(&input.board).bind(input.report_id).bind(count)
        .execute(&mut *tx).await?;
    let html = Cleared { count }.render().map_err(|_| AppError::Internal)?;
    authority.ensure_current(true).await?;
    tx.commit().await?;
    authority.finish().await?;
    Ok(Html(html))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_bounded_strict_seed_forms_are_accepted() {
        let input = parse(b"csrf=token&board=test&report_id=42").unwrap();
        assert_eq!(input.report_id, 42);
        assert_eq!(input.board, "test");
        for raw in [
            "csrf=token&board=test&report_id=0",
            "csrf=token&board=test&report_id=-1",
            "csrf=token&board=test&report_id=+1",
            "csrf=token&board=test&report_id=1x",
            "csrf=token&board=test&report_id=9223372036854775808",
            "csrf=token&board=test&report_id=42&report_id=43",
            "csrf=token&board=test&report_id=42&%63srf=other",
            "csrf=token&board=test&report_id=42&ip=127.0.0.1",
            "csrf=token&board=test&report_id%5B%5D=42",
            "csrf=token&board=test&report_id=42&",
            "csrf=token&board=test&report_id=%FF",
            "csrf=token&board=test&report_id=%0G",
            "csrf=token&board=test&report_id=%",
            "csrf=token&board=test",
            "csrf=&board=test&report_id=42",
            "csrf=token&board=../test&report_id=42",
        ] {
            assert!(parse(raw.as_bytes()).is_err(), "{raw}");
        }
        assert!(parse(&vec![b'x'; 4097]).is_err());
    }

    #[test]
    fn success_uses_the_committed_count_and_a_plain_queue_link() {
        let html = Cleared { count: 37 }.render().unwrap();
        assert!(html.contains("Cleared 37 reports"));
        assert!(html.contains("href=\"/reports\""));
        assert!(!html.contains("?"));
    }
}
