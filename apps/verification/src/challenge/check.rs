//! Exchange a solved, action-bound challenge for a short-lived write receipt.
//! The protected database write remains responsible for consuming that receipt.
use crate::{AppState, access::error, handlers};
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use board_domain::challenge_secrets::{ReceiptToken, TokenKind, VerificationToken};
use board_store::verification::{ChallengeCheck, ChallengePurpose};
use board_verification_client::protocol;

pub(crate) async fn check(
    State(state): State<AppState>,
    Json(input): Json<protocol::Answer>,
) -> Response {
    let Some(fingerprints) = handlers::session(&input.context.session) else {
        return error(StatusCode::BAD_REQUEST);
    };
    if input.context.email_token.is_some() {
        return error(StatusCode::BAD_REQUEST);
    }
    let (purpose, name) = match input.context.purpose {
        protocol::Purpose::Thread => (ChallengePurpose::Thread, "thread"),
        protocol::Purpose::Reply => (ChallengePurpose::Reply, "reply"),
        protocol::Purpose::Report => (ChallengePurpose::Report, "report"),
        _ => return error(StatusCode::BAD_REQUEST),
    };
    let Some(context_hash) = state.key.form_context(
        name,
        input.context.board.as_deref(),
        input.context.target,
        &input.context.form_nonce,
    ) else {
        return error(StatusCode::BAD_REQUEST);
    };
    let Some(token) = VerificationToken::parse(TokenKind::Challenge, &input.challenge) else {
        return Json(protocol::AnswerResult::Rejected {}).into_response();
    };
    let Ok(receipt) = ReceiptToken::generate() else {
        return error(StatusCode::SERVICE_UNAVAILABLE);
    };
    let peer_hash = state.key.network_identity(input.context.session.peer);
    let answer_hash = state.key.answer_hash(&token, &input.answer);
    match state
        .store
        .check_challenge(&ChallengeCheck {
            token_hash: &state.key.token_hash(&token),
            answer_hash: answer_hash.as_ref(),
            peer_hash: &peer_hash,
            session_hash: &fingerprints.token,
            purpose,
            board: input.context.board.as_deref(),
            target: input.context.target,
            context_hash: &context_hash,
            receipt_hash: &receipt.storage_hash(),
        })
        .await
    {
        Ok(true) => Json(protocol::AnswerResult::Verified {
            receipt: receipt.encode(),
            peer_hash,
            context_hash,
        })
        .into_response(),
        Ok(false) => Json(protocol::AnswerResult::Rejected {}).into_response(),
        Err(_) => error(StatusCode::SERVICE_UNAVAILABLE),
    }
}
