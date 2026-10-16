//! Shared bounded image issuance for authenticated verification requests.
use crate::{AppState, access::error};
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use base64::Engine;
use board_domain::{
    challenge_policy::{self, ImageStyle},
    challenge_secrets::{TokenKind, VerificationToken},
};
use board_store::verification::{ChallengePurpose, NewChallenge};
use board_verification_client::protocol;

pub(crate) async fn prepare(
    state: &AppState,
    input: &protocol::ChallengeContext,
    purpose: ChallengePurpose,
    context_hash: &[u8; 32],
    style: ImageStyle,
    suspicious: bool,
    render_permit: tokio::sync::OwnedSemaphorePermit,
) -> Result<protocol::Challenge, StatusCode> {
    let Ok(token) = VerificationToken::generate(TokenKind::Challenge) else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    let Some(answer) = state.key.challenge_answer(&token, style.characters) else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    let Some(answer_hash) = state.key.answer_hash(&token, answer.as_str()) else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    let result = match state
        .store
        .issue_challenge(&NewChallenge {
            token_hash: &state.key.token_hash(&token),
            answer_hash: &answer_hash,
            peer_hash: &state.key.network_identity(input.session.peer),
            session_hash: &input.session.fingerprints.token,
            purpose,
            board: input.board.as_deref(),
            target: input.target,
            context_hash,
            suspicious,
        })
        .await
    {
        Ok(value) => value,
        Err(_) => return Err(StatusCode::SERVICE_UNAVAILABLE),
    };
    let Ok(cooldown) = u16::try_from(result.cooldown_seconds) else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    if !result.issued {
        return Ok(protocol::Challenge::Wait {
            seconds: cooldown,
            ticket: None,
        });
    }
    let Some(seed) = state.key.render_seed(&token) else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    let rendered = tokio::task::spawn_blocking(move || {
        // The permit outlives cancellation of the HTTP future, bounding queued
        // and running raster work even if the requesting client disconnects.
        let _permit = render_permit;
        super::render::render(&answer, seed, style.slider)
    })
    .await;
    let Ok(Some(image)) = rendered else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    let encode = |bytes: &[u8]| {
        format!(
            "data:image/png;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(bytes)
        )
    };
    Ok(protocol::Challenge::Ready {
        token: token.encode(),
        image: encode(&image.foreground),
        background: image.background.as_deref().map(encode),
        ttl_seconds: challenge_policy::CHALLENGE_SECONDS as u16,
        cooldown_seconds: cooldown,
        slider: style.slider,
        offset_max: image.offset_max,
    })
}

/// Preserve the email endpoint's existing response shape while sharing the
/// typed challenge result with the posting ticket state machine.
pub(crate) async fn render(
    state: &AppState,
    input: &protocol::ChallengeContext,
    purpose: ChallengePurpose,
    context_hash: &[u8; 32],
    style: ImageStyle,
    suspicious: bool,
    render_permit: tokio::sync::OwnedSemaphorePermit,
) -> Response {
    match prepare(
        state,
        input,
        purpose,
        context_hash,
        style,
        suspicious,
        render_permit,
    )
    .await
    {
        Ok(challenge) => Json(challenge).into_response(),
        Err(status) => error(status),
    }
}
