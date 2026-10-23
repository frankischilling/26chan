//! Native forms for the opt-in score event. Proof hashes never reach the browser.
use crate::{
    AppState,
    anonymous_session::Session,
    handlers::AppError,
    security::RequestPeer,
    verification_form::{Fields, PostingSecret},
};
use askama::Template;
use axum::{
    Router,
    extract::{Path, Request, State},
    http::{HeaderMap, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use board_domain::{challenge_secrets::ReceiptToken, source_events::scores};
use board_store::events::scores as store;
use board_verification_client::protocol;
use std::net::IpAddr;

pub(super) fn routes() -> Router<AppState> {
    Router::new().route("/event-like/{board}/{post}", get(page).post(page))
}
fn bad() -> AppError {
    AppError(StatusCode::BAD_REQUEST, "Invalid event form.")
}
fn unavailable() -> AppError {
    AppError(
        StatusCode::SERVICE_UNAVAILABLE,
        "Event verification is unavailable. Try again.",
    )
}
#[derive(Template)]
#[template(path = "event-like.html")]
struct Page<'a> {
    board: &'a str,
    post: i64,
    thread: i64,
    run: i64,
    likes: i32,
    csrf: String,
    token: String,
    image: String,
    member: bool,
    known: bool,
    overview: bool,
}
async fn page(
    State(state): State<AppState>,
    Path((board, post)): Path<(String, i64)>,
    request: Request,
) -> Response {
    match render(&state, &board, post, request).await {
        Ok(response) => response,
        Err(error) => error.into_response(),
    }
}
fn context(
    state: &AppState,
    session: &Session,
    peer: IpAddr,
    board: &str,
    post: i64,
    run: i64,
    csrf: &str,
) -> Result<protocol::ChallengeContext, AppError> {
    Ok(protocol::ChallengeContext {
        session: crate::signin::trusted(session, state, Some(peer))?,
        purpose: protocol::Purpose::Like,
        board: Some(board.into()),
        target: post,
        form_nonce: scores::form_nonce(run, csrf).map_err(|_| bad())?,
        email_token: None,
    })
}
async fn render(
    state: &AppState,
    board: &str,
    post: i64,
    request: Request,
) -> Result<Response, AppError> {
    board_domain::BoardSlug::parse(board).map_err(|_| bad())?;
    if post <= 0 {
        return Err(bad());
    }
    let run = store::active_run(&state.pool)
        .await?
        .ok_or(AppError(StatusCode::NOT_FOUND, "This event is closed."))?;
    let target = store::target(&state.pool, run.id, board, post)
        .await?
        .ok_or(AppError(StatusCode::NOT_FOUND, "You can't like this post."))?;
    let peer = request
        .extensions()
        .get::<RequestPeer>()
        .and_then(|p| p.0)
        .ok_or_else(unavailable)?;
    let headers = request.headers().clone();
    let overview = match request.uri().query() {
        None => false,
        Some("view=1") => true,
        _ => return Err(bad()),
    };
    let mut view = Page {
        board,
        post,
        thread: target.thread_id,
        run: run.id,
        likes: target.likes,
        csrf: String::new(),
        token: String::new(),
        image: String::new(),
        member: false,
        known: false,
        overview,
    };
    if request.method() == Method::POST {
        if overview {
            return Err(bad());
        }
        let session = Session::resolve(state, &headers, Some(peer)).await?;
        let secret = PostingSecret::read(&headers, state.production)
            .map_err(|_| bad())?
            .ok_or_else(bad)?;
        if session.posting.minted {
            return Err(AppError(
                StatusCode::FORBIDDEN,
                "This form has expired. Reload it.",
            ));
        }
        if headers.get_all(header::CONTENT_TYPE).iter().count() != 1
            || !matches!(
                headers
                    .get(header::CONTENT_TYPE)
                    .and_then(|v| v.to_str().ok()),
                Some(
                    "application/x-www-form-urlencoded"
                        | "application/x-www-form-urlencoded;charset=UTF-8"
                        | "application/x-www-form-urlencoded; charset=UTF-8"
                )
            )
        {
            return Err(bad());
        }
        let body = axum::body::to_bytes(request.into_body(), 8192)
            .await
            .map_err(|_| bad())?;
        let fields = Fields::parse(&body, &["csrf", "run", "challenge", "answer"], 8192)
            .map_err(|_| bad())?;
        if !secret.matches(fields.get("csrf")) || fields.get("run") != run.id.to_string() {
            return Err(AppError(
                StatusCode::FORBIDDEN,
                "This form has expired. Reload it.",
            ));
        }
        let input = context(
            state,
            &session,
            peer,
            board,
            post,
            run.id,
            fields.get("csrf"),
        )?;
        let actor = state
            .poster_id_key
            .as_ref()
            .ok_or_else(unavailable)?
            .public_posting_rate_identity(peer);
        if fields.get("challenge").is_empty()
            && fields.get("answer").is_empty()
            && crate::pass_auth::read_cookie(&headers, state.production)?.is_none()
            && crate::posting_challenge::source_cookies_absent(&headers)?
        {
            store::like_known(
                &state.pool,
                &store::KnownLike {
                    run: run.id,
                    board,
                    post,
                    actor: actor.as_bytes(),
                    session: &session.posting.fingerprints.token,
                    address: peer,
                },
            )
            .await?;
        } else {
            let (receipt, peer_hash, context_hash) = proof(state, &headers, input, &fields).await?;
            store::like(
                &state.pool,
                &store::Like {
                    run: run.id,
                    board,
                    post,
                    actor: actor.as_bytes(),
                    session: &session.posting.fingerprints.token,
                    receipt: &receipt,
                    peer: &peer_hash,
                    context: &context_hash,
                    address: peer,
                },
            )
            .await?;
        }
        return Ok((
            StatusCode::SEE_OTHER,
            [
                (
                    header::LOCATION,
                    format!("/event-like/{board}/{post}?view=1"),
                ),
                (header::CACHE_CONTROL, "no-store".into()),
            ],
        )
            .into_response());
    }
    let mut cookies = HeaderMap::new();
    if !overview {
        let session = Session::resolve(state, &headers, Some(peer)).await?;
        let secret = PostingSecret::read(&headers, state.production)
            .map_err(|_| bad())?
            .map(Ok)
            .unwrap_or_else(PostingSecret::generate)
            .map_err(|_| unavailable())?;
        view.csrf = secret.nonce();
        let input = context(state, &session, peer, board, post, run.id, &view.csrf)?;
        let service = state.verification.as_ref().ok_or_else(unavailable)?;
        view.member = crate::pass_auth::posting_ready(state, &headers, Some(peer)).await?;
        if !view.member
            && crate::pass_auth::read_cookie(&headers, state.production)?.is_none()
            && crate::posting_challenge::source_cookies_absent(&headers)?
        {
            let actor = state
                .poster_id_key
                .as_ref()
                .ok_or_else(unavailable)?
                .public_posting_rate_identity(peer);
            view.known = store::known_address(
                &state.pool,
                run.id,
                actor.as_bytes(),
                &session.posting.fingerprints.token,
            )
            .await?;
        }
        if view.member || view.known {
            service
                .ensure_session(&input.session)
                .await
                .map_err(|_| unavailable())?;
        } else {
            let issued = service
                .posting_challenge(&protocol::PostingChallengeRequest {
                    context: input,
                    ticket: None,
                    ticket_answer: None,
                })
                .await
                .map_err(|_| unavailable())?;
            match issued.challenge {
                protocol::Challenge::Ready {
                    token,
                    image,
                    slider: false,
                    background: None,
                    ..
                } if !issued.ticket_challenge
                    && crate::posting_challenge::page::valid_png(&image)
                    && board_domain::challenge_secrets::VerificationToken::parse(
                        board_domain::challenge_secrets::TokenKind::Challenge,
                        &token,
                    )
                    .is_some() =>
                {
                    view.token = token;
                    view.image = image;
                }
                _ => return Err(unavailable()),
            }
        }
        secret.append(&mut cookies, state.production);
        session.append(&mut cookies, state.production);
    }
    let mut response = crate::output::html(state, &view)?;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "private, no-store".parse().unwrap());
    response
        .extensions_mut()
        .insert(crate::posting_challenge::page::CaptchaPage);
    for cookie in cookies.get_all(header::SET_COOKIE) {
        response
            .headers_mut()
            .append(header::SET_COOKIE, cookie.clone());
    }
    Ok(response)
}
async fn proof(
    state: &AppState,
    headers: &HeaderMap,
    context: protocol::ChallengeContext,
    fields: &Fields,
) -> Result<([u8; 32], [u8; 32], [u8; 32]), AppError> {
    let service = state.verification.as_ref().ok_or_else(unavailable)?;
    let (receipt, peer, hash) =
        if let Some(token) = crate::pass_auth::read_cookie(headers, state.production)? {
            match service
                .pass_write_receipt(&protocol::PassWriteRequest { token, context })
                .await
                .map_err(|_| unavailable())?
            {
                protocol::PassWriteResult::Verified {
                    receipt,
                    peer_hash,
                    context_hash,
                } => (receipt, peer_hash, context_hash),
                _ => {
                    return Err(AppError(
                        StatusCode::BAD_REQUEST,
                        "Pass verification failed. Sign in again.",
                    ));
                }
            }
        } else {
            let answer = fields.get("answer");
            if answer.is_empty() || answer.len() > 64 || answer.chars().any(char::is_control) {
                return Err(bad());
            }
            match service
                .check(&protocol::Answer {
                    context,
                    challenge: fields.get("challenge").into(),
                    answer: answer.into(),
                })
                .await
                .map_err(|_| unavailable())?
            {
                protocol::AnswerResult::Verified {
                    receipt,
                    peer_hash,
                    context_hash,
                } => (receipt, peer_hash, context_hash),
                protocol::AnswerResult::Rejected {} => {
                    return Err(AppError(
                        StatusCode::BAD_REQUEST,
                        "Incorrect or expired CAPTCHA. Reload the form.",
                    ));
                }
            }
        };
    Ok((
        ReceiptToken::parse(&receipt)
            .ok_or_else(unavailable)?
            .storage_hash(),
        peer,
        hash,
    ))
}
