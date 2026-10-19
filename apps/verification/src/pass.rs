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
