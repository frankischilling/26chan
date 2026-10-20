//! Owned-mailbox recovery. Public responses never identify an existing contact.
use crate::{AppState, access::error};
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use board_domain::{
    challenge_secrets::{PassRecoveryToken, ReceiptToken},
    pass_mail::{Payload, Recipient},
};
use board_verification_client::protocol::{self, PassRecoveryResult as ResultState};
use ring::rand::{SecureRandom, SystemRandom};
fn unavailable() -> Response {
    error(StatusCode::SERVICE_UNAVAILABLE)
}
fn accepted() -> Response {
    Json(ResultState::Accepted {}).into_response()
}
fn request_id(hash: &[u8; 32]) -> String {
    let hex: String = hash[..16].iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..]
    )
}
pub(crate) async fn request(
    State(state): State<AppState>,
    Json(input): Json<protocol::PassRecoveryRequest>,
) -> Response {
    let Some(identifier) = state.key.pass_identifier(&input.identifier) else {
        return error(StatusCode::BAD_REQUEST);
    };
    let Some(recipient) = Recipient::parse(&input.email) else {
        return error(StatusCode::BAD_REQUEST);
    };
    let Some(recipient_hash) = state.key.pass_recipient(recipient.address()) else {
        return error(StatusCode::BAD_REQUEST);
    };
    let Some(networks) = state.key.pass_networks(input.peer) else {
        return error(StatusCode::BAD_REQUEST);
    };
    let (Ok(ticket), Ok(token)) = (ReceiptToken::generate(), PassRecoveryToken::generate()) else {
        return unavailable();
    };
    let prepared = match state
        .store
        .begin_pass_recovery(
            &identifier,
            &recipient_hash,
            &networks.peer,
            &ticket.storage_hash(),
        )
        .await
    {
        Ok(value) => value,
        Err(_) => return unavailable(),
    };
    match prepared.status.as_str() {
        "accepted" => return accepted(),
        "capacity" => return Json(ResultState::Capacity {}).into_response(),
        "prepare" => {}
        _ => return unavailable(),
    }
    let Some(Payload::Contact(contact)) = prepared
        .encrypted_contact
        .as_ref()
        .and_then(|bytes| state.mail_key.open_pass(&identifier, bytes))
    else {
        return unavailable();
    };
    if state.key.pass_identifier(contact.identifier()) != Some(identifier)
        || state.key.pass_recipient(contact.recipient()) != Some(recipient_hash)
    {
        return unavailable();
    }
    let Some(recipient) = Recipient::parse(contact.recipient()) else {
        return unavailable();
    };
    let hash = token.storage_hash();
    let payload = Payload::Recovery { recipient, token };
    let Some(encrypted) = state.mail_key.seal_pass(&hash, &payload) else {
        return unavailable();
    };
    match state
        .store
        .queue_pass_recovery(
            &ticket.storage_hash(),
            &hash,
            &request_id(&hash),
            &encrypted,
        )
        .await
    {
        Ok(_) => accepted(),
        Err(_) => unavailable(),
    }
}
pub(crate) async fn status(
    State(state): State<AppState>,
    Json(input): Json<protocol::PassSession>,
) -> Response {
    let Some(token) = PassRecoveryToken::parse(&input.token) else {
        return Json(ResultState::Rejected {}).into_response();
    };
    match state.store.pass_recovery_state(&token.storage_hash()).await {
        Ok(true) => Json(ResultState::Ready {}).into_response(),
        Ok(false) => Json(ResultState::Rejected {}).into_response(),
        Err(_) => unavailable(),
    }
}
pub(crate) async fn complete(
    State(state): State<AppState>,
    Json(input): Json<protocol::PassRecoveryComplete>,
) -> Response {
    let Some(token) = PassRecoveryToken::parse(&input.token) else {
        return Json(ResultState::Rejected {}).into_response();
    };
    if input.pin.is_empty() || input.pin.len() > 128 {
        return error(StatusCode::BAD_REQUEST);
    }
    let Ok(permit) = state.pass_hashing.clone().try_acquire_owned() else {
        return Json(ResultState::Capacity {}).into_response();
    };
    match state.store.pass_recovery_state(&token.storage_hash()).await {
        Ok(true) => {}
        Ok(false) => return Json(ResultState::Rejected {}).into_response(),
        Err(_) => return unavailable(),
    }
    let encoded = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let mut salt = [0; 16];
        SystemRandom::new().fill(&mut salt).ok()?;
        let salt = SaltString::encode_b64(&salt).ok()?;
        Argon2::default()
            .hash_password(input.pin.as_bytes(), &salt)
            .ok()
            .map(|hash| hash.to_string())
    })
    .await
    .ok()
    .flatten();
    let Some(encoded) = encoded else {
        return unavailable();
    };
    match state
        .store
        .complete_pass_recovery(&token.storage_hash(), &encoded)
        .await
    {
        Ok(true) => Json(ResultState::Complete {}).into_response(),
        Ok(false) => Json(ResultState::Rejected {}).into_response(),
        Err(_) => unavailable(),
    }
}
