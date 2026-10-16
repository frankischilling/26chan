//! Posting challenge and waiting-ticket orchestration. All identity and risk
//! inputs arrive from the authenticated public service or private store.
use crate::{AppState, access::error, handlers};
use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use board_domain::{
    challenge_policy::{self, ImageStyle, TicketDecision},
    challenge_secrets::{ReceiptToken, TokenKind, VerificationToken},
};
use board_store::verification::{ChallengeCheck, ChallengePurpose, TicketCompletion};
use board_verification_client::protocol;

fn response(challenge: protocol::Challenge, ticket_challenge: bool) -> Response {
    Json(protocol::PostingChallengeResult {
        challenge,
        ticket_challenge,
    })
    .into_response()
}

async fn image(
    state: &AppState,
    context: &protocol::ChallengeContext,
    purpose: ChallengePurpose,
    hash: &[u8; 32],
    style: ImageStyle,
    suspicious: bool,
    ticket: bool,
) -> Response {
    let Ok(permit) = state.rendering.clone().try_acquire_owned() else {
        return error(StatusCode::TOO_MANY_REQUESTS);
    };
    match super::issue::prepare(state, context, purpose, hash, style, suspicious, permit).await {
        Ok(challenge) => response(challenge, ticket),
        Err(status) => error(status),
    }
}

pub(crate) async fn challenge(
    State(state): State<AppState>,
    Json(input): Json<protocol::PostingChallengeRequest>,
) -> Response {
    let context = &input.context;
    let Some(fp) = handlers::session(&context.session) else {
        return error(StatusCode::BAD_REQUEST);
    };
    if context.email_token.is_some() {
        return error(StatusCode::BAD_REQUEST);
    }
    let (purpose, policy_purpose, name) = match context.purpose {
        protocol::Purpose::Thread => (
            ChallengePurpose::Thread,
            challenge_policy::Purpose::Thread,
            "thread",
        ),
        protocol::Purpose::Reply => (
            ChallengePurpose::Reply,
            challenge_policy::Purpose::Reply,
            "reply",
        ),
        protocol::Purpose::Report => (
            ChallengePurpose::Report,
            challenge_policy::Purpose::Report,
            "report",
        ),
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
    let policy = match state.store.board_policy(board).await {
        Ok(Some(policy)) => policy,
        Ok(None) => return error(StatusCode::BAD_REQUEST),
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE),
    };
    if !policy.enabled {
        return response(protocol::Challenge::Disabled {}, false);
    }
    if state
        .store
        .ensure_session(&fp, context.session.minted)
        .await
        .is_err()
    {
        return error(StatusCode::SERVICE_UNAVAILABLE);
    }
    let snapshot = match state.store.session(&fp.token).await {
        Ok(Some(value)) => value,
        _ => return error(StatusCode::SERVICE_UNAVAILABLE),
    };
    let Ok(now) = u64::try_from(chrono::Utc::now().timestamp()) else {
        return error(StatusCode::SERVICE_UNAVAILABLE);
    };
    let resumed = snapshot.for_request(now, &fp);
    let peer = state.key.network_identity(context.session.peer);
    let risk = match state.store.risk(&peer, None).await {
        Ok(value) => value,
        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE),
    };
    // The primary source's classic provider does not use the Twister ticket
    // protocol. Its replacement remains required but uses a non-slider image.
    if policy.twister {
        let Ok(required) = u16::try_from(challenge_policy::pre_cooldown(
            &resumed,
            policy_purpose,
            now,
        )) else {
            return error(StatusCode::SERVICE_UNAVAILABLE);
        };
        if required > 0 {
            let presented = input
                .ticket
                .as_deref()
                .and_then(|value| VerificationToken::parse(TokenKind::Ticket, value));
            let presented_hash = presented.as_ref().map(|token| state.key.token_hash(token));
            let advice = match state
                .store
                .ticket_advice(presented_hash.as_ref(), &peer, board)
                .await
            {
                Ok(value) => value,
                Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE),
            };
            let Ok(current) = u64::try_from(advice.current_at) else {
                return error(StatusCode::SERVICE_UNAVAILABLE);
            };
            let issued = if advice.valid {
                u64::try_from(advice.issued_at).ok()
            } else {
                None
            };
            match challenge_policy::ticket_decision(required, issued, current) {
                TicketDecision::Wait { seconds } => {
                    return response(
                        protocol::Challenge::Wait {
                            seconds,
                            ticket: presented.map(|token| token.encode()),
                        },
                        false,
                    );
                }
                TicketDecision::Issue { .. } => {
                    let Some(ticket_hash) = state.key.form_context(
                        "ticket",
                        Some(board),
                        context.target,
                        &context.form_nonce,
                    ) else {
                        return error(StatusCode::BAD_REQUEST);
                    };
                    let receipt = if advice.escalation_required {
                        let accepted = match &input.ticket_answer {
                            Some(answer) => match VerificationToken::parse(
                                TokenKind::Challenge,
                                &answer.challenge,
                            ) {
                                Some(token) => {
                                    let Ok(receipt) = ReceiptToken::generate() else {
                                        return error(StatusCode::SERVICE_UNAVAILABLE);
                                    };
                                    let answer_hash = state.key.answer_hash(&token, &answer.answer);
                                    match state
                                        .store
                                        .check_challenge(&ChallengeCheck {
                                            token_hash: &state.key.token_hash(&token),
                                            answer_hash: answer_hash.as_ref(),
                                            peer_hash: &peer,
                                            session_hash: &fp.token,
                                            purpose: ChallengePurpose::Ticket,
                                            board: Some(board),
                                            target: context.target,
                                            context_hash: &ticket_hash,
                                            receipt_hash: &receipt.storage_hash(),
                                        })
                                        .await
                                    {
                                        Ok(true) => Some(receipt),
                                        Ok(false) => None,
                                        Err(_) => return error(StatusCode::SERVICE_UNAVAILABLE),
                                    }
                                }
                                None => None,
                            },
                            None => None,
                        };
                        let Some(receipt) = accepted else {
                            return image(
                                &state,
                                context,
                                ChallengePurpose::Ticket,
                                &ticket_hash,
                                ImageStyle {
                                    slider: true,
                                    characters: 8,
                                },
                                true,
                                true,
                            )
                            .await;
                        };
                        Some(receipt)
                    } else {
                        None
                    };
                    let Ok(candidate) = VerificationToken::generate(TokenKind::Ticket) else {
                        return error(StatusCode::SERVICE_UNAVAILABLE);
                    };
                    let candidate_hash = state.key.token_hash(&candidate);
                    let prepared = match receipt {
                        Some(receipt) => {
                            state
                                .store
                                .finish_ticket(&TicketCompletion {
                                    presented: None,
                                    candidate: &candidate_hash,
                                    peer: &peer,
                                    session: &fp.token,
                                    board,
                                    target: context.target,
                                    context: &ticket_hash,
                                    receipt: &receipt.storage_hash(),
                                })
                                .await
                        }
                        None => {
                            state
                                .store
                                .prepare_ticket(None, &candidate_hash, &peer, board)
                                .await
                        }
                    };
                    match prepared {
                        Ok(Some(ticket)) if ticket.issued => {
                            return response(
                                protocol::Challenge::Wait {
                                    seconds: required,
                                    ticket: Some(candidate.encode()),
                                },
                                false,
                            );
                        }
                        Err(board_store::StoreError::Database(ref failure))
                            if failure
                                .as_database_error()
                                .and_then(|e| e.code())
                                .as_deref()
                                == Some("P0166") =>
                        {
                            return image(
                                &state,
                                context,
                                ChallengePurpose::Ticket,
                                &ticket_hash,
                                ImageStyle {
                                    slider: true,
                                    characters: 8,
                                },
                                true,
                                true,
                            )
                            .await;
                        }
                        _ => return error(StatusCode::SERVICE_UNAVAILABLE),
                    }
                }
                TicketDecision::Ready | TicketDecision::NotRequired => (),
            }
        }
    }
    if policy.twister
        && matches!(
            context.purpose,
            protocol::Purpose::Reply | protocol::Purpose::Report
        )
    {
        match state
            .store
            .credit_receipt(&board_store::verification::CreditRequest {
                receipt: None,
                peer: &peer,
                fingerprints: &fp,
                board,
                target: context.target,
                purpose,
                context: &hash,
                request_time: now as i64,
            })
            .await
        {
            Ok(credit) if credit.status == "ready" => {
                let Ok(remaining) = u8::try_from(credit.remaining) else {
                    return error(StatusCode::SERVICE_UNAVAILABLE);
                };
                if !(1..=3).contains(&remaining) {
                    return error(StatusCode::SERVICE_UNAVAILABLE);
                }
                let Ok(ttl_seconds) = u16::try_from(credit.ttl_seconds) else {
                    return error(StatusCode::SERVICE_UNAVAILABLE);
                };
                if !(1..=120).contains(&ttl_seconds) {
                    return error(StatusCode::SERVICE_UNAVAILABLE);
                }
                return response(
                    protocol::Challenge::Credits {
                        remaining,
                        ttl_seconds,
                    },
                    false,
                );
            }
            Ok(credit) if credit.status == "ineligible" => (),
            _ => return error(StatusCode::SERVICE_UNAVAILABLE),
        }
    }
    let style = if policy.twister {
        challenge_policy::image_style(&resumed, now, policy_purpose, risk.suspicious())
    } else {
        ImageStyle {
            slider: false,
            characters: if risk.suspicious() { 8 } else { 6 },
        }
    };
    image(
        &state,
        context,
        purpose,
        &hash,
        style,
        risk.suspicious(),
        false,
    )
    .await
}
