//! A contest vote binds a live Pass entitlement to one entry and form. It does
//! not spend CAPTCHA credit or extend a posting-network hold.
use crate::{AppState, access::error};
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use board_domain::challenge_secrets::{PassSessionToken, ReceiptToken};
use board_store::verification::pass::SiteVoteRequest;
use board_verification_client::protocol;

pub(crate) async fn issue(state: AppState, input: protocol::PassWriteRequest) -> Response {
    let context = &input.context;
    if context.purpose != protocol::Purpose::SiteContestVote
        || context.session.minted
        || context.email_token.is_some()
        || context.board.is_some()
    {
        return error(StatusCode::BAD_REQUEST);
    }
    let Some(fp) = crate::handlers::session(&context.session) else {
        return error(StatusCode::BAD_REQUEST);
    };
    let Some(hash) = state.key.form_context(
        "site_contest_vote",
        None,
        context.target,
        &context.form_nonce,
    ) else {
        return error(StatusCode::BAD_REQUEST);
    };
    let Some(token) = PassSessionToken::parse(&input.token) else {
        return Json(protocol::PassWriteResult::None {}).into_response();
    };
    let Ok(receipt) = ReceiptToken::generate() else {
        return error(StatusCode::SERVICE_UNAVAILABLE);
    };
    let peer = state.key.network_identity(context.session.peer);
    let result = match state
        .store
        .pass_site_vote_receipt(&SiteVoteRequest {
            pass_session: &token.storage_hash(),
            receipt: &receipt.storage_hash(),
            peer: &peer,
            anonymous: &fp.token,
            context: &hash,
            entry: context.target,
        })
        .await
    {
        Ok(result) => result,
        Err(board_store::StoreError::Database(ref error))
            if error.as_database_error().and_then(|e| e.code()).as_deref() == Some("P0166") =>
        {
            return Json(protocol::PassWriteResult::None {}).into_response();
        }
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE),
    };
    let output = match result.status.as_str() {
        "ready" => protocol::PassWriteResult::Verified {
            receipt: receipt.encode(),
            peer_hash: peer,
            context_hash: hash,
        },
        "none" => protocol::PassWriteResult::None {},
        "expired" => protocol::PassWriteResult::Expired {},
        "disabled" => protocol::PassWriteResult::Disabled {},
        "capacity" => protocol::PassWriteResult::Capacity {},
        _ => return error(StatusCode::SERVICE_UNAVAILABLE),
    };
    Json(output).into_response()
}
