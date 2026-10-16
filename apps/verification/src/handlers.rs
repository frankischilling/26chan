use crate::{AppState, access::error};
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use board_domain::{
    anonymous_session::Fingerprints,
    challenge_secrets::{TokenKind, VerificationToken},
};
use board_verification_client::protocol;

pub(crate) fn fingerprints(input: &protocol::Fingerprints) -> Option<Fingerprints> {
    if [
        &input.token,
        &input.network,
        &input.address,
        &input.environment,
    ]
    .iter()
    .any(|value| value.iter().all(|b| *b == 0))
    {
        return None;
    }
    Some(Fingerprints {
        token: input.token,
        network: input.network,
        address: input.address,
        environment: input.environment,
    })
}
pub(crate) fn session(input: &protocol::Session) -> Option<Fingerprints> {
    if input.peer.is_unspecified()
        || input.peer.is_multicast()
        || input.country.len() != 2
        || !input.country.bytes().all(|b| b.is_ascii_uppercase())
    {
        return None;
    }
    fingerprints(&input.fingerprints)
}

pub(crate) async fn ready(State(state): State<AppState>) -> Response {
    match state.store.ready().await {
        Ok(()) => Json(protocol::Readiness { ready: true }).into_response(),
        Err(_) => error(StatusCode::SERVICE_UNAVAILABLE),
    }
}

pub(crate) async fn ensure_session(
    State(state): State<AppState>,
    Json(input): Json<protocol::Session>,
) -> Response {
    let Some(fingerprints) = session(&input) else {
        return error(StatusCode::BAD_REQUEST);
    };
    match state
        .store
        .ensure_session(&fingerprints, input.minted)
        .await
    {
        Ok(changed) => Json(protocol::SessionResult { changed }).into_response(),
        Err(_) => error(StatusCode::SERVICE_UNAVAILABLE),
    }
}

pub(crate) async fn signout(
    State(state): State<AppState>,
    Json(input): Json<protocol::Session>,
) -> Response {
    let Some(fingerprints) = session(&input) else {
        return error(StatusCode::BAD_REQUEST);
    };
    match state.store.signout(&fingerprints.token).await {
        Ok(changed) => Json(protocol::SessionResult { changed }).into_response(),
        Err(_) => error(StatusCode::SERVICE_UNAVAILABLE),
    }
}

pub(crate) async fn email_state(
    State(state): State<AppState>,
    Json(input): Json<protocol::EmailToken>,
) -> Response {
    let Some(token) = VerificationToken::parse(TokenKind::Email, &input.token) else {
        return Json(protocol::EmailResult {
            state: protocol::EmailState::Invalid,
            cooldown_seconds: 0,
        })
        .into_response();
    };
    let result = match state.store.email_state(&state.key.token_hash(&token)).await {
        Ok(value) => match value.as_str() {
            "pending" => protocol::EmailState::Pending,
            "already_used" => protocol::EmailState::AlreadyUsed,
            "expired" => protocol::EmailState::Expired,
            "invalid" => protocol::EmailState::Invalid,
            _ => return error(StatusCode::SERVICE_UNAVAILABLE),
        },
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE),
    };
    Json(protocol::EmailResult {
        state: result,
        cooldown_seconds: 0,
    })
    .into_response()
}

fn email_result(state: protocol::EmailState, cooldown_seconds: u32) -> Response {
    Json(protocol::EmailResult {
        state,
        cooldown_seconds,
    })
    .into_response()
}
fn mapped_result(status: &str, cooldown: i32) -> Response {
    let state = match status {
        "queued" => protocol::EmailState::Queued,
        "already_queued" => protocol::EmailState::AlreadyQueued,
        "cooldown" => protocol::EmailState::Cooldown,
        "verified" => protocol::EmailState::Verified,
        "already_used" => protocol::EmailState::AlreadyUsed,
        "expired" => protocol::EmailState::Expired,
        "invalid" => protocol::EmailState::Invalid,
        "challenge_required" => protocol::EmailState::ChallengeRequired,
        "authorization_changed" => protocol::EmailState::AuthorizationChanged,
        _ => return error(StatusCode::SERVICE_UNAVAILABLE),
    };
    let Ok(cooldown) = u32::try_from(cooldown) else {
        return error(StatusCode::SERVICE_UNAVAILABLE);
    };
    email_result(state, cooldown)
}

async fn email_receipt(
    state: &AppState,
    context: &protocol::ChallengeContext,
    challenge: &str,
    answer: &str,
    fingerprints: &Fingerprints,
    context_hash: &[u8; 32],
) -> Result<Option<board_domain::challenge_secrets::ReceiptToken>, ()> {
    use board_domain::challenge_secrets::ReceiptToken;
    use board_store::verification::{ChallengeCheck, ChallengePurpose};
    let purpose = match context.purpose {
        protocol::Purpose::EmailRequest => ChallengePurpose::EmailRequest,
        protocol::Purpose::EmailVerify => ChallengePurpose::EmailVerify,
        _ => return Err(()),
    };
    let Some(challenge) = VerificationToken::parse(TokenKind::Challenge, challenge) else {
        return Ok(None);
    };
    let receipt = ReceiptToken::generate().map_err(|_| ())?;
    let answer_hash = state.key.answer_hash(&challenge, answer);
    let accepted = state
        .store
        .check_challenge(&ChallengeCheck {
            token_hash: &state.key.token_hash(&challenge),
            answer_hash: answer_hash.as_ref(),
            peer_hash: &state.key.network_identity(context.session.peer),
            session_hash: &fingerprints.token,
            purpose,
            board: None,
            target: 0,
            context_hash,
            receipt_hash: &receipt.storage_hash(),
        })
        .await
        .map_err(|_| ())?;
    Ok(accepted.then_some(receipt))
}

pub(crate) async fn request_email(
    State(state): State<AppState>,
    Json(input): Json<protocol::EmailRequest>,
) -> Response {
    use board_domain::{
        challenge_secrets::TokenKind,
        email_verification::{EmailError, PreparedEmail},
        verification_mail::MailPayload,
    };
    let Some(fingerprints) = session(&input.context.session) else {
        return error(StatusCode::BAD_REQUEST);
    };
    if input.context.purpose != protocol::Purpose::EmailRequest
        || input.context.email_token.is_some()
    {
        return error(StatusCode::BAD_REQUEST);
    }
    let Some(context_hash) = state.key.form_context(
        "email_request",
        input.context.board.as_deref(),
        input.context.target,
        &input.context.form_nonce,
    ) else {
        return error(StatusCode::BAD_REQUEST);
    };
    let email = match PreparedEmail::prepare(&input.email) {
        Ok(email) => email,
        Err(reason) => {
            return email_result(
                match reason {
                    EmailError::Invalid => protocol::EmailState::BadEmail,
                    EmailError::Subaddress => protocol::EmailState::Subaddressing,
                    EmailError::Provider => protocol::EmailState::DisallowedProvider,
                },
                0,
            );
        }
    };
    let peer = state.key.network_identity(input.context.session.peer);
    let identity = state.key.email_identity(&email);
    let recipient = state.key.email_recipient(&email);
    let advice = match state.store.email_advice(&peer, &identity, &recipient).await {
        Ok(value) => value,
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE),
    };
    if advice.status != "ready" {
        return mapped_result(&advice.status, advice.cooldown_seconds);
    }
    let receipt = match email_receipt(
        &state,
        &input.context,
        &input.challenge,
        &input.answer,
        &fingerprints,
        &context_hash,
    )
    .await
    {
        Ok(Some(receipt)) => receipt,
        Ok(None) => return email_result(protocol::EmailState::ChallengeRequired, 0),
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE),
    };
    let Ok(token) = VerificationToken::generate(TokenKind::Email) else {
        return error(StatusCode::SERVICE_UNAVAILABLE);
    };
    let token_hash = state.key.token_hash(&token);
    let Some(payload) = MailPayload::new(email.recipient(), &token.encode()) else {
        return error(StatusCode::SERVICE_UNAVAILABLE);
    };
    let Some(encrypted) = state.mail_key.seal(&token_hash, &payload) else {
        return error(StatusCode::SERVICE_UNAVAILABLE);
    };
    match state
        .store
        .queue_email(&board_store::verification::EmailRequest {
            token_hash: &token_hash,
            email_hash: &identity,
            recipient_hash: &recipient,
            peer_hash: &peer,
            session_hash: &fingerprints.token,
            country: &input.context.session.country,
            encrypted_payload: &encrypted,
            receipt_hash: &receipt.storage_hash(),
            context_hash: &context_hash,
        })
        .await
    {
        Ok(result) => mapped_result(&result.status, result.cooldown_seconds),
        Err(_) => error(StatusCode::SERVICE_UNAVAILABLE),
    }
}

pub(crate) async fn verify_email(
    State(state): State<AppState>,
    Json(input): Json<protocol::EmailVerification>,
) -> Response {
    let Some(previous) = session(&input.context.session) else {
        return error(StatusCode::BAD_REQUEST);
    };
    let Some(replacement) = fingerprints(&input.replacement) else {
        return error(StatusCode::BAD_REQUEST);
    };
    if input.context.purpose != protocol::Purpose::EmailVerify
        || input.context.board.is_some()
        || input.context.target != 0
        || !board_verification_client::valid_token(&input.context.form_nonce)
    {
        return error(StatusCode::BAD_REQUEST);
    }
    let Some(token) = input
        .context
        .email_token
        .as_deref()
        .and_then(|value| VerificationToken::parse(TokenKind::Email, value))
    else {
        return email_result(protocol::EmailState::Invalid, 0);
    };
    let token_hash = state.key.token_hash(&token);
    match state.store.email_state(&token_hash).await {
        Ok(status) if status == "pending" => {}
        Ok(status) => return mapped_result(&status, 0),
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE),
    }
    let receipt = match email_receipt(
        &state,
        &input.context,
        &input.challenge,
        &input.answer,
        &previous,
        &token_hash,
    )
    .await
    {
        Ok(Some(receipt)) => receipt,
        Ok(None) => return email_result(protocol::EmailState::ChallengeRequired, 0),
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE),
    };
    match state
        .store
        .accept_email(&board_store::verification::EmailAcceptance {
            token_hash: &token_hash,
            peer_hash: &state.key.network_identity(input.context.session.peer),
            country: &input.context.session.country,
            receipt_hash: &receipt.storage_hash(),
            previous: &previous,
            replacement: &replacement,
        })
        .await
    {
        Ok(status) => mapped_result(&status, 0),
        Err(_) => error(StatusCode::SERVICE_UNAVAILABLE),
    }
}

pub(crate) async fn challenge(
    State(state): State<AppState>,
    Json(input): Json<protocol::ChallengeContext>,
) -> Response {
    use board_domain::challenge_policy;
    use board_store::verification::ChallengePurpose;
    let Some(fingerprints) = session(&input.session) else {
        return error(StatusCode::BAD_REQUEST);
    };
    if input.board.is_some()
        || input.target != 0
        || !board_verification_client::valid_token(&input.form_nonce)
    {
        return error(StatusCode::BAD_REQUEST);
    }
    let (purpose, context_hash) = match input.purpose {
        protocol::Purpose::EmailRequest if input.email_token.is_none() => {
            let Some(hash) = state
                .key
                .form_context("email_request", None, 0, &input.form_nonce)
            else {
                return error(StatusCode::BAD_REQUEST);
            };
            (ChallengePurpose::EmailRequest, hash)
        }
        protocol::Purpose::EmailVerify => {
            let Some(token) = input
                .email_token
                .as_deref()
                .and_then(|value| VerificationToken::parse(TokenKind::Email, value))
            else {
                return error(StatusCode::BAD_REQUEST);
            };
            let hash = state.key.token_hash(&token);
            match state.store.email_state(&hash).await {
                Ok(status) if status == "pending" => (),
                Ok(_) => return error(StatusCode::BAD_REQUEST),
                Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE),
            }
            (ChallengePurpose::EmailVerify, hash)
        }
        _ => return error(StatusCode::BAD_REQUEST),
    };
    let Ok(render_permit) = state.rendering.clone().try_acquire_owned() else {
        return error(StatusCode::TOO_MANY_REQUESTS);
    };
    if state
        .store
        .ensure_session(&fingerprints, input.session.minted)
        .await
        .is_err()
    {
        return error(StatusCode::SERVICE_UNAVAILABLE);
    }
    let snapshot = match state.store.session(&fingerprints.token).await {
        Ok(Some(value)) => value,
        _ => return error(StatusCode::SERVICE_UNAVAILABLE),
    };
    let Ok(now) = u64::try_from(chrono::Utc::now().timestamp()) else {
        return error(StatusCode::SERVICE_UNAVAILABLE);
    };
    let resumed = snapshot.for_request(now, &fingerprints);
    let peer = state.key.network_identity(input.session.peer);
    let risk = match state
        .store
        .risk(
            &peer,
            (input.purpose == protocol::Purpose::EmailVerify).then_some(&context_hash),
        )
        .await
    {
        Ok(risk) => risk,
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE),
    };
    let mut style = challenge_policy::image_style(
        &resumed,
        now,
        challenge_policy::Purpose::SignIn,
        risk.suspicious(),
    );
    // Replace the external-provider escalation with an eight-character local challenge.
    // The trusted stored score retains the source's below-90 decision boundary.
    if risk.escalated() {
        style.characters = 8;
        style.slider = true;
    }
    crate::challenge::issue::render(
        &state,
        &input,
        purpose,
        &context_hash,
        style,
        risk.suspicious(),
        render_permit,
    )
    .await
}
