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
pub(crate) struct TemplateBan {
    board: String,
    target: i64,
    csrf: String,
    template: i64,
}

#[derive(sqlx::FromRow)]
struct TemplateChoice {
    id: i64,
    name: String,
    global: bool,
    warning: bool,
    seconds: Option<i64>,
    public_reason: String,
}

impl TemplateChoice {
    fn scope_label(&self) -> &'static str {
        if self.global {
            "All boards"
        } else {
            "This board"
        }
    }

    fn duration_label(&self) -> String {
        if self.warning {
            "Warning".into()
        } else if let Some(seconds) = self.seconds {
            format!("Ban for {seconds} seconds")
        } else {
            "Indefinite ban".into()
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Unban {
    board: String,
    target: i64,
    csrf: String,
    id: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ThreadBan {
    board: String,
    target: i64,
    csrf: String,
    op_reason: String,
    rep_reason: String,
    #[serde(default)]
    op_days: String,
    #[serde(default)]
    rep_days: String,
}
#[derive(Template)]
#[template(path = "restrictions.html")]
struct Page {
    board: String,
    target: i64,
    csrf: String,
    recent: bool,
    restriction: Option<i64>,
    can_ban_thread: bool,
    templates: Vec<TemplateChoice>,
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
        Some("P134C" | "P136R" | "55P03") => AppError::Capacity,
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
    let (root, sticky): (bool, bool) = sqlx::query_as("SELECT p.id=p.thread_id,t.sticky FROM content.posts p JOIN content.threads t ON t.id=p.thread_id AND t.board=p.board WHERE p.id=$1 AND p.board=$2 AND NOT p.deleted AND NOT t.deleted AND t.archived_at IS NULL")
        .bind(context.target).bind(&context.board).fetch_optional(&state.staff).await?.ok_or(AppError::NotFound)?;
    let can_ban_thread = root
        && session
            .permissions
            .can_ban_thread(&session.role, &context.board)
        && (!sticky || session.at_least(Level::Admin));
    let recent = session.recent;
    let token = auth::hash(&auth::cookie(&headers, state.config.cookie_name())?);
    let templates =
        sqlx::query_as("SELECT id,name,global,warning,seconds,public_reason FROM staff_identity.restriction_templates($1,$2,$3,$4)")
            .bind(token)
            .bind(auth::hash(&csrf))
            .bind(state.config.idle_timeout.as_secs() as i32)
            .bind(&context.board)
            .fetch_all(authority.connection())
            .await
            .map_err(database)?;
    let html = Page {
        board: context.board,
        target: context.target,
        csrf,
        recent,
        restriction: context.restriction.filter(|id| *id > 0),
        can_ban_thread,
        templates,
    }
    .render()
    .map_err(|_| AppError::Internal)?;
    authority.ensure_current(false).await?;
    authority.finish().await?;
    Ok(Html(html))
}

pub(crate) async fn thread(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Form(input): Form<ThreadBan>,
) -> Result<Redirect, AppError> {
    auth::origin(&headers, &state.config.origin)?;
    validate(&input.board, input.target)?;
    let session = auth::session(&state, &headers).await?;
    authorize(&session, &input.board)?;
    if !session
        .permissions
        .can_ban_thread(&session.role, &input.board)
    {
        return Err(AppError::Forbidden);
    }
    auth::csrf(&session, &input.csrf)?;
    if !session.recent {
        return Err(AppError::Recent);
    }
    for reason in [&input.op_reason, &input.rep_reason] {
        if reason.is_empty() || reason == "0" || reason.len() > 16384 || reason.contains('\0') {
            return Err(AppError::Invalid);
        }
    }
    if input.op_days.len() > 128 || input.rep_days.len() > 128 {
        return Err(AppError::Invalid);
    }
    let op_days = board_domain::php_scalar::integer(&input.op_days);
    let rep_days = board_domain::php_scalar::integer(&input.rep_days);
    if !(0..=9999).contains(&op_days) || !(0..=9999).contains(&rep_days) {
        return Err(AppError::Invalid);
    }
    let token = auth::hash(&auth::cookie(&headers, state.config.cookie_name())?);
    let csrf = auth::hash(&input.csrf);
    board_store::ban_thread(
        &state.staff,
        board_store::restrictions::ThreadBan {
            session: &token,
            csrf: &csrf,
            idle_seconds: state.config.idle_timeout.as_secs() as i32,
            board: &input.board,
            thread: input.target,
            op_reason: &input.op_reason,
            reply_reason: &input.rep_reason,
            op_days: op_days as i32,
            reply_days: rep_days as i32,
        },
    )
    .await
    .map_err(|error| match error {
        board_store::StoreError::Database(error) => database(error),
        board_store::StoreError::NotFound => AppError::NotFound,
        _ => AppError::Internal,
    })?;
    Ok(Redirect::to("/reports"))
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

pub(crate) async fn issue_template(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Form(input): Form<TemplateBan>,
) -> Result<Redirect, AppError> {
    auth::origin(&headers, &state.config.origin)?;
    validate(&input.board, input.target)?;
    let session = auth::session(&state, &headers).await?;
    authorize(&session, &input.board)?;
    auth::csrf(&session, &input.csrf)?;
    if !session.recent {
        return Err(AppError::Recent);
    }
    if !(1..=i64::from(i32::MAX)).contains(&input.template) {
        return Err(AppError::Invalid);
    }
    let token = auth::hash(&auth::cookie(&headers, state.config.cookie_name())?);
    let id: i64 =
        sqlx::query_scalar("SELECT content.issue_post_template_restriction($1,$2,$3,$4,$5,$6)")
            .bind(token)
            .bind(auth::hash(&input.csrf))
            .bind(state.config.idle_timeout.as_secs() as i32)
            .bind(&input.board)
            .bind(input.target)
            .bind(input.template)
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
