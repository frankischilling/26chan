//! Source feedback queues with separate read/dismiss permissions and native forms.
use crate::{AppError, AppState, auth};
use askama::Template;
use axum::{
    Form,
    extract::{Query, State},
    http::{HeaderMap, header},
    response::{Html, IntoResponse, Redirect, Response},
};
use board_domain::site_submissions::feedback::Status;
use serde::Deserialize;
use std::sync::Arc;
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Context {
    action: Option<String>,
    profile: Option<i64>,
    answered: Option<String>,
    cat: Option<u16>,
    offset: Option<i64>,
    id: Option<i64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Action {
    action: String,
    id: i64,
    csrf: String,
    profile: i64,
    answered: Option<String>,
    cat: Option<u16>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedRow {
    id: i64,
    category: u16,
    category_label: String,
    subject: String,
    message: String,
    reply: Option<String>,
    reply_capcode: Option<String>,
    updated_at: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedCategory {
    id: u16,
    label: String,
    short_label: String,
    count: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedPage {
    profile: Option<i64>,
    rows: Vec<SavedRow>,
    categories: Vec<SavedCategory>,
    draft_count: i64,
    next: i64,
    can_dismiss: bool,
    is_admin: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedDetail {
    id: i64,
    profile: i64,
    category: u16,
    category_label: String,
    subject: String,
    message: String,
    state: i16,
    reply: Option<String>,
    reply_capcode: Option<String>,
    created_at: i64,
    updated_at: i64,
    address: String,
    user_agent: String,
    language: String,
}
struct Row {
    saved: SavedRow,
    date: String,
    href: String,
}
struct Category {
    saved: SavedCategory,
    href: String,
    active: bool,
}
#[derive(Template)]
#[template(path = "feedback-review.html")]
struct Page {
    profile: i64,
    status: i32,
    category: String,
    csrf: String,
    recent: bool,
    can_dismiss: bool,
    is_admin: bool,
    rows: Vec<Row>,
    categories: Vec<Category>,
    draft_count: i64,
    next: String,
    all: String,
    answered: String,
    dismissed: String,
    drafts: String,
}
#[derive(Template)]
#[template(path = "feedback-detail.html")]
struct Detail {
    saved: SavedDetail,
    date: String,
    back: String,
}
fn status(value: Option<&str>) -> Result<i32, AppError> {
    match Status::filter(value) {
        Some(Status::Pending) => Ok(0),
        Some(Status::Answered) => Ok(1),
        Some(Status::Dismissed) => Ok(2),
        Some(Status::Draft) => Ok(3),
        None => Err(AppError::Invalid),
    }
}
fn link(profile: i64, status: i32, category: Option<u16>, before: i64) -> String {
    let mut query = url::form_urlencoded::Serializer::new(String::new());
    query.append_pair("action", "review");
    if profile > 0 {
        query.append_pair("profile", &profile.to_string());
    }
    if status > 0 {
        query.append_pair("answered", &status.to_string());
    }
    if let Some(category) = category {
        query.append_pair("cat", &category.to_string());
    }
    if before > 0 {
        query.append_pair("offset", &before.to_string());
    }
    format!("/feedback?{}", query.finish())
}
fn date(epoch: i64) -> Result<String, AppError> {
    chrono::DateTime::from_timestamp(epoch, 0)
        .map(|d| d.format("%m/%d/%y %H:%M").to_string())
        .ok_or(AppError::Internal)
}
fn database(error: sqlx::Error) -> AppError {
    match error.as_database_error().and_then(|e| e.code()).as_deref() {
        Some("P225A") => AppError::Forbidden,
        Some("P225Q") => AppError::Recent,
        Some("P225C" | "55P03") => AppError::Capacity,
        Some("23514") => AppError::Invalid,
        _ => AppError::Database(error),
    }
}
fn capcode(value: Option<&str>) -> bool {
    matches!(value, None | Some("admin" | "mod"))
}
pub(crate) async fn page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(input): Query<Context>,
) -> Result<Html<String>, AppError> {
    let selected = status(input.answered.as_deref())?;
    let profile = input.profile.unwrap_or(0);
    let before = input.offset.unwrap_or(0);
    if profile < 0 || before < 0 {
        return Err(AppError::Invalid);
    }
    let read = match input.action.as_deref() {
        None | Some("review") => false,
        Some("read") => true,
        _ => return Err(AppError::Invalid),
    };
    if read
        && (input.id.is_none_or(|id| id <= 0)
            || input.answered.is_some()
            || input.cat.is_some()
            || input.offset.is_some())
        || !read && input.id.is_some()
    {
        return Err(AppError::Invalid);
    }
    let mut authority = auth::guard(&state, &headers).await?;
    let csrf = auth::cookie(&headers, &crate::handlers::csrf_cookie(&state))?;
    auth::csrf(&authority.session, &csrf)?;
    let token = auth::hash(&auth::cookie(&headers, state.config.cookie_name())?);
    if read {
        let saved: Option<sqlx::types::Json<SavedDetail>> =
            sqlx::query_scalar("SELECT submission_api.feedback_review_detail($1,$2,$3,$4)")
                .bind(&token)
                .bind(auth::hash(&csrf))
                .bind(state.config.idle_timeout.as_secs() as i32)
                .bind(input.id)
                .fetch_one(authority.connection())
                .await
                .map_err(database)?;
        authority.ensure_current(false).await?;
        authority.finish().await?;
        let saved = saved.ok_or(AppError::NotFound)?.0;
        if saved.id <= 0
            || saved.profile <= 0
            || !(0..=3).contains(&saved.state)
            || saved.address.parse::<std::net::IpAddr>().is_err()
            || !capcode(saved.reply_capcode.as_deref())
        {
            return Err(AppError::Internal);
        }
        let view = Detail {
            date: date(if saved.reply.is_some() {
                saved.updated_at
            } else {
                saved.created_at
            })?,
            back: link(saved.profile, i32::from(saved.state), None, 0),
            saved,
        };
        return Ok(Html(view.render().map_err(|_| AppError::Internal)?));
    }
    let sqlx::types::Json(saved): sqlx::types::Json<SavedPage> =
        sqlx::query_scalar("SELECT submission_api.feedback_review_page($1,$2,$3,$4,$5,$6,$7)")
            .bind(&token)
            .bind(auth::hash(&csrf))
            .bind(state.config.idle_timeout.as_secs() as i32)
            .bind(profile)
            .bind(selected)
            .bind(input.cat.map(i32::from))
            .bind(before)
            .fetch_one(authority.connection())
            .await
            .map_err(database)?;
    let recent = authority.session.recent;
    authority.ensure_current(false).await?;
    authority.finish().await?;
    if saved.rows.len() > 25
        || saved.categories.len() > 32
        || saved.profile.is_some_and(|id| id <= 0)
        || saved.next < 0
        || !(0..=100000).contains(&saved.draft_count)
    {
        return Err(AppError::Internal);
    }
    let profile = saved.profile.unwrap_or(0);
    let mut rows = Vec::with_capacity(saved.rows.len());
    for row in saved.rows {
        if row.id <= 0 || !capcode(row.reply_capcode.as_deref()) {
            return Err(AppError::Internal);
        }
        rows.push(Row {
            date: date(row.updated_at)?,
            href: format!("/feedback?action=read&id={}", row.id),
            saved: row,
        });
    }
    let mut categories = Vec::with_capacity(saved.categories.len());
    for category in saved.categories {
        if !(0..=100000).contains(&category.count) {
            return Err(AppError::Internal);
        }
        categories.push(Category {
            href: link(profile, 0, Some(category.id), 0),
            active: input.cat == Some(category.id),
            saved: category,
        });
    }
    Ok(Html(
        Page {
            profile,
            status: selected,
            category: input.cat.map_or_else(String::new, |id| id.to_string()),
            csrf,
            recent,
            can_dismiss: saved.can_dismiss,
            is_admin: saved.is_admin,
            rows,
            categories,
            draft_count: saved.draft_count,
            next: if saved.next > 0 {
                link(profile, selected, input.cat, saved.next)
            } else {
                String::new()
            },
            all: link(profile, 0, None, 0),
            answered: link(profile, 1, None, 0),
            dismissed: link(profile, 2, None, 0),
            drafts: link(profile, 3, None, 0),
        }
        .render()
        .map_err(|_| AppError::Internal)?,
    ))
}
pub(crate) async fn action(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Form(input): Form<Action>,
) -> Result<Response, AppError> {
    auth::origin(&headers, &state.config.origin)?;
    let selected = status(input.answered.as_deref())?;
    if input.id <= 0 || input.profile < 0 || !matches!(input.action.as_str(), "dismiss" | "approve")
    {
        return Err(AppError::Invalid);
    }
    let mut authority = auth::guard(&state, &headers).await?;
    auth::csrf(&authority.session, &input.csrf)?;
    let token = auth::hash(&auth::cookie(&headers, state.config.cookie_name())?);
    let changed: bool =
        sqlx::query_scalar("SELECT submission_api.feedback_review_action($1,$2,$3,$4,$5)")
            .bind(token)
            .bind(auth::hash(&input.csrf))
            .bind(state.config.idle_timeout.as_secs() as i32)
            .bind(input.id)
            .bind(input.action)
            .fetch_one(authority.connection())
            .await
            .map_err(database)?;
    if !changed {
        return Err(AppError::NotFound);
    }
    authority.ensure_current(true).await?;
    authority.finish().await?;
    Ok(Redirect::to(&link(input.profile, selected, input.cat, 0)).into_response())
}
pub(crate) async fn stylesheet() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "text/css; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        include_str!("../static/feedback-native.css"),
    )
}

#[cfg(test)]
mod source_tests;
