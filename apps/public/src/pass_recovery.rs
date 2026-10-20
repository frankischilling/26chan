//! Native replacement for the absent legacy recovery controller.
use crate::{
    AppState,
    handlers::AppError,
    security::RequestPeer,
    verification_form::{Fields, MAX_BODY, RecoverySecret},
};
use askama::Template;
use axum::{
    Extension, Router,
    body::Bytes,
    extract::{RawQuery, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
    routing::get,
};
use board_domain::challenge_secrets::PassRecoveryToken;
use board_verification_client::{
    VerificationClient,
    protocol::{self, PassRecoveryResult},
};
#[derive(Clone, Copy)]
pub(crate) struct RecoveryPage;
#[derive(Template)]
#[template(path = "pass_recovery.html")]
struct Page<'a> {
    csrf: &'a str,
    token: &'a str,
    mode: &'a str,
    message: &'a str,
}
fn invalid() -> AppError {
    AppError(StatusCode::BAD_REQUEST, "Invalid Pass recovery form.")
}
fn unavailable() -> AppError {
    AppError(
        StatusCode::SERVICE_UNAVAILABLE,
        "Pass recovery is unavailable. Try again later.",
    )
}
fn client(state: &AppState) -> Result<&VerificationClient, AppError> {
    state.verification.as_ref().ok_or_else(unavailable)
}
fn render(
    state: &AppState,
    secret: &RecoverySecret,
    token: &str,
    mode: &str,
    message: &str,
) -> Result<Response, AppError> {
    let mut response = crate::output::html(
        state,
        &Page {
            csrf: &secret.nonce(),
            token,
            mode,
            message,
        },
    )?;
    secret.append(response.headers_mut(), state.production);
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    response
        .headers_mut()
        .insert("referrer-policy", HeaderValue::from_static("strict-origin"));
    response.extensions_mut().insert(RecoveryPage);
    Ok(response)
}
pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/pass", get(page).post(submit))
        .layer(axum::extract::DefaultBodyLimit::max(MAX_BODY))
}
async fn page(
    State(state): State<AppState>,
    RawQuery(query): RawQuery,
) -> Result<Response, AppError> {
    let fields = Fields::parse(query.as_deref().unwrap_or("").as_bytes(), &["reset"], 256)
        .map_err(|_| invalid())?;
    if !fields.has("reset") {
        return Err(AppError(StatusCode::NOT_FOUND, "Pass page not found."));
    }
    let service = client(&state)?;
    let secret = RecoverySecret::generate().map_err(|_| unavailable())?;
    let token = fields.get("reset");
    if token.is_empty() {
        return render(&state, &secret, "", "request", "");
    }
    if PassRecoveryToken::parse(token).is_none() {
        return Err(invalid());
    }
    match service
        .pass_recovery_state(&protocol::PassSession {
            token: token.into(),
        })
        .await
        .map_err(|_| unavailable())?
    {
        PassRecoveryResult::Ready {} => render(&state, &secret, token, "reset", ""),
        PassRecoveryResult::Rejected {} => render(
            &state,
            &secret,
            "",
            "message",
            "This recovery link is invalid or has expired. Request a new link.",
        ),
        _ => Err(unavailable()),
    }
}
async fn submit(
    State(state): State<AppState>,
    Extension(RequestPeer(peer)): Extension<RequestPeer>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
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
    let fields = Fields::parse(
        &body,
        &["action", "csrf", "id", "email", "token", "pin", "confirm"],
        MAX_BODY,
    )
    .map_err(|_| invalid())?;
    let secret = RecoverySecret::read(&headers, state.production)
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;
    if !secret.matches(fields.get("csrf")) {
        return Err(AppError(
            StatusCode::FORBIDDEN,
            "This recovery form has expired. Reload the page.",
        ));
    }
    let service = client(&state)?;
    match fields.get("action") {
        "request" => {
            if fields.has("token") || fields.has("pin") || fields.has("confirm") {
                return Err(invalid());
            }
            if fields.get("id").len() != 10
                || !fields.get("id").bytes().all(|b| b.is_ascii_alphanumeric())
                || board_domain::pass_mail::Recipient::parse(fields.get("email")).is_none()
            {
                return render(
                    &state,
                    &secret,
                    "",
                    "request",
                    "Enter a ten-character Token and a valid email address.",
                );
            }
            let peer = peer
                .filter(|ip| !ip.is_unspecified() && !ip.is_multicast())
                .ok_or_else(unavailable)?;
            match service
                .pass_recovery_request(&protocol::PassRecoveryRequest {
                    identifier: fields.get("id").into(),
                    email: fields.get("email").into(),
                    peer,
                })
                .await
                .map_err(|_| unavailable())?
            {
                PassRecoveryResult::Accepted {} => render(
                    &state,
                    &secret,
                    "",
                    "message",
                    "If the Token and owner email match, a recovery link will be sent. Check the gift recipient's email if this Pass was a gift.",
                ),
                _ => Err(unavailable()),
            }
        }
        "complete" => {
            let token = fields.get("token");
            if fields.has("id") || fields.has("email") || PassRecoveryToken::parse(token).is_none()
            {
                return Err(invalid());
            }
            if fields.get("pin").is_empty()
                || fields.get("pin").len() > 128
                || fields.get("pin") != fields.get("confirm")
            {
                return render(
                    &state,
                    &secret,
                    token,
                    "reset",
                    "Enter matching PINs between 1 and 128 bytes.",
                );
            }
            match service
                .pass_recovery_complete(&protocol::PassRecoveryComplete {
                    token: token.into(),
                    pin: fields.get("pin").into(),
                })
                .await
                .map_err(|_| unavailable())?
            {
                PassRecoveryResult::Complete {} => {
                    let mut response = render(
                        &state,
                        &secret,
                        "",
                        "message",
                        "Your PIN has been changed. Sign in again with your Token and new PIN.",
                    )?;
                    crate::pass_auth::clear_cookie(response.headers_mut(), state.production);
                    RecoverySecret::clear(response.headers_mut(), state.production);
                    Ok(response)
                }
                PassRecoveryResult::Rejected {} => render(
                    &state,
                    &secret,
                    "",
                    "message",
                    "This recovery link is invalid or has expired. Request a new link.",
                ),
                _ => Err(unavailable()),
            }
        }
        _ => Err(invalid()),
    }
}

#[cfg(all(test, feature = "database-tests"))]
mod database_tests;

#[cfg(feature = "browser-tests")]
pub(crate) fn fixture_response(state: &AppState, mode: &str) -> Result<Response, String> {
    let secret = RecoverySecret::generate().map_err(|_| "Fixture secret unavailable.")?;
    let token = format!("pr1_{}", "11".repeat(32));
    let (token, mode, message) = match mode {
        "recovery-request" => ("", "request", ""),
        "recovery-reset" => (token.as_str(), "reset", ""),
        "recovery-complete" => (
            "",
            "message",
            "Your PIN has been changed. Sign in again with your Token and new PIN.",
        ),
        "recovery-escaped" => ("", "message", "<script>owned recovery fixture</script>"),
        _ => return Err("Invalid recovery fixture.".into()),
    };
    let response =
        render(state, &secret, token, mode, message).map_err(|_| "Fixture rendering failed.")?;
    Ok(crate::security::verification_fixture_headers(
        response, state,
    ))
}
