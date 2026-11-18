use super::applications::ApplicationPage;
use crate::{AppState, handlers::AppError, verification_form::AgreementSecret};
use askama::Template;
use axum::{
    Form, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode, header},
    response::Response,
    routing::get,
};
use board_domain::site_submissions::ownership::{Capability, Kind};
use board_store::site_submissions::agreements;
use serde::Deserialize;

pub(super) fn routes() -> Router<AppState> {
    Router::new().route(
        "/janitorapp/agreement",
        get(page).post(submit).layer(DefaultBodyLimit::max(256)),
    )
}

#[derive(Template)]
#[template(path = "application-agreement.html")]
enum Page<'a> {
    Enter {
        csrf: String,
    },
    Review {
        terms: &'a str,
        code: &'a str,
        csrf: String,
    },
    Accepted,
}

fn render(state: &AppState, page: &Page<'_>) -> Result<Response, AppError> {
    let mut response = crate::output::html(state, page)?;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "private, no-store".parse().unwrap());
    response.extensions_mut().insert(ApplicationPage(false));
    Ok(response)
}

async fn page(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, AppError> {
    let existing = AgreementSecret::read(&headers, state.production).map_err(|_| expired())?;
    let issue_cookie = existing.is_none();
    let secret = existing
        .map(Ok)
        .unwrap_or_else(AgreementSecret::generate)
        .map_err(|_| {
            AppError(
                StatusCode::SERVICE_UNAVAILABLE,
                "The agreement form is unavailable.",
            )
        })?;
    let mut response = render(
        &state,
        &Page::Enter {
            csrf: secret.nonce(),
        },
    )?;
    if issue_cookie {
        secret.append(response.headers_mut(), state.production);
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, "private, no-store".parse().unwrap());
    }
    Ok(response)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Submission {
    code: String,
    csrf: String,
    accept: Option<String>,
}

fn expired() -> AppError {
    AppError(
        StatusCode::FORBIDDEN,
        "This form expired. Open the agreement page again.",
    )
}

fn unavailable() -> AppError {
    AppError(StatusCode::NOT_FOUND, "This invitation is unavailable.")
}

async fn submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(input): Form<Submission>,
) -> Result<Response, AppError> {
    let secret = AgreementSecret::read(&headers, state.production)
        .map_err(|_| expired())?
        .ok_or_else(expired)?;
    let invite =
        Capability::parse(Kind::ApplicationAgreement, input.code.trim()).ok_or_else(unavailable)?;
    if let Some(acceptance) = input.accept {
        if acceptance != "yes" || !secret.matches_for(&input.csrf, invite.storage_hash()) {
            return Err(expired());
        }
        if !agreements::accept(&state.pool, &invite).await? {
            return Err(unavailable());
        }
        return render(&state, &Page::Accepted);
    }

    if !secret.matches(&input.csrf) {
        return Err(expired());
    }
    let terms = agreements::terms(&state.pool, &invite)
        .await?
        .ok_or_else(unavailable)?;
    render(
        &state,
        &Page::Review {
            terms: &terms,
            code: invite.token(),
            csrf: secret.nonce_for(invite.storage_hash()),
        },
    )
}
