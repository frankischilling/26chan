//! Browser Pass forms mediate the private verifier; no entitlement SQL runs here.
use crate::{
    AppState,
    handlers::AppError,
    security::RequestPeer,
    verification_form::{self, Fields, PassSecret},
};
use askama::Template;
use axum::{
    Extension, Router,
    body::Bytes,
    extract::{RawQuery, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use board_domain::challenge_secrets::PassSessionToken;
use board_verification_client::{VerificationClient, protocol};

#[derive(Template)]
#[template(path = "pass_auth.html")]
struct Page<'a> {
    csrf: &'a str,
    status: i8,
    message: &'a str,
    renewal: Option<&'a str>,
}
fn unavailable() -> AppError {
    AppError(
        StatusCode::SERVICE_UNAVAILABLE,
        "Pass authentication is unavailable. Try again later.",
    )
}
fn invalid() -> AppError {
    AppError(StatusCode::BAD_REQUEST, "Invalid Pass form.")
}
fn client(state: &AppState) -> Result<&VerificationClient, AppError> {
    state.verification.as_ref().ok_or_else(unavailable)
}
/// UI advice only; every submitted post obtains a separately bound receipt.
pub(crate) async fn posting_ready(
    state: &AppState,
    headers: &HeaderMap,
    peer: Option<std::net::IpAddr>,
) -> Result<bool, AppError> {
    let Some(token) = read_cookie(headers, state.production)? else {
        return Ok(false);
    };
    let peer = peer.ok_or_else(unavailable)?;
    Ok(matches!(
        client(state)?
            .pass_posting_state(&protocol::PassPostingSession { token, peer })
            .await
            .map_err(|_| unavailable())?,
        protocol::PassPostingState::Ready { .. }
    ))
}
fn cookie_name(production: bool) -> &'static str {
    if production {
        "__Host-board-pass"
    } else {
        "board-pass"
    }
}
pub(crate) fn read_cookie(
    headers: &HeaderMap,
    production: bool,
) -> Result<Option<String>, AppError> {
    let mut result = None;
    let mut seen = false;
    let mut total = 0usize;
    for header in headers.get_all(header::COOKIE) {
        total = total
            .checked_add(header.as_bytes().len())
            .ok_or_else(invalid)?;
        if total > verification_form::MAX_BODY {
            return Err(invalid());
        }
        for pair in header.to_str().map_err(|_| invalid())?.split(';') {
            let Some((name, value)) = pair.trim().split_once('=') else {
                continue;
            };
            if name != cookie_name(production) {
                continue;
            }
            if seen {
                return Err(invalid());
            }
            seen = true;
            result = PassSessionToken::parse(value).map(|token| token.encode());
        }
    }
    Ok(result)
}
pub(crate) fn clear_cookie(headers: &mut HeaderMap, production: bool) {
    let (session, enabled) = if production {
        (
            "__Host-board-pass=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax; Secure",
            "pass_enabled=; Path=/; Max-Age=0; SameSite=Lax; Secure",
        )
    } else {
        (
            "board-pass=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax",
            "pass_enabled=; Path=/; Max-Age=0; SameSite=Lax",
        )
    };
    headers.append(header::SET_COOKIE, HeaderValue::from_static(session));
    headers.append(header::SET_COOKIE, HeaderValue::from_static(enabled));
}
fn set_cookie(
    headers: &mut HeaderMap,
    production: bool,
    token: &str,
    ttl: u32,
) -> Result<(), AppError> {
    if (!token.is_empty() && PassSessionToken::parse(token).is_none())
        || ![0, 86400, 31556900].contains(&ttl)
    {
        return Err(unavailable());
    }
    let secure = if production { "; Secure" } else { "" };
    let value = format!(
        "{}={token}; Path=/; Max-Age={ttl}; HttpOnly; SameSite=Lax{secure}",
        cookie_name(production)
    );
    headers.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&value).map_err(|_| unavailable())?,
    );
    // This source-compatible presentation cookie is never read as authority.
    let enabled = if token.is_empty() { "" } else { "1" };
    headers.append(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "pass_enabled={enabled}; Path=/; Max-Age={ttl}; SameSite=Lax{secure}"
        ))
        .map_err(|_| unavailable())?,
    );
    Ok(())
}
fn render(
    state: &AppState,
    secret: &PassSecret,
    status: i8,
    message: &str,
    xhr: bool,
) -> Result<Response, AppError> {
    render_with_renewal(state, secret, status, message, xhr, None)
}
fn renewal_message(renewal: Option<&str>) -> Result<Option<String>, AppError> {
    // A private response is still validated before it becomes a URL or HTML.
    if renewal.is_some_and(|value| {
        value.is_empty()
            || value.len() > 128
            || !value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    }) {
        return Err(unavailable());
    }
    Ok(renewal.map(|reference| format!("This Pass has expired. Please visit <a href=\"/pass?renew={reference}\">this page</a> to renew it.")))
}
fn render_with_renewal(
    state: &AppState,
    secret: &PassSecret,
    status: i8,
    message: &str,
    xhr: bool,
    renewal: Option<&str>,
) -> Result<Response, AppError> {
    let xhr_message = renewal_message(renewal)?;
    let json_message = xhr_message.as_deref().unwrap_or(message);
    let mut response = if xhr {
        let encoded = crate::output::json(
            state.limits.response_writer(4096),
            &serde_json::json!({"status":status,"message":if json_message.is_empty(){None}else{Some(json_message)}}),
        )?;
        (
            [(header::CONTENT_TYPE, "application/json")],
            encoded.into_body(),
        )
            .into_response()
    } else {
        crate::output::html(
            state,
            &Page {
                csrf: &secret.nonce(),
                status,
                message,
                renewal,
            },
        )?
    };
    if status != 1 && status != 4 {
        secret.append(response.headers_mut(), state.production);
    }
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("private, no-store"),
    );
    Ok(response)
}
pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/auth", get(page).post(submit))
        .route("/auth.php", get(page).post(submit))
        .layer(axum::extract::DefaultBodyLimit::max(
            verification_form::MAX_BODY,
        ))
}
async fn page(
    State(state): State<AppState>,
    RawQuery(query): RawQuery,
    headers: HeaderMap,
) -> Result<Response, AppError> {
    let service = client(&state)?;
    let fields = Fields::parse(
        query.as_deref().unwrap_or("").as_bytes(),
        &["xhr", "action"],
        128,
    )
    .map_err(|_| invalid())?;
    if !matches!(fields.get("action"), "" | "index") || !matches!(fields.get("xhr"), "" | "1") {
        return Err(invalid());
    }
    let secret = PassSecret::generate().map_err(|_| unavailable())?;
    let token = read_cookie(&headers, state.production)?;
    let status = if let Some(token) = token.as_ref() {
        service
            .pass_state(&protocol::PassSession {
                token: token.clone(),
            })
            .await
            .map_err(|_| unavailable())?
    } else {
        protocol::PassState::None {}
    };
    let authenticated = matches!(status, protocol::PassState::Ready { .. });
    let mut response = render(
        &state,
        &secret,
        if authenticated { 2 } else { 0 },
        "",
        fields.has("xhr"),
    )?;
    if token.is_some() && !authenticated {
        set_cookie(response.headers_mut(), state.production, "", 0)?;
    }
    Ok(response)
}
async fn submit(
    State(state): State<AppState>,
    Extension(RequestPeer(peer)): Extension<RequestPeer>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Response, AppError> {
    let service = client(&state)?;
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
        &["id", "pin", "long_login", "logout", "csrf", "xhr", "action"],
        verification_form::MAX_BODY,
    )
    .map_err(|_| invalid())?;
    if !matches!(fields.get("action"), "" | "index")
        || !matches!(fields.get("xhr"), "" | "1")
        || !matches!(fields.get("long_login"), "" | "1")
        || !matches!(fields.get("logout"), "" | "1")
    {
        return Err(invalid());
    }
    let secret = PassSecret::read(&headers, state.production)
        .map_err(|_| invalid())?
        .ok_or_else(invalid)?;
    if !secret.matches(fields.get("csrf")) {
        return Err(AppError(
            StatusCode::FORBIDDEN,
            "This Pass form has expired. Reload the page.",
        ));
    }
    let xhr = fields.has("xhr");
    if fields.get("logout") == "1" {
        if fields.has("id") || fields.has("pin") || fields.has("long_login") {
            return Err(invalid());
        }
        if let Some(token) = read_cookie(&headers, state.production)? {
            service
                .pass_signout(&protocol::PassSession { token })
                .await
                .map_err(|_| unavailable())?;
        }
        let mut response = render(&state, &secret, 4, "", xhr)?;
        set_cookie(response.headers_mut(), state.production, "", 0)?;
        PassSecret::clear(response.headers_mut(), state.production);
        return Ok(response);
    }
    if fields.get("id").is_empty() || fields.get("pin").is_empty() {
        return render(
            &state,
            &secret,
            -1,
            "You have left one or more fields blank.",
            xhr,
        );
    }
    if fields.get("id").len() != 10 {
        return render(
            &state,
            &secret,
            -1,
            "Your Token must be exactly 10 characters.",
            xhr,
        );
    }
    if !fields
        .get("id")
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric())
        || fields.get("pin").len() > 128
    {
        return render(&state, &secret, -1, "Incorrect Token or PIN.", xhr);
    }
    let peer = peer
        .filter(|value| !value.is_unspecified() && !value.is_multicast())
        .ok_or_else(unavailable)?;
    let result = service
        .pass_login(&protocol::PassLogin {
            identifier: fields.get("id").into(),
            pin: fields.get("pin").into(),
            peer,
            remember: fields.get("long_login") == "1",
        })
        .await
        .map_err(|_| unavailable())?;
    let message = match result {
        protocol::PassLoginResult::Ready { token, ttl_seconds } => {
            let mut response = render(&state, &secret, 1, "", xhr)?;
            set_cookie(
                response.headers_mut(),
                state.production,
                &token,
                ttl_seconds,
            )?;
            PassSecret::clear(response.headers_mut(), state.production);
            return Ok(response);
        }
        protocol::PassLoginResult::Rejected {} => "Incorrect Token or PIN.".into(),
        protocol::PassLoginResult::Flood {} => {
            "You have to wait a while before attempting this again.".into()
        }
        protocol::PassLoginResult::InUse { seconds } => format!(
            "This Pass is already in use from another network. Wait {seconds} seconds before trying again."
        ),
        protocol::PassLoginResult::Disabled { status, renewal } => {
            let message = match status {
                1 => "This Pass has expired.",
                2 => "This Pass has been refunded and disabled. You cannot use it anymore.",
                3 => {
                    "This Pass has a disputed payment. You cannot use it until the dispute is resolved."
                }
                4 => "This Pass has been revoked due to spamming.",
                5 => "This Pass has been revoked due to illegal content.",
                _ => return Err(unavailable()),
            };
            if status != 1 && renewal.is_some() {
                return Err(unavailable());
            }
            let mut response =
                render_with_renewal(&state, &secret, -1, message, xhr, renewal.as_deref())?;
            set_cookie(response.headers_mut(), state.production, "", 0)?;
            return Ok(response);
        }
        protocol::PassLoginResult::Capacity {} => return Err(unavailable()),
    };
    render(&state, &secret, -1, &message, xhr)
}

#[cfg(all(test, feature = "database-tests"))]
mod database_tests;

/// Rendering-only browser evidence; this fixture cannot issue membership authority.
#[cfg(feature = "browser-tests")]
pub(crate) fn fixture_response(mode: &str, origin: &str) -> Result<Response, String> {
    let parsed = url::Url::parse(origin).map_err(|_| "Invalid fixture origin.")?;
    if parsed.origin().ascii_serialization() != origin {
        return Err("Invalid fixture origin.".into());
    }
    let (status, message) = match mode {
        "login" => (0, ""),
        "success" => (1, ""),
        "authenticated" => (2, ""),
        "logout" => (4, ""),
        "error" => (-1, "Incorrect Token or PIN."),
        "expired" => (-1, "This Pass has expired."),
        "escaped" => (-1, "<script>owned fixture</script>"),
        "recovery-request" | "recovery-reset" | "recovery-complete" | "recovery-escaped" => (0, ""),
        "checkout-purchase" | "checkout-renew" | "checkout-pending" | "checkout-paid"
        | "checkout-declined" | "checkout-cancelled" | "checkout-refunded"
        | "checkout-disputed" | "checkout-expired" | "checkout-escaped" => (0, ""),
        _ => return Err("Invalid Pass fixture.".into()),
    };
    let state = AppState {
        event_workflows: false,
        asn_database: None,
        browser_hint_key: None,
        posting_browser_compat_key: None,
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
    if mode.starts_with("checkout-") {
        return crate::pass_checkout::fixture_response(&state, mode);
    }
    if mode.starts_with("recovery-") {
        return crate::pass_recovery::fixture_response(&state, mode);
    }
    let secret = PassSecret::generate().map_err(|_| "Fixture secret unavailable.")?;
    let response = render_with_renewal(
        &state,
        &secret,
        status,
        message,
        false,
        (mode == "expired").then_some("owned-expired-renewal"),
    )
    .map_err(|_| "Fixture rendering failed.")?;
    Ok(crate::security::verification_fixture_headers(
        response, &state,
    ))
}

#[cfg(test)]
mod source_markup_tests {
    use super::*;
    #[test]
    fn renewal_message_refuses_markup_and_url_delimiters() {
        assert!(renewal_message(None).unwrap().is_none());
        assert!(
            renewal_message(Some("owned_Reference-123"))
                .unwrap()
                .unwrap()
                .contains("/pass?renew=owned_Reference-123")
        );
        for value in [
            "",
            "a&other=value",
            "a#fragment",
            "a/b",
            "a%22",
            "\"><script>",
            "é",
            &"a".repeat(129),
        ] {
            assert!(renewal_message(Some(value)).is_err());
        }
    }
    #[test]
    fn expired_auth_uses_same_origin_typed_link() {
        let html = Page {
            csrf: "owned",
            status: -1,
            message: "",
            renewal: Some("owned-expired-renewal"),
        }
        .render()
        .unwrap();
        assert!(html.contains("href=\"/pass?renew=owned-expired-renewal\""));
        assert!(html.contains("to renew it."));
        let escaped = Page {
            csrf: "owned",
            status: -1,
            message: "",
            renewal: Some("\"><script>owned</script>"),
        }
        .render()
        .unwrap();
        assert!(!escaped.contains("<script>"));
    }
    #[test]
    fn source_auth_chrome_and_status_navigation_are_preserved() {
        for status in [0, 1, 2, 4, -1] {
            let html = Page {
                csrf: "owned-csrf",
                status,
                message: "",
                renewal: None,
            }
            .render()
            .unwrap();
            assert!(html.contains("<title>4chan Pass - Authenticate</title>"));
            assert!(html.contains("<h1 id=\"title\">4chan Pass</h1>"));
            assert!(html.contains("/static/notifications/favicon.ico"));
            if matches!(status, 1 | 4) {
                assert!(!html.contains("href=\"/auth\""));
            }
            if status == -1 {
                assert!(html.contains("Something went wrong."));
                assert!(html.contains("[<a href=\"/auth\">Return</a>]"));
            }
            if status == 2 {
                assert!(html.contains("id=\"logout-fields\""));
            }
        }
    }
}
