//! Reconstructed /banned controller: private matching, native warning viewing.
use crate::{
    AppState,
    handlers::AppError,
    security::{RequestPeer, RequestStart},
};
use askama::Template;
use axum::{
    Extension, Form, Router,
    extract::{DefaultBodyLimit, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
    routing::get,
};
use board_domain::{
    BoardSlug, anonymous_session::Capability, poster_id::PublicPostingRateIdentity,
};
use board_store::restrictions::Status;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Context {
    board: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ViewForm {
    board: String,
    id: i64,
    csrf: String,
}

#[derive(Template)]
#[template(path = "restrictions.html")]
struct Page {
    board: String,
    rows: Vec<Status>,
    csrf: String,
}

#[derive(Template)]
#[template(path = "restriction-error.html")]
struct Refusal<'a> {
    board: &'a str,
    warning: bool,
}

pub(crate) fn posting_refusal(
    state: &AppState,
    board: &str,
    warning: bool,
) -> Result<Response, AppError> {
    BoardSlug::parse(board).map_err(|_| unavailable())?;
    let mut response = private(crate::output::html(state, &Refusal { board, warning })?);
    *response.status_mut() = StatusCode::UNPROCESSABLE_ENTITY;
    response.headers_mut().insert(
        "refresh",
        HeaderValue::from_str(&format!("3; url=/banned?board={board}"))
            .map_err(|_| unavailable())?,
    );
    Ok(response)
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new().route(
        "/banned",
        get(show).post(view).layer(DefaultBodyLimit::max(1024)),
    )
}

fn unavailable() -> AppError {
    AppError(
        StatusCode::SERVICE_UNAVAILABLE,
        "Restriction status is unavailable. Try again later.",
    )
}
fn invalid() -> AppError {
    AppError(
        StatusCode::FORBIDDEN,
        "Reload the warning page before viewing a warning.",
    )
}
fn cookie_name(production: bool) -> &'static str {
    if production {
        "__Host-board-warning"
    } else {
        "board-warning"
    }
}
fn csrf(cap: &Capability) -> String {
    cap.storage_hash()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn view_cookie(headers: &HeaderMap, production: bool) -> Result<Capability, AppError> {
    let mut found = None;
    let mut size = 0usize;
    for value in headers.get_all(header::COOKIE) {
        size += value.as_bytes().len();
        if size > 8192 {
            return Err(invalid());
        }
        for pair in value.to_str().map_err(|_| invalid())?.split(';') {
            if let Some((name, value)) = pair.trim().split_once('=')
                && name == cookie_name(production)
            {
                if found.is_some() {
                    return Err(invalid());
                }
                found = Some(Capability::parse(value).ok_or_else(invalid)?);
            }
        }
    }
    found.ok_or_else(invalid)
}

fn actor(
    state: &AppState,
    peer: RequestPeer,
    board: &str,
) -> Result<PublicPostingRateIdentity, AppError> {
    BoardSlug::parse(board).map_err(|_| AppError(StatusCode::BAD_REQUEST, "Invalid board."))?;
    Ok(state
        .poster_id_key
        .as_ref()
        .ok_or_else(unavailable)?
        .public_posting_rate_identity(peer.0.ok_or_else(unavailable)?))
}

fn private(mut response: Response) -> Response {
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response
}

async fn show(
    State(state): State<AppState>,
    Extension(peer): Extension<RequestPeer>,
    Extension(start): Extension<RequestStart>,
    headers: HeaderMap,
    Query(context): Query<Context>,
) -> Result<Response, AppError> {
    let actor = actor(&state, peer, &context.board)?;
    let token =
        crate::anonymous_session::cookie(&headers, state.production)?.map(|cap| cap.storage_hash());
    let rows = board_store::restrictions::status(
        &state.pool,
        &actor,
        token.as_ref(),
        &context.board,
        start.0.timestamp(),
    )
    .await?;
    let cap = Capability::generate().map_err(|_| unavailable())?;
    let mut response = private(crate::output::html(
        &state,
        &Page {
            board: context.board,
            rows,
            csrf: csrf(&cap),
        },
    )?);
    let secure = if state.production { "; Secure" } else { "" };
    response.headers_mut().append(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "{}={}; Path=/; HttpOnly; SameSite=Strict{secure}",
            cookie_name(state.production),
            cap.credential()
        ))
        .map_err(|_| unavailable())?,
    );
    Ok(response)
}

async fn view(
    State(state): State<AppState>,
    Extension(peer): Extension<RequestPeer>,
    Extension(start): Extension<RequestStart>,
    headers: HeaderMap,
    Form(form): Form<ViewForm>,
) -> Result<Response, AppError> {
    // The shared middleware checks same-origin state-changing requests. The
    // separate HttpOnly viewing cookie grants no posting/deletion equality.
    let cap = view_cookie(&headers, state.production)?;
    if form.csrf.len() != 64 || form.csrf != csrf(&cap) || form.id <= 0 {
        return Err(invalid());
    }
    let actor = actor(&state, peer, &form.board)?;
    let token =
        crate::anonymous_session::cookie(&headers, state.production)?.map(|cap| cap.storage_hash());
    let changed = board_store::restrictions::acknowledge(
        &state.pool,
        &actor,
        token.as_ref(),
        &form.board,
        start.0.timestamp(),
        form.id,
    )
    .await?;
    if !changed {
        return Err(AppError(
            StatusCode::CONFLICT,
            "This warning is no longer available.",
        ));
    }
    Ok(private(
        Redirect::to(&format!("/banned?board={}", form.board)).into_response(),
    ))
}
