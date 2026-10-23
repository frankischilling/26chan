//! Source site forms use their own purpose-bound verification receipts.
pub(crate) mod applications;
mod forms;
use crate::{
    AppState, anonymous_session::Session, handlers::AppError, security::RequestPeer,
    verification_form::PostingSecret,
};
use askama::Template;
use axum::{
    Router,
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use board_domain::{
    challenge_secrets::{ReceiptToken, TokenKind, VerificationToken},
    site_submissions::feedback::Submission,
};
use board_store::site_submissions::{
    self as store,
    feedback::{Proof, Request as Write},
};
use board_verification_client::protocol;

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/feedback", get(page).post(page))
        .route(
            "/janitorapp",
            get(applications::page).post(applications::page),
        )
}
#[derive(Template)]
#[template(path = "site-feedback.html")]
struct Page<'a> {
    profile: Option<&'a store::FeedbackProfile>,
    csrf: String,
    token: String,
    image: String,
    sent: bool,
    error: Option<&'static str>,
}
fn bad() -> AppError {
    AppError(StatusCode::BAD_REQUEST, "Invalid feedback form.")
}
fn unavailable() -> AppError {
    AppError(
        StatusCode::SERVICE_UNAVAILABLE,
        "Feedback verification is unavailable. Try again.",
    )
}
async fn page(State(state): State<AppState>, request: Request) -> Response {
    match render(&state, request).await {
        Ok(response) => response,
        Err(error) => {
            let view = Page {
                profile: None,
                csrf: String::new(),
                token: String::new(),
                image: String::new(),
                sent: false,
                error: Some(error.1),
            };
            match output(&state, &view, HeaderMap::new()) {
                Ok(mut response) => {
                    *response.status_mut() = error.0;
                    response
                }
                Err(_) => error.into_response(),
            }
        }
    }
}
async fn render(state: &AppState, request: Request) -> Result<Response, AppError> {
    let sent = match request.uri().query() {
        None => false,
        Some("sent=1") => true,
        _ => return Err(bad()),
    };
    if sent {
        if request.method() == Method::POST {
            return Err(bad());
        }
        return output(
            state,
            &Page {
                profile: None,
                csrf: String::new(),
                token: String::new(),
                image: String::new(),
                sent: true,
                error: None,
            },
            HeaderMap::new(),
        );
    }
    let Some(profile) = store::feedback_profile(&state.pool).await? else {
        if request.method() == Method::POST {
            return Err(AppError(StatusCode::CONFLICT, "Feedback is closed."));
        }
        return crate::site::info(state.clone(), "feedback", "Feedback").await;
    };
    let peer = request
        .extensions()
        .get::<RequestPeer>()
        .and_then(|p| p.0)
        .ok_or_else(unavailable)?;
    let headers = request.headers().clone();
    let session = Session::resolve(state, &headers, Some(peer)).await?;
    let existing = PostingSecret::read(&headers, state.production).map_err(|_| bad())?;
    let service = state.verification.as_ref().ok_or_else(unavailable)?;
    let mut context = protocol::ChallengeContext {
        session: crate::signin::trusted(&session, state, Some(peer))?,
        purpose: protocol::Purpose::SiteFeedback,
        board: None,
        target: profile.id,
        form_nonce: String::new(),
        email_token: None,
    };
    if request.method() == Method::POST {
        if session.posting.minted {
            return Err(AppError(
                StatusCode::FORBIDDEN,
                "This form has expired. Reload it.",
            ));
        }
        let secret = existing.ok_or_else(bad)?;
        let fields = forms::Fields::read(
            request,
            &[
                "action",
                "category",
                "subject",
                "message",
                "csrf",
                "profile",
                "challenge",
                "answer",
            ],
        )
        .await
        .map_err(|status| AppError(status, "Invalid feedback form."))?;
        if !secret.matches(fields.get("csrf")) || fields.get("profile") != profile.id.to_string() {
            return Err(AppError(
                StatusCode::FORBIDDEN,
                "This form has expired. Reload it.",
            ));
        }
        if fields.get("action") != "submit" {
            return Err(bad());
        }
        let category: u16 = fields.get("category").parse().map_err(|_| bad())?;
        if category.to_string() != fields.get("category") {
            return Err(bad());
        }
        let submission = Submission::prepare(
            &profile.form,
            category,
            fields.get("subject"),
            fields.get("message"),
        )
        .map_err(|_| bad())?;
        let answer = fields.get("answer");
        if answer.is_empty()
            || answer.len() > 64
            || answer.chars().any(char::is_control)
            || VerificationToken::parse(TokenKind::Challenge, fields.get("challenge")).is_none()
        {
            return Err(bad());
        }
        let user_agent = bounded_header(&headers, header::USER_AGENT, 512)?;
        let language = bounded_header(&headers, header::ACCEPT_LANGUAGE, 128)?;
        context.form_nonce = fields.get("csrf").into();
        let protocol::AnswerResult::Verified {
            receipt,
            peer_hash,
            context_hash,
        } = service
            .check(&protocol::Answer {
                context,
                challenge: fields.get("challenge").into(),
                answer: answer.into(),
            })
            .await
            .map_err(|_| unavailable())?
        else {
            return Err(AppError(
                StatusCode::BAD_REQUEST,
                "Verification failed. Reload the form and try again.",
            ));
        };
        let receipt = ReceiptToken::parse(&receipt)
            .ok_or_else(unavailable)?
            .storage_hash();
        store::feedback::submit(
            &state.pool,
            &Write {
                profile: profile.id,
                fields: &submission,
                address: peer,
                user_agent,
                language,
                proof: Proof {
                    session: &session.posting.fingerprints.token,
                    receipt: &receipt,
                    peer: &peer_hash,
                    context: &context_hash,
                },
            },
        )
        .await?;
        return Ok((
            StatusCode::SEE_OTHER,
            [
                (header::LOCATION, "/feedback?sent=1"),
                (header::CACHE_CONTROL, "no-store"),
            ],
        )
            .into_response());
    }
    let secret = existing
        .map(Ok)
        .unwrap_or_else(PostingSecret::generate)
        .map_err(|_| unavailable())?;
    context.form_nonce = secret.nonce();
    let issued = service
        .posting_challenge(&protocol::PostingChallengeRequest {
            context,
            ticket: None,
            ticket_answer: None,
        })
        .await
        .map_err(|_| unavailable())?;
    let protocol::Challenge::Ready {
        token,
        image,
        slider: false,
        background: None,
        ..
    } = issued.challenge
    else {
        return Err(unavailable());
    };
    if issued.ticket_challenge
        || !crate::posting_challenge::page::valid_png(&image)
        || VerificationToken::parse(TokenKind::Challenge, &token).is_none()
    {
        return Err(unavailable());
    }
    let mut cookies = HeaderMap::new();
    secret.append(&mut cookies, state.production);
    session.append(&mut cookies, state.production);
    output(
        state,
        &Page {
            profile: Some(&profile),
            csrf: secret.nonce(),
            token,
            image,
            sent: false,
            error: None,
        },
        cookies,
    )
}
fn bounded_header(
    headers: &HeaderMap,
    name: header::HeaderName,
    limit: usize,
) -> Result<&str, AppError> {
    if headers.get_all(&name).iter().count() > 1 {
        return Err(bad());
    }
    let value = headers
        .get(name)
        .map(|v| v.to_str())
        .transpose()
        .map_err(|_| bad())?
        .unwrap_or("");
    if value.len() > limit || value.chars().any(char::is_control) {
        return Err(bad());
    }
    Ok(value)
}
fn output(state: &AppState, page: &Page<'_>, cookies: HeaderMap) -> Result<Response, AppError> {
    let mut response = crate::output::html(state, page)?;
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
