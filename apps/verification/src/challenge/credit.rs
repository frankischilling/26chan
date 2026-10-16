//! Turn an eligible credit into a claim for the immediate protected write.
use crate::{AppState, access::error, handlers};
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use board_domain::challenge_secrets::ReceiptToken;
use board_store::verification::{ChallengePurpose, CreditRequest};
use board_verification_client::protocol;

pub(crate) async fn claim(
    State(state): State<AppState>,
    Json(context): Json<protocol::ChallengeContext>,
) -> Response {
    let Some(fp) = handlers::session(&context.session) else {
        return error(StatusCode::BAD_REQUEST);
    };
    if context.email_token.is_some() {
        return error(StatusCode::BAD_REQUEST);
    }
    let (purpose, name) = match context.purpose {
        protocol::Purpose::Reply => (ChallengePurpose::Reply, "reply"),
        protocol::Purpose::Report => (ChallengePurpose::Report, "report"),
        _ => return error(StatusCode::BAD_REQUEST),
    };
    let Some(hash) = state.key.form_context(
        name,
        context.board.as_deref(),
        context.target,
        &context.form_nonce,
    ) else {
        return error(StatusCode::BAD_REQUEST);
    };
    let Some(board) = context.board.as_deref() else {
        return error(StatusCode::BAD_REQUEST);
    };
    let Ok(receipt) = ReceiptToken::generate() else {
        return error(StatusCode::SERVICE_UNAVAILABLE);
    };
    let peer = state.key.network_identity(context.session.peer);
    match state
        .store
        .credit_receipt(&CreditRequest {
            receipt: Some(&receipt.storage_hash()),
            peer: &peer,
            fingerprints: &fp,
            board,
            target: context.target,
            purpose,
            context: &hash,
            request_time: chrono::Utc::now().timestamp(),
        })
        .await
    {
        Ok(result) if result.status == "ready" => Json(protocol::AnswerResult::Verified {
            receipt: receipt.encode(),
            peer_hash: peer,
            context_hash: hash,
        })
        .into_response(),
        Ok(result) if result.status == "ineligible" => {
            Json(protocol::AnswerResult::Rejected {}).into_response()
        }
        Err(board_store::StoreError::Database(ref error))
            if error.as_database_error().and_then(|e| e.code()).as_deref() == Some("P0166") =>
        {
            Json(protocol::AnswerResult::Rejected {}).into_response()
        }
        _ => error(StatusCode::SERVICE_UNAVAILABLE),
    }
}
