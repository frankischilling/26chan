//! Source-derived destructive text-thread moves. Formatting and private evidence
//! are sealed by the trusted issuer; the browser submits only board/thread IDs.
use crate::{AppError, AppState, auth, auth::Session};
use askama::Template;
use axum::{
    body::Bytes,
    extract::{RawQuery, State},
    http::HeaderMap,
    response::Html,
};
use board_store::thread_transfer::{
    self as transfer, Credentials, Ticket, TransferError, TransferResult,
};
use rand::{RngCore, rngs::OsRng};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug, PartialEq, Eq)]
struct Input {
    board: String,
    target: i64,
    destination: String,
    csrf: String,
}
#[derive(sqlx::FromRow)]
struct Destination {
    slug: String,
    title: String,
}
#[derive(Template)]
#[template(path = "thread_transfer.html")]
struct Page {
    board: String,
    target: i64,
    destinations: Vec<Destination>,
    csrf: String,
    recent: bool,
}
#[derive(Template)]
#[template(path = "thread_transfer_result.html")]
struct ResultPage {
    result: TransferResult,
    public_origin: String,
}

fn decode(raw: &[u8]) -> Result<String, AppError> {
    let mut bytes = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        match raw[i] {
            b'%' => {
                let pair = raw.get(i + 1..i + 3).ok_or(AppError::Invalid)?;
                let hi = (pair[0] as char).to_digit(16).ok_or(AppError::Invalid)?;
                let lo = (pair[1] as char).to_digit(16).ok_or(AppError::Invalid)?;
                bytes.push((hi * 16 + lo) as u8);
                i += 3;
            }
            b'+' => {
                bytes.push(b' ');
                i += 1;
            }
            byte => {
                bytes.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8(bytes).map_err(|_| AppError::Invalid)
}
fn fields(raw: &[u8], allowed: &[&str]) -> Result<BTreeMap<String, String>, AppError> {
    if raw.is_empty() || raw.len() > 4096 {
        return Err(AppError::Invalid);
    }
    let mut result = BTreeMap::new();
    for pair in raw.split(|b| *b == b'&') {
        let at = pair
            .iter()
            .position(|b| *b == b'=')
            .ok_or(AppError::Invalid)?;
        let name = decode(&pair[..at])?;
        let value = decode(&pair[at + 1..])?;
        if !allowed.contains(&name.as_str()) || result.insert(name, value).is_some() {
            return Err(AppError::Invalid);
        }
    }
    Ok(result)
}
fn source(fields: &BTreeMap<String, String>) -> Result<(String, i64), AppError> {
    let board = fields.get("board").ok_or(AppError::Invalid)?;
    board_domain::BoardSlug::parse(board).map_err(|_| AppError::Invalid)?;
    let target = fields.get("target").ok_or(AppError::Invalid)?;
    if target.is_empty() || target.len() > 19 || !target.bytes().all(|b| b.is_ascii_digit()) {
        return Err(AppError::Invalid);
    }
    let target = target.parse::<i64>().map_err(|_| AppError::Invalid)?;
    if target <= 0 {
        return Err(AppError::Invalid);
    }
    Ok((board.clone(), target))
}
fn parse(raw: &[u8]) -> Result<Input, AppError> {
    let fields = fields(raw, &["board", "target", "destination", "csrf", "move_del"])?;
    let (board, target) = source(&fields)?;
    let destination = fields.get("destination").ok_or(AppError::Invalid)?.clone();
    board_domain::BoardSlug::parse(&destination).map_err(|_| AppError::Invalid)?;
    if board == "b"
        || destination == board
        || matches!(destination.as_str(), "f" | "j")
        || fields.get("move_del").map(String::as_str) != Some("1")
    {
        return Err(AppError::Invalid);
    }
    let csrf = fields.get("csrf").ok_or(AppError::Invalid)?.clone();
    if csrf.len() != 43
        || !csrf
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(AppError::Invalid);
    }
    Ok(Input {
        board,
        target,
        destination,
        csrf,
    })
}
fn authorize(session: &Session, board: &str) -> Result<(), AppError> {
    if !session
        .permissions
        .action_allowed(&session.role, board, "thread-transfer")
    {
        return Err(AppError::Forbidden);
    }
    Ok(())
}
fn credentials(state: &AppState, headers: &HeaderMap, csrf: &str) -> Result<Credentials, AppError> {
    let token = auth::cookie(headers, state.config.cookie_name())?;
    Ok(Credentials {
        session_hash: auth::hash(&token)
            .try_into()
            .map_err(|_| AppError::Internal)?,
        csrf_hash: auth::hash(csrf)
            .try_into()
            .map_err(|_| AppError::Internal)?,
        idle_seconds: i32::try_from(state.config.idle_timeout.as_secs())
            .map_err(|_| AppError::Internal)?,
    })
}
fn result_page(state: &AppState, result: TransferResult) -> Result<Html<String>, AppError> {
    ResultPage {
        result,
        public_origin: state.config.public_origin.clone(),
    }
    .render()
    .map(Html)
    .map_err(|_| AppError::Internal)
}
pub(crate) fn error_status(error: &TransferError) -> axum::http::StatusCode {
    use axum::http::StatusCode as S;
    match error {
        TransferError::Forbidden | TransferError::Recent => S::FORBIDDEN,
        TransferError::NotFound => S::NOT_FOUND,
        TransferError::Invalid | TransferError::Format => S::BAD_REQUEST,
        TransferError::Busy | TransferError::Expired | TransferError::Drift => S::CONFLICT,
        TransferError::Capacity => S::TOO_MANY_REQUESTS,
        TransferError::Database => S::SERVICE_UNAVAILABLE,
    }
}

pub(crate) async fn show(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    RawQuery(query): RawQuery,
) -> Result<Html<String>, AppError> {
    let session = auth::session(&state, &headers).await?;
    let (board, target) = source(&fields(
        query.as_deref().unwrap_or("").as_bytes(),
        &["board", "target"],
    )?)?;
    authorize(&session, &board)?;
    let csrf = auth::cookie(&headers, &format!("{}-csrf", state.config.cookie_name()))?;
    auth::csrf(&session, &csrf)?;
    let credentials = credentials(&state, &headers, &csrf)?;
    // Recovery is read-only and account/both-board scoped, even when the source
    // has already been erased and the original HTTP response was interrupted.
    if let Some(result) = transfer::result_for_source(&state.auth, &credentials, &board, target)
        .await
        .map_err(AppError::ThreadTransfer)?
    {
        return result_page(&state, result);
    }
    if board == "b" {
        return Err(AppError::Invalid);
    }
    let available:bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM content.boards b JOIN content.threads t ON t.board=b.slug JOIN content.posts p ON p.board=t.board AND p.id=t.id AND p.thread_id=t.id WHERE b.slug=$1 AND t.id=$2 AND NOT b.staff_only AND NOT b.upload_board AND NOT t.deleted AND NOT t.closed AND t.archived_at IS NULL AND NOT p.deleted)")
        .bind(&board).bind(target).fetch_one(&state.staff).await?;
    if !available {
        return Err(AppError::NotFound);
    }
    let mut destinations:Vec<Destination> = sqlx::query_as("SELECT slug,title FROM content.boards WHERE NOT staff_only AND NOT upload_board AND slug<>$1 AND slug NOT IN ('f','j') ORDER BY slug LIMIT 1001")
        .bind(&board).fetch_all(&state.staff).await?;
    if destinations.len() > 1000 {
        return Err(AppError::Capacity);
    }
    destinations.retain(|b| {
        session
            .permissions
            .action_allowed(&session.role, &b.slug, "thread-transfer")
    });
    // This guard performs only the final read-authority check. It is released
    // before any prepare/seal/consume function can acquire transfer locks.
    let mut final_authority = auth::guard(&state, &headers).await?;
    authorize(&final_authority.session, &board)?;
    destinations.retain(|b| {
        final_authority.session.permissions.action_allowed(
            &final_authority.session.role,
            &b.slug,
            "thread-transfer",
        )
    });
    final_authority.ensure_current(false).await?;
    let recent = final_authority.finish().await?.recent;
    Page {
        board,
        target,
        destinations,
        csrf,
        recent,
    }
    .render()
    .map(Html)
    .map_err(|_| AppError::Internal)
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
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.split(';').next())
            .map(str::trim)
            != Some("application/x-www-form-urlencoded")
    {
        return Err(AppError::Invalid);
    }
    let input = parse(&body)?;
    // Finish the account/session guard before crossing auth and staff pools.
    // The SQL entry points independently validate current authority and expiry.
    let session = auth::session(&state, &headers).await?;
    auth::csrf(&session, &input.csrf)?;
    authorize(&session, &input.board)?;
    authorize(&session, &input.destination)?;
    if !session.recent {
        return Err(AppError::Recent);
    }
    let credentials = credentials(&state, &headers, &input.csrf)?;
    // An interrupted response may already have committed this destructive move.
    if let Some(result) =
        transfer::result_for_source(&state.auth, &credentials, &input.board, input.target)
            .await
            .map_err(AppError::ThreadTransfer)?
    {
        if result.destination_board != input.destination {
            return Err(AppError::ThreadTransfer(TransferError::Drift));
        }
        return result_page(&state, result);
    }
    let mut bytes = [0u8; 32];
    OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| AppError::Internal)?;
    let ticket = Ticket::from_bytes(bytes);
    let prepared = match transfer::prepare(
        &state.auth,
        &credentials,
        &ticket,
        &input.board,
        input.target,
        &input.destination,
    )
    .await
    {
        Ok(prepared) => prepared,
        Err(error) => {
            let _ = transfer::discard(&state.auth, &credentials, &ticket).await;
            return Err(AppError::ThreadTransfer(error));
        }
    };
    let request_id = prepared.request_id().to_owned();
    if let Err(error) = transfer::seal(&state.auth, &credentials, &prepared).await {
        let _ = transfer::discard(&state.auth, &credentials, &ticket).await;
        return Err(AppError::ThreadTransfer(error));
    }
    match transfer::consume(&state.staff, &ticket).await {
        Ok(result) => result_page(&state, result),
        Err(error) => {
            // Reconcile this request only; never issue another ticket or retry
            // consume automatically after an uncertain database response.
            if let Ok(Some(result)) = transfer::result(&state.auth, &credentials, &request_id).await
            {
                return result_page(&state, result);
            }
            let _ = transfer::discard(&state.auth, &credentials, &ticket).await;
            Err(AppError::ThreadTransfer(error))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transfer_form_accepts_only_exact_bounded_identifiers() {
        let csrf = "a".repeat(43);
        let valid = format!("board=g&target=12&destination=v&csrf={csrf}&move_del=1");
        let parsed = parse(valid.as_bytes()).unwrap();
        assert_eq!(
            (
                parsed.board.as_str(),
                parsed.target,
                parsed.destination.as_str()
            ),
            ("g", 12, "v")
        );
        for extra in [
            "&target=13",
            "&payload=FP01",
            "&ticket=forged",
            "&destination=v",
            "&unknown=x",
        ] {
            assert!(parse(format!("{valid}{extra}").as_bytes()).is_err());
        }
        for (old, new) in [
            ("target=12", "target=0"),
            ("target=12", "target=-1"),
            ("target=12", "target=9223372036854775808"),
            ("target=12", "target=12x"),
            ("board=g", "board=b"),
            ("destination=v", "destination=g"),
            ("destination=v", "destination=f"),
            ("destination=v", "destination=j"),
            ("destination=v", "destination=%FF"),
            ("destination=v", "destination=%"),
            ("move_del=1", "move_del=0"),
        ] {
            assert!(parse(valid.replace(old, new).as_bytes()).is_err(), "{new}");
        }
        assert!(parse(valid.replace("&move_del=1", "").as_bytes()).is_err());
        assert!(parse(&vec![b'x'; 4097]).is_err());
    }
    #[test]
    fn transfer_requires_moderator_and_each_board_scope() {
        let mut session = Session {
            account_id: 1,
            role: "janitor".into(),
            csrf_hash: vec![],
            recent: true,
            permissions: crate::access::Permissions::all_boards(),
        };
        assert!(authorize(&session, "g").is_err());
        session.role = "moderator".into();
        assert!(authorize(&session, "g").is_ok());
        session.role = "unknown".into();
        assert!(authorize(&session, "g").is_err());
    }
}
