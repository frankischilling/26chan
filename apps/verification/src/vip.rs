//! VIP verification shares the bounded hash worker pool, never staff authority.
use crate::{AppState, access::error};
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use board_domain::challenge_secrets::ReceiptToken;
use board_verification_client::protocol;

pub(crate) async fn ready(State(state): State<AppState>) -> Response {
    if state.store.ready().await.is_err() {
        return error(StatusCode::SERVICE_UNAVAILABLE);
    }
    match state.store.vip_ready().await {
        Ok(true) => Json(protocol::Readiness { ready: true }).into_response(),
        _ => error(StatusCode::SERVICE_UNAVAILABLE),
    }
}

pub(crate) async fn receipt(
    State(state): State<AppState>,
    Json(input): Json<protocol::VipWriteRequest>,
) -> Response {
    let Some(fingerprints) = crate::handlers::session(&input.session) else {
        return error(StatusCode::BAD_REQUEST);
    };
    if input.target < 0
        || board_domain::BoardSlug::parse(&input.board).is_err()
        || input.options.len() > board_domain::MAX_PUBLIC_FIELD_BYTES
    {
        return error(StatusCode::BAD_REQUEST);
    }
    let options = board_domain::posting_options::without_sage(&input.options);
    // The SQL issuer decides bypass eligibility from board policy and a checked
    // Pass receipt. This syntax parser cannot authorize that decision.
    let Some(credentials) = board_domain::vip_capcode::credentials(&options, true) else {
        return Json(protocol::VipWriteResult::None {}).into_response();
    };
    let Some(identifier) = state.key.vip_identifier(credentials.identifier()) else {
        return Json(protocol::VipWriteResult::None {}).into_response();
    };
    let Ok(permit) = state.pass_hashing.clone().try_acquire_owned() else {
        return Json(protocol::VipWriteResult::Capacity {}).into_response();
    };
    let (Ok(ticket), Ok(receipt), Ok(context)) = (
        ReceiptToken::generate(),
        ReceiptToken::generate(),
        ReceiptToken::generate(),
    ) else {
        return error(StatusCode::SERVICE_UNAVAILABLE);
    };
    // The nonce is internal, not a browser authority claim. SQL binds the random
    // context to the trusted peer, anonymous session, board and target.
    let context_hash = context.storage_hash();
    let peer_hash = state.key.network_identity(input.session.peer);
    let lookup = match state
        .store
        .begin_vip_login(&board_store::verification::vip::LoginRequest {
            identifier: &identifier,
            peer: &peer_hash,
            ticket: &ticket.storage_hash(),
            anonymous: &fingerprints.token,
            context: &context_hash,
            board: &input.board,
            target: input.target,
            pass_proof: input
                .pass_proof
                .as_ref()
                .map(|proof| (&proof.receipt_hash, &proof.context_hash)),
        })
        .await
    {
        Ok(value) => value,
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE),
    };
    match lookup.status.as_str() {
        "flood" | "ineligible" => {
            return Json(protocol::VipWriteResult::None {}).into_response();
        }
        "capacity" => return Json(protocol::VipWriteResult::Capacity {}).into_response(),
        "verify" => {}
        _ => return error(StatusCode::SERVICE_UNAVAILABLE),
    }
    let recognized = lookup
        .password_hash
        .as_ref()
        .is_some_and(|hash| crate::pass::bounded_hash(hash));
    let encoded = lookup
        .password_hash
        .filter(|hash| crate::pass::bounded_hash(hash))
        .unwrap_or_else(|| state.pass_dummy.as_ref().clone());
    let password = credentials.password().to_owned();
    let verified = tokio::task::spawn_blocking(move || {
        // Cancellation cannot release the shared permit before hashing ends.
        let _permit = permit;
        crate::pass::verify(&password, &encoded) && recognized
    })
    .await
    .unwrap_or(false);
    match state
        .store
        .finish_vip_login(
            &ticket.storage_hash(),
            &peer_hash,
            verified,
            &receipt.storage_hash(),
        )
        .await
    {
        Ok(status) => match status.as_str() {
            "ready" => Json(protocol::VipWriteResult::Ready {
                receipt: receipt.encode(),
                peer_hash,
                context_hash,
            })
            .into_response(),
            "rejected" | "ineligible" => Json(protocol::VipWriteResult::None {}).into_response(),
            "capacity" => Json(protocol::VipWriteResult::Capacity {}).into_response(),
            _ => error(StatusCode::SERVICE_UNAVAILABLE),
        },
        Err(_) => error(StatusCode::SERVICE_UNAVAILABLE),
    }
}
