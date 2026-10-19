//! Protected request queue, review actions and the current account's statistics.
use crate::{AppError, AppState, access::Level, auth, report_post_snapshot, views::Preview};
use askama::Template;
use axum::{
    Form, Json,
    extract::{Query, State},
    http::HeaderMap,
    response::{Html, IntoResponse, Response},
};
use board_domain::{comment_markup::Tag, formatting::Token, word_break::WordPart};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Context {
    board: Option<String>,
    #[serde(default)]
    offset: u32,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StatsContext {
    #[serde(default)]
    offset: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Review {
    id: i64,
    csrf: String,
    decision: String,
    amend_tpl: Option<String>,
    ban_length: Option<String>,
    global: Option<String>,
    public_reason: Option<String>,
    private_reason: Option<String>,
    return_board: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedRequest {
    id: i64,
    board: String,
    post_id: i64,
    template_id: i64,
    template_name: String,
    warning: bool,
    account_id: i64,
    created_at: DateTime<Utc>,
    can_accept: bool,
    payload: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Choice {
    id: i64,
    name: String,
    global: bool,
    warning: bool,
    seconds: Option<i64>,
    available: bool,
    no_warn: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BoardCount {
    board: String,
    count: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedQueue {
    requests: Vec<SavedRequest>,
    templates: Vec<Choice>,
    counts: Vec<BoardCount>,
    offset: u32,
    previous_offset: u32,
    next_offset: u32,
}
#[derive(Debug, PartialEq, Serialize)]
struct QuickReview {
    label: &'static str,
    template: i64,
    days: i32,
    global: &'static str,
}

fn quick_reviews(template_id: i64, warning: bool, templates: &[Choice]) -> Vec<QuickReview> {
    let mut actions = Vec::new();
    if let Some(template) = templates
        .iter()
        .find(|t| t.id == template_id && t.available)
    {
        let is_warning = warning || template.warning;
        if !is_warning && !template.no_warn {
            actions.push(QuickReview {
                label: "Warn",
                template: template_id,
                days: 0,
                global: "",
            });
        }
        if is_warning {
            actions.push(QuickReview {
                label: "Ban",
                template: template_id,
                days: 1,
                global: "1",
            });
        } else if !template.global
            && let Some(seconds) = template
                .seconds
                .filter(|s| *s >= 86400 && *s <= 1024 * 86400 && *s % 86400 == 0)
        {
            actions.push(QuickReview {
                label: "Global",
                template: template_id,
                days: (seconds / 86400) as i32,
                global: "1",
            });
        }
    }
    if template_id == 135 && templates.iter().any(|t| t.id == 207 && t.available) {
        actions.push(QuickReview {
            label: "Short",
            template: 207,
            days: 1,
            global: "1",
        });
    }
    actions
}

struct Request {
    id: i64,
    template_id: i64,
    template_name: String,
    warning: bool,
    default_warning: bool,
    quick_reviews: Vec<QuickReview>,
    account_id: i64,
    created_at: String,
    can_accept: bool,
    preview: Preview,
}
#[derive(Template)]
#[template(path = "request_queue.html")]
struct Page {
    board: String,
    csrf: String,
    recent: bool,
    requests: Vec<Request>,
    templates: Vec<Choice>,
    counts: Vec<BoardCount>,
    offset: u32,
    previous_offset: u32,
    next_offset: u32,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Outcome {
    changed: bool,
    ban_id: Option<i64>,
    cleared: u32,
}
#[derive(Template)]
#[template(path = "request_review_result.html")]
struct ResultPage {
    outcome: Outcome,
    approved: bool,
    board: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Totals {
    approved: u64,
    denied: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct History {
    id: i64,
    request_id: i64,
    account_id: i64,
    reviewer_id: i64,
    board: String,
    post_id: i64,
    requested_template: i64,
    accepted_template: Option<i64>,
    approved: bool,
    warning_action: Option<bool>,
    ban_id: Option<i64>,
    created_at: DateTime<Utc>,
}
#[derive(Deserialize, Template)]
#[serde(deny_unknown_fields)]
#[template(path = "request_statistics.html")]
struct Statistics {
    account_id: i64,
    totals: Totals,
    pending: u32,
    reviewed: Totals,
    history: Vec<History>,
    offset: u32,
    previous_offset: u32,
    next_offset: u32,
}
fn board(value: Option<&str>) -> Result<Option<&str>, AppError> {
    match value.filter(|value| !value.is_empty()) {
        None => Ok(None),
        Some(value)
            if value.len() <= 10
                && value
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()) =>
        {
            Ok(Some(value))
        }
        _ => Err(AppError::Invalid),
    }
}
fn optional_number<T: std::str::FromStr>(value: Option<&str>) -> Result<Option<T>, AppError> {
    value
        .filter(|value| !value.is_empty())
        .map(|value| value.parse().map_err(|_| AppError::Invalid))
        .transpose()
}
pub(crate) async fn queue(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(input): Query<Context>,
) -> Result<Html<String>, AppError> {
    let filter = board(input.board.as_deref())?;
    if input.offset > 100000 {
        return Err(AppError::Invalid);
    }
    let mut authority = auth::guard(&state, &headers).await?;
    let csrf = auth::cookie(&headers, &crate::handlers::csrf_cookie(&state))?;
    auth::csrf(&authority.session, &csrf)?;
    let token = auth::hash(&auth::cookie(&headers, state.config.cookie_name())?);
    let sqlx::types::Json(saved): sqlx::types::Json<SavedQueue> =
        sqlx::query_scalar("SELECT staff_identity.janitor_request_queue($1,$2,$3,$4,$5)")
            .bind(token)
            .bind(auth::hash(&csrf))
            .bind(state.config.idle_timeout.as_secs() as i32)
            .bind(filter)
            .bind(input.offset as i32)
            .fetch_one(authority.connection())
            .await
            .map_err(crate::janitor_requests::database)?;
    let recent = authority.session.recent;
    authority.ensure_current(false).await?;
    authority.finish().await?;
    let mut requests = Vec::with_capacity(saved.requests.len());
    for request in saved.requests {
        let default_warning = request.warning
            || saved
                .templates
                .iter()
                .any(|choice| choice.id == request.template_id && choice.warning);
        let preview = report_post_snapshot::request_preview(
            request.id,
            request.board,
            request.post_id,
            request.created_at,
            request.payload,
        )?;
        requests.push(Request {
            id: request.id,
            template_id: request.template_id,
            template_name: request.template_name,
            warning: request.warning,
            default_warning,
            quick_reviews: quick_reviews(request.template_id, request.warning, &saved.templates),
            account_id: request.account_id,
            created_at: request.created_at.format("%m/%d/%y %H:%M").to_string(),
            can_accept: request.can_accept,
            preview,
        });
    }
    Ok(Html(
        Page {
            board: filter.unwrap_or("").into(),
            csrf,
            recent,
            requests,
            templates: saved.templates,
            counts: saved.counts,
            offset: saved.offset,
            previous_offset: saved.previous_offset,
            next_offset: saved.next_offset,
        }
        .render()
        .map_err(|_| AppError::Internal)?,
    ))
}
pub(crate) async fn review(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Form(input): Form<Review>,
) -> Result<Response, AppError> {
    auth::origin(&headers, &state.config.origin)?;
    let return_board = board(input.return_board.as_deref())?.unwrap_or("");
    if input.id <= 0 {
        return Err(AppError::Invalid);
    }
    let approved = match input.decision.as_str() {
        "accept" => true,
        "deny" => false,
        _ => return Err(AppError::Invalid),
    };
    let template: Option<i64> = input
        .amend_tpl
        .as_deref()
        .map(|value| value.parse().map_err(|_| AppError::Invalid))
        .transpose()?;
    let days = optional_number::<i32>(input.ban_length.as_deref())?;
    let global = match input.global.as_deref() {
        None | Some("") => None,
        Some("0") => Some(false),
        Some("1") => Some(true),
        _ => return Err(AppError::Invalid),
    };
    let session = auth::session(&state, &headers).await?;
    if !session.at_least(Level::Moderator) {
        return Err(AppError::Forbidden);
    }
    auth::csrf(&session, &input.csrf)?;
    if !session.recent {
        return Err(AppError::Recent);
    }
    let token = auth::hash(&auth::cookie(&headers, state.config.cookie_name())?);
    let sqlx::types::Json(outcome): sqlx::types::Json<Outcome> =
        sqlx::query_scalar("SELECT content.review_janitor_request($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
            .bind(token)
            .bind(auth::hash(&input.csrf))
            .bind(state.config.idle_timeout.as_secs() as i32)
            .bind(input.id)
            .bind(approved)
            .bind(template)
            .bind(days)
            .bind(global)
            .bind(input.public_reason)
            .bind(input.private_reason)
            .fetch_one(&state.staff)
            .await
            .map_err(crate::janitor_requests::database)?;
    if headers.get("accept").and_then(|value| value.to_str().ok()) == Some("application/json") {
        return Ok(Json(outcome).into_response());
    }
    Ok(Html(
        ResultPage {
            outcome,
            approved,
            board: return_board.into(),
        }
        .render()
        .map_err(|_| AppError::Internal)?,
    )
    .into_response())
}
pub(crate) async fn statistics(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    Query(input): Query<StatsContext>,
) -> Result<Html<String>, AppError> {
    if input.offset > 100000 {
        return Err(AppError::Invalid);
    }
    let mut authority = auth::guard(&state, &headers).await?;
    let csrf = auth::cookie(&headers, &crate::handlers::csrf_cookie(&state))?;
    auth::csrf(&authority.session, &csrf)?;
    let token = auth::hash(&auth::cookie(&headers, state.config.cookie_name())?);
    let sqlx::types::Json(saved): sqlx::types::Json<Statistics> =
        sqlx::query_scalar("SELECT staff_identity.janitor_request_statistics($1,$2,$3,$4)")
            .bind(token)
            .bind(auth::hash(&csrf))
            .bind(state.config.idle_timeout.as_secs() as i32)
            .bind(input.offset as i32)
            .fetch_one(authority.connection())
            .await
            .map_err(crate::janitor_requests::database)?;
    authority.ensure_current(false).await?;
    authority.finish().await?;
    Ok(Html(saved.render().map_err(|_| AppError::Internal)?))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shortcut_payloads_match_pinned_source_cases() {
        #[derive(Deserialize)]
        struct Case {
            id: i64,
            warning: bool,
            templates: Vec<Choice>,
            expected: serde_json::Value,
        }
        let cases: Vec<Case> = serde_json::from_str(include_str!(
            "../../../fixtures/janitor-shortcut-cases.json"
        ))
        .unwrap();
        assert_eq!(cases.len(), 160);
        for case in cases {
            assert_eq!(
                serde_json::to_value(quick_reviews(case.id, case.warning, &case.templates))
                    .unwrap(),
                case.expected
            );
        }
    }

    #[test]
    fn shortcuts_require_available_configuration_and_exact_days() {
        let mut choices = vec![
            Choice {
                id: 135,
                name: "Owned".into(),
                global: false,
                warning: false,
                seconds: Some(86401),
                available: true,
                no_warn: false,
            },
            Choice {
                id: 207,
                name: "Owned short".into(),
                global: true,
                warning: false,
                seconds: Some(86400),
                available: false,
                no_warn: false,
            },
        ];
        assert_eq!(
            quick_reviews(135, false, &choices)
                .iter()
                .map(|a| a.label)
                .collect::<Vec<_>>(),
            vec!["Warn"]
        );
        choices[0].available = false;
        assert!(quick_reviews(135, false, &choices).is_empty());
        assert!(quick_reviews(42, false, &choices).is_empty());
        choices[1].available = true;
        assert_eq!(quick_reviews(135, false, &choices)[0].label, "Short");
    }

    #[test]
    fn filters_and_optional_amendments_are_canonical() {
        assert_eq!(board(Some("")).unwrap(), None);
        assert_eq!(board(Some("r9k")).unwrap(), Some("r9k"));
        for value in ["../g", "A", "g&offset=1"] {
            assert!(board(Some(value)).is_err());
        }
        assert_eq!(optional_number::<i32>(Some("")).unwrap(), None);
        assert_eq!(optional_number::<i32>(Some("-1")).unwrap(), Some(-1));
        assert!(optional_number::<i32>(Some("1.5")).is_err());
    }
}
