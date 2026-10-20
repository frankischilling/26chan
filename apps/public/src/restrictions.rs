//! Reconstructed /banned controller: private matching, native warning viewing.
use crate::{
    AppState,
    handlers::AppError,
    security::{RequestPeer, RequestStart},
};
use crate::{anonymous_session::Session, posting_challenge::ProofError};
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
use board_store::restrictions::{MemberContext, Status};
use board_verification_client::protocol;
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

/// Status and acknowledgment get distinct, short-lived private receipts.
/// The warning cookie remains the browser's CSRF authority, never a Pass ID.
async fn member_proof(
    state: &AppState,
    headers: &HeaderMap,
    peer: RequestPeer,
    board: &str,
    target: i64,
    nonce: &str,
) -> Result<Option<(Session, board_store::WriteChallengeProof)>, ProofError> {
    let Some(token) = crate::pass_auth::read_cookie(headers, state.production)? else {
        return Ok(None);
    };
    let mut session = Session::resolve(state, headers, peer.0).await?;
    if target > 0 && session.posting.minted {
        return Err(invalid().into());
    }
    let service = state.verification.as_ref().ok_or_else(unavailable)?;
    service
        .ensure_session(&crate::signin::trusted(&session, state, peer.0)?)
        .await
        .map_err(|_| unavailable())?;
    session.posting.minted = false;
    let result = service
        .pass_restriction_receipt(&protocol::PassRestrictionRequest {
            token,
            session: crate::signin::trusted(&session, state, peer.0)?,
            board: board.to_owned(),
            action: if target == 0 {
                protocol::PassRestrictionAction::Status
            } else {
                protocol::PassRestrictionAction::WarningView
            },
            target,
            nonce: nonce.to_owned(),
        })
        .await
        .map_err(|_| unavailable())?;
    let proof = match result {
        protocol::PassWriteResult::Verified {
            receipt,
            peer_hash,
            context_hash,
        } => board_store::WriteChallengeProof::from_issuer(&receipt, peer_hash, context_hash)
            .ok_or_else(unavailable)?,
        protocol::PassWriteResult::Expired {} => {
            return Err(ProofError::pass_rejected(
                state.production,
                "This Pass has expired.",
            ));
        }
        protocol::PassWriteResult::Disabled {} => {
            return Err(ProofError::pass_rejected(
                state.production,
                "This Pass has been disabled.",
            ));
        }
        protocol::PassWriteResult::InUse {} => {
            return Err(ProofError::pass_rejected(
                state.production,
                "This Pass is already in use from another network.",
            ));
        }
        protocol::PassWriteResult::None {} => {
            return Err(ProofError::pass_rejected(
                state.production,
                "This Pass session is invalid. Authenticate again.",
            ));
        }
        protocol::PassWriteResult::Capacity {} => return Err(unavailable().into()),
    };
    Ok(Some((session, proof)))
}

async fn show(
    State(state): State<AppState>,
    Extension(peer): Extension<RequestPeer>,
    Extension(start): Extension<RequestStart>,
    headers: HeaderMap,
    Query(context): Query<Context>,
) -> Result<Response, ProofError> {
    let actor = actor(&state, peer, &context.board)?;
    let cap = Capability::generate().map_err(|_| unavailable())?;
    let member = member_proof(&state, &headers, peer, &context.board, 0, &csrf(&cap)).await?;
    let token =
        crate::anonymous_session::cookie(&headers, state.production)?.map(|cap| cap.storage_hash());
    let rows = if let Some((session, proof)) = member.as_ref() {
        board_store::restrictions::member_status(
            &state.pool,
            &MemberContext {
                actor: &actor,
                session: &session.posting,
                proof,
                board: &context.board,
                request_at: start.0.timestamp(),
            },
        )
        .await?
    } else {
        board_store::restrictions::status(
            &state.pool,
            &actor,
            token.as_ref(),
            &context.board,
            start.0.timestamp(),
        )
        .await?
    };
    let mut response = private(crate::output::html(
        &state,
        &Page {
            board: context.board,
            rows,
            csrf: csrf(&cap),
        },
    )?);
    if let Some((session, _)) = member.as_ref() {
        session.append(response.headers_mut(), state.production);
    }
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
    Ok(private(response))
}

async fn view(
    State(state): State<AppState>,
    Extension(peer): Extension<RequestPeer>,
    Extension(start): Extension<RequestStart>,
    headers: HeaderMap,
    Form(form): Form<ViewForm>,
) -> Result<Response, ProofError> {
    // The shared middleware checks same-origin state-changing requests. The
    // separate HttpOnly viewing cookie grants no posting/deletion equality.
    let cap = view_cookie(&headers, state.production)?;
    if form.csrf.len() != 64 || form.csrf != csrf(&cap) || form.id <= 0 {
        return Err(invalid().into());
    }
    let actor = actor(&state, peer, &form.board)?;
    let member = member_proof(&state, &headers, peer, &form.board, form.id, &form.csrf).await?;
    let token =
        crate::anonymous_session::cookie(&headers, state.production)?.map(|cap| cap.storage_hash());
    let changed = if let Some((session, proof)) = member.as_ref() {
        board_store::restrictions::member_acknowledge(
            &state.pool,
            &MemberContext {
                actor: &actor,
                session: &session.posting,
                proof,
                board: &form.board,
                request_at: start.0.timestamp(),
            },
            form.id,
        )
        .await?
    } else {
        board_store::restrictions::acknowledge(
            &state.pool,
            &actor,
            token.as_ref(),
            &form.board,
            start.0.timestamp(),
            form.id,
        )
        .await?
    };
    if !changed {
        return Err(AppError(StatusCode::CONFLICT, "This warning is no longer available.").into());
    }
    Ok(private(
        Redirect::to(&format!("/banned?board={}", form.board)).into_response(),
    ))
}
