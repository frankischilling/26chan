//! Private application review. Initial pages never fetch applicant identity.
use crate::{AppError, AppState, auth};
use askama::Template;
use axum::{
    Form,
    extract::{Query, State},
    http::HeaderMap,
    response::{Html, IntoResponse, Redirect, Response},
};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Context {
    campaign: Option<i64>,
    state: Option<i32>,
    before: Option<i64>,
    id: Option<i64>,
    identity: Option<bool>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Action {
    id: i64,
    revision: i32,
    action: String,
    csrf: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Summary {
    id: i64,
    state: i16,
    revision: i32,
    age: i16,
    timezone: i16,
    hours: i16,
    board1: String,
    board2: Option<String>,
    created_at: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedPage {
    campaign: Option<i64>,
    rows: Vec<Summary>,
    next: i64,
    is_admin: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedDetail {
    id: i64,
    campaign: i64,
    state: i16,
    revision: i32,
    age: i16,
    timezone: i16,
    hours: i16,
    times: String,
    board1: String,
    board2: Option<String>,
    answers: [String; 4],
    questions: [String; 4],
    created_at: i64,
    updated_at: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    id: i64,
    firstname: String,
    handle: String,
    email: String,
}
#[derive(Template)]
#[template(path = "applications.html")]
struct Page {
    saved: SavedPage,
    campaign: i64,
    selected: i32,
    next: String,
}
#[derive(Template)]
#[template(path = "application-detail.html")]
struct Detail {
    saved: SavedDetail,
    identity: Option<Identity>,
    csrf: String,
    is_admin: bool,
    recent: bool,
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
fn valid_row(id: i64, state: i16, revision: i32, age: i16, timezone: i16, hours: i16) -> bool {
    id > 0
        && (0..=4).contains(&state)
        && (1..=1_000_000).contains(&revision)
        && (1..=150).contains(&age)
        && (-12..=13).contains(&timezone)
        && (1..=24).contains(&hours)
}
fn board(value: &str) -> bool {
    board_domain::BoardSlug::parse(value).is_ok()
}
pub(crate) async fn page(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(input): Query<Context>,
) -> Result<Html<String>, AppError> {
    let campaign = input.campaign.unwrap_or(0);
    let selected = input.state.unwrap_or(0);
    let before = input.before.unwrap_or(0);
    if campaign < 0
        || !(0..=4).contains(&selected)
        || before < 0
        || input.id.is_some_and(|id| id <= 0)
        || input.id.is_some()
            && (input.campaign.is_some() || input.state.is_some() || input.before.is_some())
        || input.identity.is_some() && input.id.is_none()
    {
        return Err(AppError::Invalid);
    }
    let mut authority = auth::guard(&state, &headers).await?;
    let csrf = auth::cookie(&headers, &crate::handlers::csrf_cookie(&state))?;
    auth::csrf(&authority.session, &csrf)?;
    let token = auth::hash(&auth::cookie(&headers, state.config.cookie_name())?);
    let idle = state.config.idle_timeout.as_secs() as i32;
    if let Some(id) = input.id {
        let saved: Option<sqlx::types::Json<SavedDetail>> =
            sqlx::query_scalar("SELECT submission_api.application_review_detail($1,$2,$3,$4)")
                .bind(&token)
                .bind(auth::hash(&csrf))
                .bind(idle)
                .bind(id)
                .fetch_one(authority.connection())
                .await
                .map_err(database)?;
        let saved = saved.ok_or(AppError::NotFound)?.0;
        let identity = if input.identity == Some(true) {
            let value: Option<sqlx::types::Json<Identity>> = sqlx::query_scalar(
                "SELECT submission_api.application_selected_identity($1,$2,$3,$4)",
            )
            .bind(&token)
            .bind(auth::hash(&csrf))
            .bind(idle)
            .bind(id)
            .fetch_one(authority.connection())
            .await
            .map_err(database)?;
            Some(value.ok_or(AppError::NotFound)?.0)
        } else {
            None
        };
        let is_admin = authority.session.role == "admin";
        let recent = authority.session.recent;
        authority.ensure_current(identity.is_some()).await?;
        authority.finish().await?;
        if saved.id != id
            || saved.campaign <= 0
            || !valid_row(
                saved.id,
                saved.state,
                saved.revision,
                saved.age,
                saved.timezone,
                saved.hours,
            )
            || !board(&saved.board1)
            || saved.board2.as_deref().is_some_and(|b| !board(b))
            || saved.created_at < 0
            || saved.updated_at < saved.created_at
            || identity.as_ref().is_some_and(|i| i.id != id)
        {
            return Err(AppError::Internal);
        }
        return Ok(Html(
            Detail {
                saved,
                identity,
                csrf,
                is_admin,
                recent,
            }
            .render()
            .map_err(|_| AppError::Internal)?,
        ));
    }
    let sqlx::types::Json(saved): sqlx::types::Json<SavedPage> =
        sqlx::query_scalar("SELECT submission_api.application_review_page($1,$2,$3,$4,$5,$6)")
            .bind(token)
            .bind(auth::hash(&csrf))
            .bind(idle)
            .bind(campaign)
            .bind(selected)
            .bind(before)
            .fetch_one(authority.connection())
            .await
            .map_err(database)?;
    authority.ensure_current(false).await?;
    authority.finish().await?;
    if saved.rows.len() > 25
        || saved.campaign.is_some_and(|id| id <= 0)
        || saved.next < 0
        || saved.rows.iter().any(|r| {
            !valid_row(r.id, r.state, r.revision, r.age, r.timezone, r.hours)
                || i32::from(r.state) != selected
                || r.created_at < 0
                || !board(&r.board1)
                || r.board2.as_deref().is_some_and(|b| !board(b))
        })
    {
        return Err(AppError::Internal);
    }
    let campaign = saved.campaign.unwrap_or(0);
    let next = if saved.next > 0 {
        format!(
            "/applications?campaign={campaign}&state={selected}&before={}",
            saved.next
        )
    } else {
        String::new()
    };
    Ok(Html(
        Page {
            saved,
            campaign,
            selected,
            next,
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
    if input.id <= 0
        || !(1..=1_000_000).contains(&input.revision)
        || !matches!(input.action.as_str(), "accept" | "decline")
    {
        return Err(AppError::Invalid);
    }
    let mut authority = auth::guard(&state, &headers).await?;
    auth::csrf(&authority.session, &input.csrf)?;
    let token = auth::hash(&auth::cookie(&headers, state.config.cookie_name())?);
    let changed: bool =
        sqlx::query_scalar("SELECT submission_api.application_review_action($1,$2,$3,$4,$5,$6)")
            .bind(token)
            .bind(auth::hash(&input.csrf))
            .bind(state.config.idle_timeout.as_secs() as i32)
            .bind(input.id)
            .bind(input.revision)
            .bind(input.action)
            .fetch_one(authority.connection())
            .await
            .map_err(database)?;
    if !changed {
        return Err(AppError::NotFound);
    }
    authority.ensure_current(true).await?;
    authority.finish().await?;
    Ok(Redirect::to(&format!("/applications?id={}", input.id)).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn detail() -> Detail {
        Detail {
            saved: SavedDetail {
                id: 1,
                campaign: 2,
                state: 0,
                revision: 3,
                age: 21,
                timezone: -4,
                hours: 3,
                times: "<script>times</script>".into(),
                board1: "g".into(),
                board2: None,
                answers: std::array::from_fn(|_| "<img src=x onerror=alert(1)>".into()),
                questions: std::array::from_fn(|_| "<question>".into()),
                created_at: 1,
                updated_at: 2,
            },
            identity: None,
            csrf: "owned-csrf".into(),
            is_admin: false,
            recent: true,
        }
    }
    #[test]
    fn blind_review_escapes_answers_and_never_renders_selection_for_non_admins() {
        let mut view = detail();
        let html = view.render().unwrap();
        assert!(html.contains("&#60;script&#62;"));
        assert!(html.contains("&#60;img"));
        assert!(!html.contains("<script>"));
        assert!(!html.contains("name=\"action\""));
        assert!(!html.contains("identity=true"));
        view.is_admin = true;
        let html = view.render().unwrap();
        assert!(html.contains("value=\"accept\""));
        assert!(html.contains("name=\"revision\" value=\"3\""));
        view.recent = false;
        assert!(view.render().unwrap().contains("disabled"));
        view.saved.state = 2;
        let html = view.render().unwrap();
        assert!(!html.contains("name=\"action\""));
        assert!(html.contains("identity=true"));
        view.identity = Some(Identity {
            id: 1,
            firstname: "<name>".into(),
            handle: "<handle>".into(),
            email: "owned@example.invalid".into(),
        });
        let html = view.render().unwrap();
        assert!(html.contains("&#60;name&#62;"));
        assert!(!html.contains("<name>"));
    }
}
