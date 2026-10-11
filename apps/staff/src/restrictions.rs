//! Separately authenticated issuance; target equality stays in the database.
use crate::{AppError, AppState, access::Level, auth};
use askama::Template;
use axum::{
    Form,
    extract::{Query, State},
    http::HeaderMap,
    response::{Html, Redirect},
};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Context {
    board: String,
    target: i64,
    restriction: Option<i64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Ban {
    board: String,
    target: i64,
    csrf: String,
    scope: String,
    kind: String,
    seconds: Option<i64>,
    public_reason: String,
    private_reason: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Unban {
    board: String,
    target: i64,
    csrf: String,
    id: i64,
}
#[derive(Template)]
#[template(path = "restrictions.html")]
struct Page {
    board: String,
    target: i64,
    csrf: String,
    recent: bool,
    restriction: Option<i64>,
}

fn validate(board: &str, target: i64) -> Result<(), AppError> {
    board_domain::BoardSlug::parse(board).map_err(|_| AppError::Invalid)?;
    if target <= 0 {
        return Err(AppError::Invalid);
    }
    Ok(())
}
fn authorize(session: &auth::Session, board: &str) -> Result<(), AppError> {
    if !session.at_least(Level::Moderator)
        || !session.permissions.allows(board)
        || (board == "j" && !session.at_least(Level::Manager))
    {
        return Err(AppError::Forbidden);
    }
    Ok(())
}
fn database(error: sqlx::Error) -> AppError {
    match error
        .as_database_error()
        .and_then(|error| error.code())
        .as_deref()
    {
        Some("P134A") => AppError::Forbidden,
        Some("P134Q") => AppError::Recent,
        Some("P134N") => AppError::NotFound,
        Some("23514") => AppError::Invalid,
        Some("P134C" | "55P03") => AppError::Capacity,
        _ => AppError::Database(error),
    }
}

pub(crate) async fn show(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(context): Query<Context>,
) -> Result<Html<String>, AppError> {
    validate(&context.board, context.target)?;
    let mut authority = auth::guard(&state, &headers).await?;
    let session = &authority.session;
    authorize(session, &context.board)?;
    let csrf = auth::cookie(&headers, &format!("{}-csrf", state.config.cookie_name()))?;
    auth::csrf(session, &csrf)?;
    let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM content.posts p JOIN content.threads t ON t.id=p.thread_id AND t.board=p.board WHERE p.id=$1 AND p.board=$2 AND NOT p.deleted AND NOT t.deleted AND t.archived_at IS NULL)")
        .bind(context.target).bind(&context.board).fetch_one(&state.staff).await?;
    if !exists {
        return Err(AppError::NotFound);
    }
    let html = Page {
        board: context.board,
        target: context.target,
        csrf,
        recent: session.recent,
        restriction: context.restriction.filter(|id| *id > 0),
    }
    .render()
    .map_err(|_| AppError::Internal)?;
    authority.ensure_current(false).await?;
    authority.finish().await?;
    Ok(Html(html))
}

pub(crate) async fn issue(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Form(input): Form<Ban>,
) -> Result<Redirect, AppError> {
    auth::origin(&headers, &state.config.origin)?;
    validate(&input.board, input.target)?;
    let session = auth::session(&state, &headers).await?;
    authorize(&session, &input.board)?;
    auth::csrf(&session, &input.csrf)?;
    if !session.recent {
        return Err(AppError::Recent);
    }
    let global = match input.scope.as_str() {
        "local" => false,
        "global" => true,
        _ => return Err(AppError::Invalid),
    };
    let (warning, seconds) = match (input.kind.as_str(), input.seconds) {
        ("warning", None | Some(0)) => (true, Some(0)),
        ("finite", Some(seconds)) if (1..=863913600).contains(&seconds) => (false, Some(seconds)),
        ("indefinite", None | Some(0)) => (false, None),
        _ => return Err(AppError::Invalid),
    };
    if input.public_reason.is_empty()
        || input.public_reason.len() > 16384
        || input.private_reason.len() > 32768
        || input.public_reason.contains('\0')
        || input.private_reason.contains('\0')
    {
        return Err(AppError::Invalid);
    }
    let token = auth::hash(&auth::cookie(&headers, state.config.cookie_name())?);
    // lock_session has already committed its activity update. The independent
    // definer now locks/revalidates real authority through the content commit.
    let id: i64 =
        sqlx::query_scalar("SELECT content.issue_post_restriction($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
            .bind(token)
            .bind(auth::hash(&input.csrf))
            .bind(state.config.idle_timeout.as_secs() as i32)
            .bind(&input.board)
            .bind(input.target)
            .bind(global)
            .bind(warning)
            .bind(seconds)
            .bind(input.public_reason)
            .bind(input.private_reason)
            .fetch_one(&state.staff)
            .await
            .map_err(database)?;
    Ok(Redirect::to(&format!(
        "/ban?board={}&target={}&restriction={id}",
        input.board, input.target
    )))
}

pub(crate) async fn unban(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Form(input): Form<Unban>,
) -> Result<Redirect, AppError> {
    auth::origin(&headers, &state.config.origin)?;
    validate(&input.board, input.target)?;
    if input.id <= 0 {
        return Err(AppError::Invalid);
    }
    let session = auth::session(&state, &headers).await?;
    authorize(&session, &input.board)?;
    auth::csrf(&session, &input.csrf)?;
    if !session.recent {
        return Err(AppError::Recent);
    }
    let token = auth::hash(&auth::cookie(&headers, state.config.cookie_name())?);
    // The claimed return board never authorizes the ban: the definer resolves
    // its stored board and checks current scope before retiring it.
    let _: bool = sqlx::query_scalar("SELECT content.unban_restriction($1,$2,$3,$4)")
        .bind(token)
        .bind(auth::hash(&input.csrf))
        .bind(state.config.idle_timeout.as_secs() as i32)
        .bind(input.id)
        .fetch_one(&state.staff)
        .await
        .map_err(database)?;
    Ok(Redirect::to(&format!(
        "/ban?board={}&target={}",
        input.board, input.target
    )))
}
