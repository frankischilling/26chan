//! Email verification is mediated by the separately credentialed issuer.
use crate::verification_form as form;
use crate::{
    AppState,
    anonymous_session::{self, Session},
    handlers::AppError,
    security::RequestPeer,
};
use askama::Template;
use axum::{
    Extension, Router,
    body::Bytes,
    extract::{RawQuery, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use board_verification_client::{
    VerificationClient,
    protocol::{self, EmailState},
};
use form::{Fields, FormSecret};
use std::net::IpAddr;

pub(crate) const SCRIPT_PATH: &str = "/static/verification.v1.js";
#[derive(Clone, Copy)]
pub(crate) struct VerificationPage;
#[derive(Template)]
#[template(path = "signin.html")]
struct Page<'a> {
    csrf: &'a str,
    token: &'a str,
    mode: &'a str,
    message: &'a str,
    email: &'a str,
}
fn invalid() -> AppError {
    AppError(StatusCode::BAD_REQUEST, "Invalid verification form.")
}
fn unavailable() -> AppError {
    AppError(
        StatusCode::SERVICE_UNAVAILABLE,
        "Verification is unavailable. Reload this page before trying again.",
    )
}
fn client(state: &AppState) -> Result<&VerificationClient, AppError> {
    state.verification.as_ref().ok_or_else(unavailable)
}
pub(crate) fn trusted(
    session: &Session,
    state: &AppState,
    peer: Option<IpAddr>,
) -> Result<protocol::Session, AppError> {
    let peer = peer
        .filter(|ip| !ip.is_unspecified() && !ip.is_multicast())
        .ok_or_else(unavailable)?;
    let fps = session.posting.fingerprints;
    Ok(protocol::Session {
        fingerprints: protocol::Fingerprints {
            token: fps.token,
            network: fps.network,
            address: fps.address,
            environment: fps.environment,
        },
        minted: session.posting.minted,
        peer,
        country: String::from_utf8(anonymous_session::country(state, Some(peer))?.to_vec())
            .map_err(|_| unavailable())?,
    })
}
fn context(
    session: &Session,
    state: &AppState,
    peer: Option<IpAddr>,
    nonce: &str,
    token: &str,
) -> Result<protocol::ChallengeContext, AppError> {
    if !token.is_empty() && !form::email_token(token) {
        return Err(invalid());
    }
    Ok(protocol::ChallengeContext {
        session: trusted(session, state, peer)?,
        purpose: if token.is_empty() {
            protocol::Purpose::EmailRequest
        } else {
            protocol::Purpose::EmailVerify
        },
        board: None,
        target: 0,
        form_nonce: nonce.into(),
        email_token: (!token.is_empty()).then(|| token.into()),
    })
}
async fn verified(state: &AppState, session: &Session) -> Result<bool, AppError> {
    let snapshot =
        board_store::anonymous_session::snapshot(&state.pool, &session.posting.fingerprints.token)
            .await?;
    Ok(snapshot.is_some_and(|snapshot| {
        snapshot
            .for_request(
                session.posting.now.timestamp().max(0) as u64,
                &session.posting.fingerprints,
            )
            .verified_level
            > 0
    }))
}
fn render(
    state: &AppState,
    session: &Session,
    secret: &FormSecret,
    mode: &str,
    token: &str,
    message: &str,
    email: &str,
) -> Result<Response, AppError> {
    let mut response = render_document(state, secret, mode, token, message, email)?;
    session.append(response.headers_mut(), state.production);
    Ok(response)
}
fn render_document(
    state: &AppState,
    secret: &FormSecret,
    mode: &str,
    token: &str,
    message: &str,
    email: &str,
) -> Result<Response, AppError> {
    let nonce = secret.nonce();
    let mut response = crate::output::html(
        state,
        &Page {
            csrf: &nonce,
            token,
            mode,
            message,
            email,
        },
    )?;
    response.extensions_mut().insert(VerificationPage);
    secret.append(response.headers_mut(), state.production);
    Ok(response)
}
pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/signin", get(page).post(submit))
        .route("/signin/challenge", axum::routing::post(challenge))
        .layer(axum::extract::DefaultBodyLimit::max(form::MAX_BODY))
}
async fn page(
    State(state): State<AppState>,
    Extension(RequestPeer(peer)): Extension<RequestPeer>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let service = client(&state)?;
    let fields = Fields::parse(
        query.as_deref().unwrap_or("").as_bytes(),
        &["action", "tkn"],
        256,
    )
    .map_err(|_| invalid())?;
    let action = fields.get("action");
    let token = fields.get("tkn");
    if !matches!(action, "" | "verify")
        || (action == "verify") != form::email_token(token)
        || (action.is_empty() && fields.has("tkn"))
    {
        return Err(invalid());
    }
    let secret = FormSecret::generate().map_err(|_| unavailable())?;
    if action == "verify" {
        let result = service
            .email_state(&protocol::EmailToken {
                token: token.into(),
            })
            .await
            .map_err(|_| unavailable())?;
        let mode = if matches!(result.state, EmailState::Pending) {
            "preview"
        } else {
            "message"
        };
        return render_document(&state, &secret, mode, token, &result_message(&result), "");
    }
    let session = Session::resolve(&state, &headers, peer).await?;
    service
        .ensure_session(&trusted(&session, &state, peer)?)
        .await
        .map_err(|_| unavailable())?;
    let mode = if verified(&state, &session).await? {
        "logout"
    } else {
        "request"
    };
    render(&state, &session, &secret, mode, "", "", "")
}
fn read_form(
    headers: &HeaderMap,
    body: &[u8],
    allowed: &[&str],
    production: bool,
) -> Result<(Fields, FormSecret), AppError> {
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
    let fields = Fields::parse(body, allowed, form::MAX_BODY).map_err(|_| invalid())?;
    let secret = FormSecret::read(headers, production)
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;
    if !secret.matches(fields.get("csrf")) {
        return Err(AppError(
            StatusCode::FORBIDDEN,
            "This verification form has expired. Reload the page.",
        ));
    }
    Ok((fields, secret))
}
async fn challenge(
    State(state): State<AppState>,
    Extension(RequestPeer(peer)): Extension<RequestPeer>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    let service = client(&state)?;
    let (fields, _) = read_form(&headers, &body, &["csrf", "tkn"], state.production)?;
    let session = Session::resolve(&state, &headers, peer).await?;
    // Never issue a replacement cookie as a side effect of a stale form request.
    if session.posting.minted {
        return Err(invalid());
    }
    let input = context(
        &session,
        &state,
        peer,
        fields.get("csrf"),
        fields.get("tkn"),
    )?;
    let challenge = service.challenge(&input).await.map_err(|_| unavailable())?;
    let encoded = crate::output::json(state.limits.response_writer(262_144), &challenge)?;
    Ok((
        [(header::CONTENT_TYPE, "application/json")],
        encoded.into_body(),
    )
        .into_response())
}
async fn submit(
    State(state): State<AppState>,
    Extension(RequestPeer(peer)): Extension<RequestPeer>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    let service = client(&state)?;
    let (fields, secret) = read_form(
        &headers,
        &body,
        &[
            "action",
            "csrf",
            "email",
            "tkn",
            "t-challenge",
            "t-response",
        ],
        state.production,
    )?;
    let action = fields.get("action");
    let token = fields.get("tkn");
    let session = Session::resolve(&state, &headers, peer).await?;
    if action == "preview" {
        if !form::email_token(token)
            || ["email", "t-challenge", "t-response"]
                .iter()
                .any(|k| fields.has(k))
        {
            return Err(invalid());
        }
        service
            .ensure_session(&trusted(&session, &state, peer)?)
            .await
            .map_err(|_| unavailable())?;
        let result = service
            .email_state(&protocol::EmailToken {
                token: token.into(),
            })
            .await
            .map_err(|_| unavailable())?;
        let mode = if matches!(result.state, EmailState::Pending) {
            "verify"
        } else {
            "message"
        };
        return render(
            &state,
            &session,
            &secret,
            mode,
            token,
            &result_message(&result),
            "",
        );
    }
    if session.posting.minted {
        return Err(invalid());
    }
    if action == "signout" {
        if ["email", "tkn", "t-challenge", "t-response"]
            .iter()
            .any(|k| fields.has(k))
        {
            return Err(invalid());
        }
        let result = service
            .signout(&trusted(&session, &state, peer)?)
            .await
            .map_err(|_| unavailable())?;
        let mut response = render(
            &state,
            &session,
            &secret,
            "message",
            "",
            "Session cleared.",
            "",
        )?;
        if result.changed {
            anonymous_session::Session::clear(response.headers_mut(), state.production);
        }
        FormSecret::clear(response.headers_mut(), state.production);
        response.headers_mut().append(
            header::SET_COOKIE,
            "_ev1=; Path=/; Max-Age=0; SameSite=Strict".parse().unwrap(),
        );
        return Ok(response);
    }
    if !matches!(action, "request" | "verify")
        || (action == "verify") != form::email_token(token)
        || (action == "request" && fields.has("tkn"))
        || (action == "verify" && fields.has("email"))
    {
        return Err(invalid());
    }
    let input = context(&session, &state, peer, fields.get("csrf"), token)?;
    if action == "request" {
        let result = service
            .request_email(&protocol::EmailRequest {
                context: input,
                email: fields.get("email").into(),
                challenge: fields.get("t-challenge").into(),
                answer: fields.get("t-response").into(),
            })
            .await
            .map_err(|_| unavailable())?;
        let mode = if matches!(result.state, EmailState::Queued | EmailState::AlreadyQueued) {
            "message"
        } else {
            "request"
        };
        return render(
            &state,
            &session,
            &secret,
            mode,
            "",
            &result_message(&result),
            fields.get("email"),
        );
    }
    let replacement = Session::fresh(&state, peer)?;
    // Reserve the response body and replacement cookies before consuming the
    // one-use link, so output-budget failure cannot strand a committed session.
    let mut success_response = render(
        &state,
        &replacement,
        &secret,
        "message",
        "",
        "This session is now verified.",
        "",
    )?;
    FormSecret::clear(success_response.headers_mut(), state.production);
    let hint = if state.production {
        "_ev1=1; Path=/; Max-Age=60; SameSite=Strict; Secure"
    } else {
        "_ev1=1; Path=/; Max-Age=60; SameSite=Strict"
    };
    success_response
        .headers_mut()
        .append(header::SET_COOKIE, hint.parse().unwrap());
    let result = service
        .verify_email(&protocol::EmailVerification {
            context: input,
            replacement: trusted(&replacement, &state, peer)?.fingerprints,
            challenge: fields.get("t-challenge").into(),
            answer: fields.get("t-response").into(),
        })
        .await;
    // Reconcile only the exact server-generated replacement after a lost reply.
    // Never retry the token mutation or select a browser-supplied capability.
    let confirmed = match &result {
        Ok(result) => matches!(result.state, EmailState::Verified),
        Err(_) => verified(&state, &replacement).await?,
    };
    if confirmed {
        return Ok(success_response);
    }
    drop(success_response);
    let result = result.map_err(|_| unavailable())?;
    let mode = if matches!(result.state, EmailState::ChallengeRequired) {
        "verify"
    } else {
        "message"
    };
    render(
        &state,
        &session,
        &secret,
        mode,
        token,
        &result_message(&result),
        "",
    )
}
fn result_message(result: &protocol::EmailResult) -> String {
    if matches!(result.state, EmailState::Cooldown) {
        let (count, unit) = if result.cooldown_seconds > 3600 {
            (result.cooldown_seconds.div_ceil(3600), "hour")
        } else {
            (result.cooldown_seconds.div_ceil(60).max(1), "minute")
        };
        format!(
            "You have to wait {count} more {unit}{} before attempting this again.",
            if count == 1 { "" } else { "s" }
        )
    } else {
        state_message(&result.state).into()
    }
}
fn state_message(state: &EmailState) -> &'static str {
    match state {
        EmailState::BadEmail => "Invalid email address.",
        EmailState::DisallowedProvider => "This email provider is not allowed.",
        EmailState::Subaddressing => "Email subaddressing is not allowed",
        EmailState::Pending => "",
        EmailState::Queued => "An email containing the verification link will be sent out shortly.",
        EmailState::AlreadyQueued => "You have to wait a while before attempting this again.",
        EmailState::Verified => "This session is now verified.",
        EmailState::AlreadyUsed => {
            "This verification link has already been used. Request a new link to verify another session."
        }
        EmailState::Expired => "This verification link has expired. Request a new link.",
        EmailState::Invalid => "Invalid verification link.",
        EmailState::ChallengeRequired => "You seem to have mistyped the CAPTCHA.",
        EmailState::AuthorizationChanged => {
            "This verification link cannot be used from the current session or location. Request a new link."
        }
        EmailState::Cooldown => "Please wait before requesting another verification email.",
    }
}

/// Synthetic UI qualification uses the production template and response policy.
/// This entry point is absent unless the explicit browser-test feature is built.
#[cfg(feature = "browser-tests")]
pub(crate) fn fixture_response(mode: &str, origin: &str) -> Result<Response, String> {
    let parsed = url::Url::parse(origin).map_err(|_| "Invalid fixture origin.")?;
    if parsed.origin().ascii_serialization() != origin
        || !matches!(
            mode,
            "request" | "verify" | "preview" | "logout" | "message"
        )
    {
        return Err("Invalid verification fixture.".into());
    }
    let state = AppState {
        pool: sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://unused:synthetic@127.0.0.1:9/unavailable")
            .map_err(|_| "Fixture pool unavailable.")?,
        origin: origin.into(),
        production: false,
        limits: std::sync::Arc::new(crate::security::Limits::new(
            board_config::PublicRequestLimits::default(),
        )),
        media: None,
        verification: None,
        proxy_uid: None,
        poster_id_key: None,
        tripcode_key: None,
        country_database: None,
    };
    let secret = FormSecret::generate().map_err(|_| "Fixture secret unavailable.")?;
    let token = format!("e1_{}", "ab".repeat(32));
    let response = render_document(
        &state,
        &secret,
        mode,
        &token,
        if mode == "message" {
            "This session is now verified."
        } else {
            ""
        },
        "",
    )
    .map_err(|_| "Fixture rendering failed.")?;
    Ok(crate::security::verification_fixture_headers(
        response, &state,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cooldown_message_uses_source_units_and_actual_remaining_delay() {
        for (seconds, text) in [
            (1, "1 more minute"),
            (300, "5 more minutes"),
            (3600, "60 more minutes"),
            (3601, "2 more hours"),
        ] {
            let result = protocol::EmailResult {
                state: EmailState::Cooldown,
                cooldown_seconds: seconds,
            };
            assert!(result_message(&result).contains(text));
        }
    }
    #[test]
    fn form_requires_exact_content_type_and_matching_private_cookie() {
        let secret = FormSecret::generate().unwrap();
        let mut response = HeaderMap::new();
        secret.append(&mut response, true);
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            "application/x-www-form-urlencoded".parse().unwrap(),
        );
        headers.insert(
            header::COOKIE,
            response[header::SET_COOKIE]
                .to_str()
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .parse()
                .unwrap(),
        );
        let body = format!("action=request&csrf={}", secret.nonce());
        assert!(read_form(&headers, body.as_bytes(), &["action", "csrf"], true).is_ok());
        assert!(read_form(&headers, body.as_bytes(), &["action", "csrf"], false).is_err());
        let other = format!(
            "action=request&csrf={}",
            FormSecret::generate().unwrap().nonce()
        );
        assert_eq!(
            read_form(&headers, other.as_bytes(), &["action", "csrf"], true)
                .err()
                .unwrap()
                .0,
            StatusCode::FORBIDDEN
        );
        headers.append(header::CONTENT_TYPE, "text/plain".parse().unwrap());
        assert!(read_form(&headers, body.as_bytes(), &["action", "csrf"], true).is_err());
    }
    #[test]
    fn template_escapes_email_and_has_no_inline_execution_or_token_urls() {
        let html = Page {
            csrf: "ab",
            token: "e1_fixture",
            mode: "request",
            message: "",
            email: "\"><script>alert(1)</script>",
        }
        .render()
        .unwrap();
        assert!(!html.contains("<script>alert"));
        assert!(html.contains("&#60;script&#62;"));
        assert!(html.contains("action=\"/signin\" method=\"post\""));
        assert!(!html.contains("onclick="));
        let html = Page {
            csrf: "ab",
            token: "e1_fixture",
            mode: "verify",
            message: "",
            email: "",
        }
        .render()
        .unwrap();
        assert!(html.contains("name=\"tkn\" value=\"e1_fixture\""));
        assert!(!html.contains("?tkn="));
    }
}

#[cfg(all(test, feature = "database-tests"))]
mod database_tests;
