//! Credential verification stays behind the authenticated internal listener.
use crate::{AppState, access::error};
use argon2::{Argon2, PasswordHash, PasswordVerifier};
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use board_domain::challenge_secrets::PassSessionToken;
use board_verification_client::protocol;

fn bounded_hash(encoded: &str) -> bool {
    encoded.len() <= 256
        && PasswordHash::new(encoded).is_ok_and(|hash| {
            hash.algorithm.as_str() == "argon2id"
                && hash.version == Some(19)
                && hash.params.iter().count() == 3
                && hash.params.get_decimal("m") == Some(19456)
                && hash.params.get_decimal("t") == Some(2)
                && hash.params.get_decimal("p") == Some(1)
                && hash
                    .salt
                    .is_some_and(|value| (11..=64).contains(&value.as_str().len()))
                && hash.hash.as_ref().is_some_and(|value| value.len() == 32)
        })
}
fn verify(pin: &str, encoded: &str) -> bool {
    PasswordHash::new(encoded).is_ok_and(|hash| {
        Argon2::default()
            .verify_password(pin.as_bytes(), &hash)
            .is_ok()
    })
}
pub(crate) async fn login(
    State(state): State<AppState>,
    Json(input): Json<protocol::PassLogin>,
) -> Response {
    let Some(identifier) = state.key.pass_identifier(&input.identifier) else {
        return error(StatusCode::BAD_REQUEST);
    };
    let Some(networks) = state.key.pass_networks(input.peer) else {
        return error(StatusCode::BAD_REQUEST);
    };
    if input.pin.is_empty() || input.pin.len() > 128 {
        return error(StatusCode::BAD_REQUEST);
    }
    let Ok(permit) = state.pass_hashing.clone().try_acquire_owned() else {
        return Json(protocol::PassLoginResult::Capacity {}).into_response();
    };
    let (Ok(ticket), Ok(session)) = (PassSessionToken::generate(), PassSessionToken::generate())
    else {
        return error(StatusCode::SERVICE_UNAVAILABLE);
    };
    let lookup = match state
        .store
        .begin_pass_login(&identifier, &networks.peer, &ticket.storage_hash())
        .await
    {
        Ok(value) => value,
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE),
    };
    match lookup.status.as_str() {
        "flood" => return Json(protocol::PassLoginResult::Flood {}).into_response(),
        "capacity" => return Json(protocol::PassLoginResult::Capacity {}).into_response(),
        "verify" => {}
        _ => return error(StatusCode::SERVICE_UNAVAILABLE),
    }
    let recognized = lookup
        .pin_hash
        .as_ref()
        .is_some_and(|value| bounded_hash(value));
    let encoded = lookup
        .pin_hash
        .filter(|value| bounded_hash(value))
        .unwrap_or_else(|| state.pass_dummy.as_ref().clone());
    let verified = tokio::task::spawn_blocking(move || {
        // The permit stays with the hash work even if its HTTP caller cancels.
        let _permit = permit;
        verify(&input.pin, &encoded) && recognized
    })
    .await
    .unwrap_or(false);
    let result = match state
        .store
        .finish_pass_login(
            &ticket.storage_hash(),
            &networks,
            verified,
            &session.storage_hash(),
            input.remember,
        )
        .await
    {
        Ok(value) => value,
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE),
    };
    let output = match result.status.as_str() {
        "ready" if result.ttl_seconds > 0 => protocol::PassLoginResult::Ready {
            token: session.encode(),
            ttl_seconds: result.ttl_seconds as u32,
        },
        "rejected" => protocol::PassLoginResult::Rejected {},
        "capacity" => protocol::PassLoginResult::Capacity {},
        "in_use" if result.remaining > 0 => protocol::PassLoginResult::InUse {
            seconds: result.remaining as u32,
        },
        "disabled_1" => protocol::PassLoginResult::Disabled { status: 1 },
        "disabled_2" => protocol::PassLoginResult::Disabled { status: 2 },
        "disabled_3" => protocol::PassLoginResult::Disabled { status: 3 },
        "disabled_4" => protocol::PassLoginResult::Disabled { status: 4 },
        "disabled_5" => protocol::PassLoginResult::Disabled { status: 5 },
        _ => return error(StatusCode::SERVICE_UNAVAILABLE),
    };
    Json(output).into_response()
}
pub(crate) async fn status(
    State(state): State<AppState>,
    Json(input): Json<protocol::PassSession>,
) -> Response {
    let Some(token) = PassSessionToken::parse(&input.token) else {
        return Json(protocol::PassState::None {}).into_response();
    };
    let result = match state.store.pass_state(&token.storage_hash()).await {
        Ok(value) => value,
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE),
    };
    let output = match result.status.as_str() {
        "ready" => match result.purchase_year {
            Some(year) => protocol::PassState::Ready {
                purchase_year: year,
            },
            None => return error(StatusCode::SERVICE_UNAVAILABLE),
        },
        "none" => protocol::PassState::None {},
        "expired" => protocol::PassState::Expired {},
        "disabled" => protocol::PassState::Disabled {},
        _ => return error(StatusCode::SERVICE_UNAVAILABLE),
    };
    Json(output).into_response()
}
pub(crate) async fn signout(
    State(state): State<AppState>,
    Json(input): Json<protocol::PassSession>,
) -> Response {
    if let Some(token) = PassSessionToken::parse(&input.token)
        && state
            .store
            .pass_signout(&token.storage_hash())
            .await
            .is_err()
    {
        return error(StatusCode::SERVICE_UNAVAILABLE);
    }
    Json(protocol::PassState::None {}).into_response()
}

pub(crate) async fn posting_status(
    State(state): State<AppState>,
    Json(input): Json<protocol::PassPostingSession>,
) -> Response {
    let Some(networks) = state.key.pass_networks(input.peer) else {
        return error(StatusCode::BAD_REQUEST);
    };
    let Some(token) = PassSessionToken::parse(&input.token) else {
        return Json(protocol::PassPostingState::None {}).into_response();
    };
    let result = match state
        .store
        .pass_posting_state(&token.storage_hash(), networks.posting.as_ref())
        .await
    {
        Ok(value) => value,
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE),
    };
    let output = match result.status.as_str() {
        "ready" => match result.purchase_year {
            Some(purchase_year) => protocol::PassPostingState::Ready { purchase_year },
            None => return error(StatusCode::SERVICE_UNAVAILABLE),
        },
        "none" => protocol::PassPostingState::None {},
        "expired" => protocol::PassPostingState::Expired {},
        "disabled" => protocol::PassPostingState::Disabled {},
        "in_use" => protocol::PassPostingState::InUse {},
        _ => return error(StatusCode::SERVICE_UNAVAILABLE),
    };
    Json(output).into_response()
}

pub(crate) async fn write_receipt(
    State(state): State<AppState>,
    Json(input): Json<protocol::PassWriteRequest>,
) -> Response {
    let context = &input.context;
    if context.session.minted || context.email_token.is_some() {
        return error(StatusCode::BAD_REQUEST);
    }
    let purpose = match context.purpose {
        protocol::Purpose::Thread => "thread",
        protocol::Purpose::Reply => "reply",
        protocol::Purpose::Report => "report",
        _ => return error(StatusCode::BAD_REQUEST),
    };
    let Some(board) = context.board.as_deref() else {
        return error(StatusCode::BAD_REQUEST);
    };
    issue_receipt(
        &state,
        &input.token,
        &context.session,
        purpose,
        board,
        context.target,
        &context.form_nonce,
    )
    .await
}

pub(crate) async fn restriction_receipt(
    State(state): State<AppState>,
    Json(input): Json<protocol::PassRestrictionRequest>,
) -> Response {
    let purpose = match input.action {
        protocol::PassRestrictionAction::Status => "restriction_status",
        protocol::PassRestrictionAction::WarningView => "warning_view",
    };
    issue_receipt(
        &state,
        &input.token,
        &input.session,
        purpose,
        &input.board,
        input.target,
        &input.nonce,
    )
    .await
}

async fn issue_receipt(
    state: &AppState,
    pass_token: &str,
    session: &protocol::Session,
    purpose: &str,
    board: &str,
    target: i64,
    nonce: &str,
) -> Response {
    use board_domain::challenge_secrets::ReceiptToken;
    let Some(fp) = crate::handlers::session(session) else {
        return error(StatusCode::BAD_REQUEST);
    };
    if session.minted {
        return error(StatusCode::BAD_REQUEST);
    }
    let Some(hash) = state.key.form_context(purpose, Some(board), target, nonce) else {
        return error(StatusCode::BAD_REQUEST);
    };
    let Some(networks) = state.key.pass_networks(session.peer) else {
        return error(StatusCode::BAD_REQUEST);
    };
    let Some(token) = PassSessionToken::parse(pass_token) else {
        return Json(protocol::PassWriteResult::None {}).into_response();
    };
    let Ok(receipt) = ReceiptToken::generate() else {
        return error(StatusCode::SERVICE_UNAVAILABLE);
    };
    let result = match state
        .store
        .pass_write_receipt(&board_store::verification::pass::WriteRequest {
            pass_session: &token.storage_hash(),
            receipt: &receipt.storage_hash(),
            networks: &networks,
            anonymous: &fp.token,
            context: &hash,
            purpose,
            board,
            target,
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
            peer_hash: networks.peer,
            context_hash: hash,
        },
        "none" => protocol::PassWriteResult::None {},
        "expired" => protocol::PassWriteResult::Expired {},
        "disabled" => protocol::PassWriteResult::Disabled {},
        "in_use" => protocol::PassWriteResult::InUse {},
        "capacity" => protocol::PassWriteResult::Capacity {},
        _ => return error(StatusCode::SERVICE_UNAVAILABLE),
    };
    Json(output).into_response()
}
