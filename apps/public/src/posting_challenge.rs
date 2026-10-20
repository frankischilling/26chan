//! Browser CAPTCHA requests carry only form input. Trusted peer/session scope
//! and private issuer credentials never come from the browser request body.
#[cfg(feature = "browser-tests")]
pub(crate) mod fixture;
pub(crate) mod page;

use crate::{
    AppState,
    anonymous_session::Session,
    handlers::AppError,
    security::RequestPeer,
    verification_form::{Fields, MAX_BODY, PostingSecret},
};
use axum::{
    Extension, Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::post,
};
use board_verification_client::protocol;
use serde::Serialize;
use std::net::IpAddr;

fn invalid() -> AppError {
    AppError(StatusCode::BAD_REQUEST, "Invalid CAPTCHA request.")
}
fn unavailable() -> AppError {
    AppError(
        StatusCode::SERVICE_UNAVAILABLE,
        "CAPTCHA is unavailable. Please try again.",
    )
}

pub(crate) struct Scope {
    pub(crate) board: String,
    pub(crate) target: i64,
    pub(crate) purpose: protocol::Purpose,
}
impl Scope {
    fn parse(fields: &Fields) -> Result<Self, AppError> {
        let board = fields.get("board");
        board_domain::BoardSlug::parse(board).map_err(|_| invalid())?;
        let value = fields.get("target");
        let target = value.parse::<i64>().map_err(|_| invalid())?;
        if target < 0 || target.to_string() != value {
            return Err(invalid());
        }
        let purpose = match (fields.get("purpose"), target) {
            ("thread", 0) => protocol::Purpose::Thread,
            ("reply", 1..) => protocol::Purpose::Reply,
            ("report", 1..) => protocol::Purpose::Report,
            _ => return Err(invalid()),
        };
        Ok(Self {
            board: board.into(),
            target,
            purpose,
        })
    }
    async fn check_target(&self, state: &AppState) -> Result<(), AppError> {
        match self.purpose {
            protocol::Purpose::Reply => {
                board_store::thread(&state.pool, &self.board, self.target).await?;
            }
            protocol::Purpose::Report => {
                board_store::report_target(&state.pool, &self.board, self.target).await?;
            }
            _ => {}
        }
        Ok(())
    }
    fn context(
        &self,
        state: &AppState,
        session: &Session,
        peer: Option<IpAddr>,
        nonce: &str,
    ) -> Result<protocol::ChallengeContext, AppError> {
        Ok(protocol::ChallengeContext {
            session: crate::signin::trusted(session, state, peer)?,
            purpose: self.purpose,
            board: Some(self.board.clone()),
            target: self.target,
            form_nonce: nonce.into(),
            email_token: None,
        })
    }
}

/// PHP isset checks presence of these two names, even with an empty value.
/// The UserPwd-equivalent anonymous session is checked separately by the Store.
pub(crate) fn source_cookies_absent(headers: &HeaderMap) -> Result<bool, AppError> {
    let mut total = 0usize;
    let mut absent = true;
    for value in headers.get_all(header::COOKIE) {
        total = total
            .checked_add(value.as_bytes().len())
            .ok_or_else(invalid)?;
        if total > MAX_BODY {
            return Err(invalid());
        }
        for pair in value.to_str().map_err(|_| invalid())?.split(';') {
            if let Some((name, _)) = pair.trim().split_once('=')
                && matches!(name, "4chan_auser" | "pass_enabled")
            {
                absent = false;
            }
        }
    }
    Ok(absent)
}
fn fields(headers: &HeaderMap, body: &[u8]) -> Result<Fields, AppError> {
    if headers.get_all(header::CONTENT_TYPE).iter().count() != 1
        || headers
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .is_none_or(|v| {
                !matches!(
                    v,
                    "application/x-www-form-urlencoded"
                        | "application/x-www-form-urlencoded;charset=UTF-8"
                        | "application/x-www-form-urlencoded; charset=UTF-8"
                )
            })
    {
        return Err(invalid());
    }
    Fields::parse(
        body,
        &[
            "action",
            "board",
            "target",
            "purpose",
            "csrf",
            "ticket",
            "ticket_challenge",
            "ticket_answer",
        ],
        MAX_BODY,
    )
    .map_err(|_| invalid())
}
fn ticket_answer(fields: &Fields) -> Result<Option<protocol::TicketAnswer>, AppError> {
    let challenge = fields.get("ticket_challenge");
    let answer = fields.get("ticket_answer");
    if challenge.is_empty() && answer.is_empty() {
        return Ok(None);
    }
    if board_domain::challenge_secrets::VerificationToken::parse(
        board_domain::challenge_secrets::TokenKind::Challenge,
        challenge,
    )
    .is_none()
        || answer.is_empty()
        || answer.len() > 64
        || answer.chars().any(char::is_control)
    {
        return Err(invalid());
    }
    Ok(Some(protocol::TicketAnswer {
        challenge: challenge.into(),
        answer: answer.into(),
    }))
}
#[derive(Default)]
pub(crate) struct FormInput {
    pub(crate) csrf: String,
    pub(crate) challenge: Option<String>,
    pub(crate) answer: Option<String>,
    pub(crate) credit: String,
}

#[derive(Debug)]
pub(crate) struct ProofError {
    pub(crate) error: AppError,
    clear_pass: Option<bool>,
}
impl From<AppError> for ProofError {
    fn from(error: AppError) -> Self {
        Self {
            error,
            clear_pass: None,
        }
    }
}
impl From<board_store::StoreError> for ProofError {
    fn from(error: board_store::StoreError) -> Self {
        AppError::from(error).into()
    }
}
impl ProofError {
    pub(crate) fn pass_rejected(production: bool, message: &'static str) -> Self {
        Self {
            error: AppError(StatusCode::BAD_REQUEST, message),
            clear_pass: Some(production),
        }
    }
    pub(crate) fn render_with(self, render: impl FnOnce(AppError) -> Response) -> Response {
        let mut response = render(self.error);
        if let Some(production) = self.clear_pass {
            crate::pass_auth::clear_cookie(response.headers_mut(), production);
        }
        response
    }
}
impl IntoResponse for ProofError {
    fn into_response(self) -> Response {
        self.render_with(IntoResponse::into_response)
    }
}

/// Exchange an answer immediately before the protected write. Only the issuer's
/// private response supplies the peer/context hashes; they never cross HTTP to
/// the browser. The database consumes the resulting receipt with the write.
pub(crate) async fn proof(
    state: &AppState,
    headers: &HeaderMap,
    session: &Session,
    peer: Option<IpAddr>,
    scope: Scope,
    input: &FormInput,
) -> Result<Option<board_store::WriteChallengeProof>, ProofError> {
    let pass = crate::pass_auth::read_cookie(headers, state.production)?;
    // Presence is retained for request observations. Verification still treats
    // absent and empty values alike and derives authority only from the issuer.
    let challenge = input.challenge.as_deref().unwrap_or_default();
    let answer = input.answer.as_deref().unwrap_or_default();
    if pass.is_none()
        && !board_store::posting_challenge::board_policy(&state.pool, &scope.board)
            .await?
            .enabled
    {
        return Ok(None);
    }
    if !matches!(
        (scope.purpose, scope.target),
        (protocol::Purpose::Thread, 0)
            | (protocol::Purpose::Reply | protocol::Purpose::Report, 1..)
    ) {
        return Err(invalid().into());
    }
    let secret = PostingSecret::read(headers, state.production)
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;
    if !secret.matches(&input.csrf) || session.posting.minted {
        return Err(AppError(
            StatusCode::FORBIDDEN,
            "This CAPTCHA form has expired. Reload it.",
        )
        .into());
    }
    if !matches!(input.credit.as_str(), "" | "1")
        || answer.len() > 64
        || answer.chars().any(char::is_control)
    {
        return Err(invalid().into());
    }
    if let Some(token) = pass {
        let context = scope.context(state, session, peer, &input.csrf)?;
        let checked = state
            .verification
            .as_ref()
            .ok_or_else(unavailable)?
            .pass_write_receipt(&protocol::PassWriteRequest { token, context })
            .await
            .map_err(|_| unavailable())?;
        return match checked {
            protocol::PassWriteResult::Verified {
                receipt,
                peer_hash,
                context_hash,
            } => board_store::WriteChallengeProof::from_issuer(&receipt, peer_hash, context_hash)
                .map(Some)
                .ok_or_else(unavailable)
                .map_err(Into::into),
            protocol::PassWriteResult::Expired {} => Err(ProofError::pass_rejected(
                state.production,
                "This Pass has expired.",
            )),
            protocol::PassWriteResult::Disabled {} => Err(ProofError::pass_rejected(
                state.production,
                "This Pass has been disabled.",
            )),
            protocol::PassWriteResult::InUse {} => Err(ProofError::pass_rejected(
                state.production,
                "This Pass is already in use from another network.",
            )),
            protocol::PassWriteResult::None {} => Err(ProofError::pass_rejected(
                state.production,
                "This Pass session is invalid. Authenticate again.",
            )),
            protocol::PassWriteResult::Capacity {} => Err(unavailable().into()),
        };
    }
    if scope.purpose == protocol::Purpose::Report
        && challenge.is_empty()
        && answer.is_empty()
        && input.credit.is_empty()
    {
        let (key, peer) = state
            .poster_id_key
            .as_ref()
            .zip(peer)
            .ok_or_else(unavailable)?;
        let identity = key.public_report_rate_identity(peer);
        if board_store::report_challenge::advice(
            &state.pool,
            &scope.board,
            scope.target,
            &identity,
            session.posting,
            source_cookies_absent(headers)?,
        )
        .await?
        {
            return Ok(None);
        }
    }
    let context = scope.context(state, session, peer, &input.csrf)?;
    let service = state.verification.as_ref().ok_or_else(unavailable)?;
    let checked = if input.credit == "1" {
        if scope.purpose == protocol::Purpose::Thread || !challenge.is_empty() || !answer.is_empty()
        {
            return Err(invalid().into());
        }
        service.credit(&context).await
    } else {
        if board_domain::challenge_secrets::VerificationToken::parse(
            board_domain::challenge_secrets::TokenKind::Challenge,
            challenge,
        )
        .is_none()
            || answer.is_empty()
        {
            return Err(AppError(
                StatusCode::BAD_REQUEST,
                "Complete the CAPTCHA before submitting.",
            )
            .into());
        }
        service
            .check(&protocol::Answer {
                context,
                challenge: challenge.to_owned(),
                answer: answer.to_owned(),
            })
            .await
    }
    .map_err(|_| unavailable())?;
    match checked {
        protocol::AnswerResult::Verified {
            receipt,
            peer_hash,
            context_hash,
        } => board_store::WriteChallengeProof::from_issuer(&receipt, peer_hash, context_hash)
            .map(|proof| Some(proof.with_credit_request(credit_request(headers))))
            .ok_or_else(unavailable)
            .map_err(Into::into),
        protocol::AnswerResult::Rejected {} => Err(AppError(
            StatusCode::BAD_REQUEST,
            "Incorrect or expired CAPTCHA. Please try again.",
        )
        .into()),
    }
}

fn credit_request(
    headers: &HeaderMap,
) -> board_domain::content_admission::classifiers::CreditRequest<'_> {
    use board_domain::content_admission::{classifiers::CreditRequest, network::ProviderValue};
    fn fact<'a>(headers: &'a HeaderMap, name: &str) -> ProviderValue<Option<&'a [u8]>> {
        let mut values = headers.get_all(name).iter();
        let first = values.next();
        if values.next().is_some() {
            return ProviderValue::Unavailable;
        }
        ProviderValue::Available(first.map(|value| value.as_bytes()))
    }
    CreditRequest {
        // Only presence is used for these fields. Duplicate values cannot
        // make their source `isset` result ambiguous.
        accept_language: ProviderValue::Available(
            headers.contains_key("accept-language").then_some(&b""[..]),
        ),
        accept: ProviderValue::Available(headers.contains_key("accept").then_some(&b""[..])),
        user_agent: fact(headers, "user-agent"),
        referer: fact(headers, "referer"),
    }
}

#[derive(Serialize)]
struct Reply {
    csrf: String,
    challenge: protocol::Challenge,
    ticket_challenge: bool,
    report_exempt: bool,
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/captcha", post(request))
        .layer(axum::extract::DefaultBodyLimit::max(MAX_BODY))
}
async fn request(
    State(state): State<AppState>,
    Extension(RequestPeer(peer)): Extension<RequestPeer>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    let fields = fields(&headers, &body)?;
    let scope = Scope::parse(&fields)?;
    let initial = match fields.get("action") {
        "init" => true,
        "challenge" => false,
        _ => return Err(invalid()),
    };
    if initial
        && ["csrf", "ticket_challenge", "ticket_answer"]
            .iter()
            .any(|name| fields.has(name))
    {
        return Err(invalid());
    }
    let secret = PostingSecret::read(&headers, state.production).map_err(|_| invalid())?;
    let secret = match (secret, initial) {
        (Some(secret), _) => secret,
        (None, true) => PostingSecret::generate().map_err(|_| unavailable())?,
        (None, false) => return Err(invalid()),
    };
    if !initial && !secret.matches(fields.get("csrf")) {
        return Err(AppError(
            StatusCode::FORBIDDEN,
            "This CAPTCHA form has expired. Reload it.",
        ));
    }
    let ticket = fields.get("ticket");
    if !ticket.is_empty()
        && board_domain::challenge_secrets::VerificationToken::parse(
            board_domain::challenge_secrets::TokenKind::Ticket,
            ticket,
        )
        .is_none()
    {
        return Err(invalid());
    }
    let ticket_answer = ticket_answer(&fields)?;
    let policy = board_store::posting_challenge::board_policy(&state.pool, &scope.board).await?;
    scope.check_target(&state).await?;
    let session = Session::resolve(&state, &headers, peer).await?;
    if !initial && session.posting.minted {
        return Err(invalid());
    }
    let nonce = secret.nonce();
    let exempt = if policy.enabled && scope.purpose == protocol::Purpose::Report {
        let (key, peer) = state
            .poster_id_key
            .as_ref()
            .zip(peer)
            .ok_or_else(unavailable)?;
        let identity = key.public_report_rate_identity(peer);
        board_store::report_challenge::advice(
            &state.pool,
            &scope.board,
            scope.target,
            &identity,
            session.posting,
            source_cookies_absent(&headers)?,
        )
        .await?
    } else {
        false
    };
    let pass_exempt = crate::pass_auth::posting_ready(&state, &headers, peer).await?;
    if pass_exempt {
        state
            .verification
            .as_ref()
            .ok_or_else(unavailable)?
            .ensure_session(&crate::signin::trusted(&session, &state, peer)?)
            .await
            .map_err(|_| unavailable())?;
    }
    let issued = if !policy.enabled || exempt || pass_exempt {
        protocol::PostingChallengeResult {
            challenge: protocol::Challenge::Disabled {},
            ticket_challenge: false,
        }
    } else {
        state
            .verification
            .as_ref()
            .ok_or_else(unavailable)?
            .posting_challenge(&protocol::PostingChallengeRequest {
                context: scope.context(&state, &session, peer, &nonce)?,
                ticket: (!ticket.is_empty()).then(|| ticket.into()),
                ticket_answer,
            })
            .await
            .map_err(|_| unavailable())?
    };
    let encoded = crate::output::json(
        state.limits.response_writer(262_144),
        &Reply {
            csrf: nonce,
            challenge: issued.challenge,
            ticket_challenge: issued.ticket_challenge,
            report_exempt: exempt,
        },
    )?;
    let mut response = (
        [(header::CONTENT_TYPE, "application/json")],
        encoded.into_body(),
    )
        .into_response();
    secret.append(response.headers_mut(), state.production);
    session.append(response.headers_mut(), state.production);
    Ok(response)
}

#[cfg(test)]
mod tests {
    #[test]
    fn only_typed_pass_rejections_clear_membership_cookies() {
        use super::*;
        for production in [false, true] {
            let response =
                ProofError::pass_rejected(production, "This Pass has expired.").into_response();
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            let cookies: Vec<_> = response
                .headers()
                .get_all(header::SET_COOKIE)
                .iter()
                .map(|value| value.to_str().unwrap())
                .collect();
            assert_eq!(cookies.len(), 2);
            assert!(cookies[0].starts_with(if production {
                "__Host-board-pass="
            } else {
                "board-pass="
            }));
            assert!(cookies[0].contains("HttpOnly"));
            assert!(cookies[1].starts_with("pass_enabled="));
            for cookie in cookies {
                assert!(cookie.contains("Max-Age=0"));
                assert_eq!(cookie.contains("; Secure"), production);
                assert!(!cookie.contains("Domain="));
            }
        }
        let ordinary =
            ProofError::from(AppError(StatusCode::BAD_REQUEST, "This Pass has expired."));
        assert!(
            !ordinary
                .into_response()
                .headers()
                .contains_key(header::SET_COOKIE)
        );
        assert!(
            !ProofError::from(unavailable())
                .into_response()
                .headers()
                .contains_key(header::SET_COOKIE)
        );
    }

    #[test]
    fn courtesy_credit_gate_uses_actual_header_presence_and_duplicate_uncertainty() {
        use board_domain::content_admission::{
            classifiers::withhold_captcha_credit, network::ProviderValue,
        };
        let mut headers = axum::http::HeaderMap::new();
        assert!(matches!(
            withhold_captcha_credit(super::credit_request(&headers)),
            ProviderValue::Available(true)
        ));
        headers.insert("accept-language", axum::http::HeaderValue::from_static(""));
        headers.insert("accept", axum::http::HeaderValue::from_static(""));
        assert!(matches!(
            withhold_captcha_credit(super::credit_request(&headers)),
            ProviderValue::Available(false)
        ));
        headers.insert("user-agent", axum::http::HeaderValue::from_static("iPhone"));
        headers.append("accept", axum::http::HeaderValue::from_static("text/html"));
        assert!(matches!(
            withhold_captcha_credit(super::credit_request(&headers)),
            ProviderValue::Available(false)
        ));
        headers.remove("accept");
        headers.append("user-agent", axum::http::HeaderValue::from_static("Mobile"));
        assert!(matches!(
            withhold_captcha_credit(super::credit_request(&headers)),
            ProviderValue::Unavailable
        ));
    }

    use super::*;
    #[test]
    fn browser_scope_cannot_select_internal_purposes_or_ambiguous_targets() {
        for (purpose, target, valid) in [
            ("thread", "0", true),
            ("reply", "1", true),
            ("report", "2", true),
            ("email_request", "0", false),
            ("ticket", "0", false),
            ("thread", "1", false),
            ("report", "0", false),
            ("reply", "01", false),
            ("reply", "-1", false),
            ("reply", "+1", false),
        ] {
            let input = format!("board=b&purpose={purpose}&target={target}");
            let fields =
                Fields::parse(input.as_bytes(), &["board", "purpose", "target"], MAX_BODY).unwrap();
            assert_eq!(Scope::parse(&fields).is_ok(), valid);
        }
    }
    #[test]
    fn source_cookie_presence_only_opts_out_and_is_bounded() {
        for (cookie, absent) in [
            ("", true),
            ("board-anon=example; 4chan_pass=example", true),
            ("4chan_auser=", false),
            ("pass_enabled=0", false),
            ("pass_enabled=; other=x", false),
            ("prefix_pass_enabled=1", true),
        ] {
            let mut headers = HeaderMap::new();
            headers.insert(header::COOKIE, cookie.parse().unwrap());
            assert_eq!(source_cookies_absent(&headers).unwrap(), absent);
        }
        let mut headers = HeaderMap::new();
        headers.append(header::COOKIE, "board-anon=x".parse().unwrap());
        headers.append(header::COOKIE, "4chan_auser=".parse().unwrap());
        assert!(!source_cookies_absent(&headers).unwrap());
        headers.append(header::COOKIE, "x".repeat(MAX_BODY).parse().unwrap());
        assert!(source_cookies_absent(&headers).is_err());
    }
    #[test]
    fn escalation_answers_are_bounded_and_use_challenge_tokens() {
        for input in [
            "ticket_answer=x",
            "ticket_challenge=t1_abc&ticket_answer=x",
            "ticket_challenge=x",
            "ticket_answer=%00",
        ] {
            let parsed = Fields::parse(
                input.as_bytes(),
                &["ticket_challenge", "ticket_answer"],
                MAX_BODY,
            );
            assert!(parsed.is_err() || ticket_answer(&parsed.unwrap()).is_err());
        }
    }
}

#[cfg(all(test, feature = "database-tests"))]
mod database_tests;
