//! Like proofs do not borrow posting credits, tickets or disabled-board bypass.
use crate::{AppState, access::error, handlers};
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use board_domain::challenge_policy::ImageStyle;
use board_store::verification::ChallengePurpose;
use board_verification_client::protocol;

pub(super) async fn challenge(
    state: AppState,
    input: protocol::PostingChallengeRequest,
) -> Response {
    let context = &input.context;
    let Some(fp) = handlers::session(&context.session) else {
        return error(StatusCode::BAD_REQUEST);
    };
    if context.email_token.is_some() || input.ticket.is_some() || input.ticket_answer.is_some() {
        return error(StatusCode::BAD_REQUEST);
    }
    let Some(hash) = state.key.form_context(
        "like",
        context.board.as_deref(),
        context.target,
        &context.form_nonce,
    ) else {
        return error(StatusCode::BAD_REQUEST);
    };
    let Some(board) = context.board.as_deref() else {
        return error(StatusCode::BAD_REQUEST);
    };
    match state.store.score_event_open().await {
        Ok(true) => (),
        Ok(false) => return error(StatusCode::NOT_FOUND),
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE),
    }
    match state.store.board_policy(board).await {
        Ok(Some(_)) => (),
        Ok(None) => return error(StatusCode::BAD_REQUEST),
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE),
    }
    if state
        .store
        .ensure_session(&fp, context.session.minted)
        .await
        .is_err()
    {
        return error(StatusCode::SERVICE_UNAVAILABLE);
    }
    let peer = state.key.network_identity(context.session.peer);
    let risk = match state.store.risk(&peer, None).await {
        Ok(value) => value,
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE),
    };
    let Ok(permit) = state.rendering.clone().try_acquire_owned() else {
        return error(StatusCode::TOO_MANY_REQUESTS);
    };
    let style = ImageStyle {
        slider: false,
        characters: if risk.suspicious() { 8 } else { 6 },
    };
    match super::issue::prepare(
        &state,
        context,
        ChallengePurpose::Like,
        &hash,
        style,
        risk.suspicious(),
        permit,
    )
    .await
    {
        Ok(challenge) => Json(protocol::PostingChallengeResult {
            challenge,
            ticket_challenge: false,
        })
        .into_response(),
        Err(status) => error(status),
    }
}
