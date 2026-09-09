use crate::{AppError, AppState, auth, store, views};
use askama::Template;
use axum::{
    Form, Json,
    extract::{Request, State},
    http::{HeaderMap, HeaderValue},
    middleware::Next,
    response::{Html, IntoResponse, Redirect, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::sync::Arc;
use webauthn_rs::prelude::*;
type Shared = State<Arc<AppState>>;

pub async fn request_limits(State(state): Shared, request: Request, next: Next) -> Response {
    let Ok(permit) = state.limits.permits.clone().try_acquire_owned() else {
        return AppError::Capacity.into_response();
    };
    let response = request_limits_inner(state, request, next).await;
    board_http::hold_permit(response, permit)
}

async fn request_limits_inner(state: Arc<AppState>, request: Request, next: Next) -> Response {
    if request.method() == axum::http::Method::POST
        && matches!(request.uri().path(), "/login/start" | "/enroll/start")
    {
        let Ok(mut rate) = state.limits.attempts.lock() else {
            return AppError::Internal.into_response();
        };
        if rate.0.elapsed() >= std::time::Duration::from_secs(60) {
            *rate = (std::time::Instant::now(), 0);
        }
        if rate.1 >= 30 {
            return AppError::Capacity.into_response();
        }
        rate.1 += 1;
    }
    match tokio::time::timeout(std::time::Duration::from_secs(10), next.run(request)).await {
        Ok(response) => response,
        Err(_) => AppError::Internal.into_response(),
    }
}

pub async fn security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    for (key, value) in [
        ("cache-control", "private, no-store"),
        (
            "content-security-policy",
            "default-src 'none'; script-src 'self'; style-src 'self'; form-action 'self'; frame-ancestors 'none'; base-uri 'none'; connect-src 'self'",
        ),
        ("x-content-type-options", "nosniff"),
        ("referrer-policy", "same-origin"),
        (
            "permissions-policy",
            "publickey-credentials-get=(self), publickey-credentials-create=(self)",
        ),
    ] {
        response
            .headers_mut()
            .insert(key, HeaderValue::from_static(value));
    }
    response
}
pub async fn landing() -> Result<Html<String>, AppError> {
    Ok(Html(views::Login.render().map_err(|_| AppError::Internal)?))
}
pub async fn javascript() -> impl IntoResponse {
    (
        [("content-type", "text/javascript; charset=utf-8")],
        include_str!("../static/staff.js"),
    )
}
pub async fn ready(State(state): Shared) -> Result<&'static str, AppError> {
    sqlx::query("SELECT token_hash,last_activity_at FROM staff_identity.sessions LIMIT 0")
        .execute(&state.auth)
        .await?;
    sqlx::query("SELECT id FROM content.moderation_audit LIMIT 0")
        .execute(&state.staff)
        .await?;
    Ok("ready")
}
fn set_cookie(
    response: &mut Response,
    state: &AppState,
    name: &str,
    value: &str,
    age: u32,
) -> Result<(), AppError> {
    response.headers_mut().append(
        "set-cookie",
        HeaderValue::from_str(&state.config.cookie(name, value, age))
            .map_err(|_| AppError::Internal)?,
    );
    Ok(())
}
fn ceremony_cookie(state: &AppState) -> String {
    format!("{}-ceremony", state.config.cookie_name())
}
fn csrf_cookie(state: &AppState) -> String {
    format!("{}-csrf", state.config.cookie_name())
}
#[derive(sqlx::FromRow)]
struct Account {
    id: i64,
    username: String,
    user_handle: String,
}
#[derive(Deserialize)]
pub struct Start {
    #[serde(default)]
    username: String,
    #[serde(default)]
    invitation: String,
}
async fn save_ceremony(
    state: &AppState,
    account: i64,
    kind: &str,
    ceremony: Value,
    invite: Option<Vec<u8>>,
    challenge: Value,
) -> Result<Response, AppError> {
    let mut tx = state.auth.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(account)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM staff_identity.ceremonies WHERE expires_at<=clock_timestamp()")
        .execute(&mut *tx)
        .await?;
    let value = auth::token();
    // Serialize the capacity check and insertion for this account.
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM staff_identity.ceremonies WHERE account_id=$1")
            .bind(account)
            .fetch_one(&mut *tx)
            .await?;
    if count >= 5 {
        return Err(AppError::Invalid);
    }
    sqlx::query("INSERT INTO staff_identity.ceremonies(token_hash,account_id,kind,state,invitation_hash) VALUES ($1,$2,$3,$4::text::jsonb,$5)").bind(auth::hash(&value)).bind(account).bind(kind).bind(ceremony.to_string()).bind(invite).execute(&mut *tx).await?;
    tx.commit().await?;
    let mut response = Json(challenge).into_response();
    set_cookie(&mut response, state, &ceremony_cookie(state), &value, 180)?;
    Ok(response)
}
pub async fn enroll_start(
    State(state): Shared,
    headers: HeaderMap,
    Json(input): Json<Start>,
) -> Result<Response, AppError> {
    auth::origin(&headers, &state.config.origin)?;
    if input.invitation.len() != 43 {
        return Err(AppError::Unauthorized);
    }
    let invite = auth::hash(&input.invitation);
    let account:Account=sqlx::query_as("SELECT a.id,a.username,a.user_handle FROM staff_identity.accounts a JOIN staff_identity.invitations i ON i.account_id=a.id WHERE i.token_hash=$1 AND i.expires_at>clock_timestamp() AND a.revoked_at IS NULL AND a.role IN ('moderator','admin')").bind(&invite).fetch_optional(&state.auth).await?.ok_or(AppError::Unauthorized)?;
    let existing: Vec<String> = sqlx::query_scalar(
        "SELECT credential::text FROM staff_identity.credentials WHERE account_id=$1",
    )
    .bind(account.id)
    .fetch_all(&state.auth)
    .await?;
    let exclude = existing
        .iter()
        .map(|v| serde_json::from_str::<Passkey>(v).map(|key| key.cred_id().clone()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| AppError::Internal)?;
    let (challenge, ceremony) = state
        .webauthn
        .start_passkey_registration(
            Uuid::parse_str(&account.user_handle).map_err(|_| AppError::Internal)?,
            &account.username,
            &account.username,
            Some(exclude),
        )
        .map_err(|_| AppError::Invalid)?;
    save_ceremony(
        &state,
        account.id,
        "enroll",
        serde_json::to_value(ceremony).map_err(|_| AppError::Internal)?,
        Some(invite),
        serde_json::to_value(challenge).map_err(|_| AppError::Internal)?,
    )
    .await
}
#[derive(sqlx::FromRow)]
struct Ceremony {
    account_id: i64,
    state: String,
    invitation_hash: Option<Vec<u8>>,
}
async fn consume(state: &AppState, headers: &HeaderMap, kind: &str) -> Result<Ceremony, AppError> {
    let value = auth::cookie(headers, &ceremony_cookie(state))?;
    // Autocommit deletion precedes cryptographic verification: failed attempts
    // consume their challenge too, and cannot be replayed or retried.
    sqlx::query_as("DELETE FROM staff_identity.ceremonies WHERE token_hash=$1 AND kind=$2 AND expires_at>clock_timestamp() RETURNING account_id,state::text,invitation_hash").bind(auth::hash(&value)).bind(kind).fetch_optional(&state.auth).await?.ok_or(AppError::Unauthorized)
}
pub async fn enroll_finish(
    State(state): Shared,
    headers: HeaderMap,
    Json(input): Json<RegisterPublicKeyCredential>,
) -> Result<Response, AppError> {
    auth::origin(&headers, &state.config.origin)?;
    let ceremony = consume(&state, &headers, "enroll").await?;
    let saved: PasskeyRegistration =
        serde_json::from_str(&ceremony.state).map_err(|_| AppError::Internal)?;
    let key = state
        .webauthn
        .finish_passkey_registration(&input, &saved)
        .map_err(|_| AppError::Unauthorized)?;
    let invite = ceremony.invitation_hash.ok_or(AppError::Unauthorized)?;
    let key_id: &[u8] = key.cred_id().as_ref();
    let account: i64 = sqlx::query_scalar("SELECT staff_identity.enroll($1,$2,$3::text::jsonb)")
        .bind(invite)
        .bind(key_id)
        .bind(serde_json::to_string(&key).map_err(|_| AppError::Internal)?)
        .fetch_one(&state.auth)
        .await?;
    if account != ceremony.account_id {
        return Err(AppError::Internal);
    }
    let mut response = Json(json!({"ok":true})).into_response();
    set_cookie(&mut response, &state, &ceremony_cookie(&state), "", 0)?;
    Ok(response)
}
pub async fn login_start(
    State(state): Shared,
    headers: HeaderMap,
    Json(input): Json<Start>,
) -> Result<Response, AppError> {
    auth::origin(&headers, &state.config.origin)?;
    if input.username.is_empty() || input.username.len() > 64 {
        return Err(AppError::Unauthorized);
    }
    let account:Account=sqlx::query_as("SELECT id,username,user_handle FROM staff_identity.accounts WHERE username=$1 AND revoked_at IS NULL AND role IN ('moderator','admin')").bind(input.username).fetch_optional(&state.auth).await?.ok_or(AppError::Unauthorized)?;
    let credentials: Vec<String> = sqlx::query_scalar(
        "SELECT credential::text FROM staff_identity.credentials WHERE account_id=$1",
    )
    .bind(account.id)
    .fetch_all(&state.auth)
    .await?;
    let keys = credentials
        .iter()
        .map(|v| serde_json::from_str::<Passkey>(v))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| AppError::Internal)?;
    let (challenge, ceremony) = state
        .webauthn
        .start_passkey_authentication(&keys)
        .map_err(|_| AppError::Unauthorized)?;
    save_ceremony(
        &state,
        account.id,
        "login",
        serde_json::to_value(ceremony).map_err(|_| AppError::Internal)?,
        None,
        serde_json::to_value(challenge).map_err(|_| AppError::Internal)?,
    )
    .await
}
pub async fn login_finish(
    State(state): Shared,
    headers: HeaderMap,
    Json(input): Json<PublicKeyCredential>,
) -> Result<Response, AppError> {
    auth::origin(&headers, &state.config.origin)?;
    let ceremony = consume(&state, &headers, "login").await?;
    let saved: PasskeyAuthentication =
        serde_json::from_str(&ceremony.state).map_err(|_| AppError::Internal)?;
    let result = state
        .webauthn
        .finish_passkey_authentication(&input, &saved)
        .map_err(|_| AppError::Unauthorized)?;
    if !result.user_verified() {
        return Err(AppError::Unauthorized);
    }
    let key_id: &[u8] = result.cred_id().as_ref();
    let mut tx = state.auth.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(ceremony.account_id)
        .execute(&mut *tx)
        .await?;
    let previous:String=sqlx::query_scalar("SELECT c.credential::text FROM staff_identity.credentials c JOIN staff_identity.accounts a ON a.id=c.account_id WHERE c.id=$1 AND a.id=$2 AND a.revoked_at IS NULL AND a.role IN ('moderator','admin')").bind(key_id).bind(ceremony.account_id).fetch_optional(&mut *tx).await?.ok_or(AppError::Unauthorized)?;
    let json: Value = serde_json::from_str(&previous).map_err(|_| AppError::Internal)?;
    let old_counter = json["cred"]["counter"].as_u64().ok_or(AppError::Internal)?;
    if (old_counter > 0 || result.counter() > 0) && u64::from(result.counter()) <= old_counter {
        return Err(AppError::Unauthorized);
    }
    let mut key: Passkey = serde_json::from_str(&previous).map_err(|_| AppError::Internal)?;
    key.update_credential(&result)
        .ok_or(AppError::Unauthorized)?;
    let updated = serde_json::to_string(&key).map_err(|_| AppError::Internal)?;
    let changed: bool = sqlx::query_scalar(
        "SELECT staff_identity.update_counter($1,$2::text::jsonb,$3::text::jsonb)",
    )
    .bind(key_id)
    .bind(previous)
    .bind(updated)
    .fetch_one(&mut *tx)
    .await?;
    if !changed {
        return Err(AppError::Unauthorized);
    }
    if let Ok(old) = auth::cookie(&headers, state.config.cookie_name()) {
        sqlx::query("DELETE FROM staff_identity.sessions WHERE token_hash=$1")
            .bind(auth::hash(&old))
            .execute(&mut *tx)
            .await?;
    }
    sqlx::query("DELETE FROM staff_identity.sessions WHERE expires_at<=clock_timestamp() OR last_activity_at<=clock_timestamp()-($1::bigint*interval '1 second')")
        .bind(state.config.idle_timeout.as_secs() as i64)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM staff_identity.sessions WHERE token_hash IN (SELECT token_hash FROM staff_identity.sessions WHERE account_id=$1 ORDER BY authenticated_at DESC,token_hash OFFSET 9)").bind(ceremony.account_id).execute(&mut *tx).await?;
    let session = auth::token();
    let csrf = auth::token();
    sqlx::query("INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id) VALUES ($1,$2,$3,$4)").bind(auth::hash(&session)).bind(auth::hash(&csrf)).bind(ceremony.account_id).bind(key_id).execute(&mut *tx).await?;
    tx.commit().await?;
    let mut response = Json(json!({"ok":true})).into_response();
    set_cookie(
        &mut response,
        &state,
        state.config.cookie_name(),
        &session,
        28800,
    )?;
    set_cookie(&mut response, &state, &csrf_cookie(&state), &csrf, 28800)?;
    set_cookie(&mut response, &state, &ceremony_cookie(&state), "", 0)?;
    Ok(response)
}
pub async fn queue(State(state): Shared, headers: HeaderMap) -> Result<Html<String>, AppError> {
    let session = auth::session(&state, &headers).await?;
    let csrf = auth::cookie(&headers, &csrf_cookie(&state))?;
    auth::csrf(&session, &csrf)?;
    let reports = store::reports(&state.staff)
        .await?
        .into_iter()
        .map(views::Preview::from)
        .collect();
    Ok(Html(
        views::Queue {
            reports,
            csrf,
            recent: session.recent,
        }
        .render()
        .map_err(|_| AppError::Internal)?,
    ))
}
#[derive(Deserialize)]
pub struct Mutation {
    pub csrf: String,
    #[serde(default)]
    pub board: String,
    #[serde(default)]
    pub target: i64,
    #[serde(default)]
    pub action: String,
}
pub async fn moderate(
    State(state): Shared,
    headers: HeaderMap,
    Form(input): Form<Mutation>,
) -> Result<Redirect, AppError> {
    auth::origin(&headers, &state.config.origin)?;
    let session = auth::session(&state, &headers).await?;
    auth::csrf(&session, &input.csrf)?;
    store::moderate(
        &state.staff,
        &session,
        &input.board,
        input.target,
        &input.action,
    )
    .await?;
    Ok(Redirect::to("/reports"))
}
pub async fn logout(
    State(state): Shared,
    headers: HeaderMap,
    Form(input): Form<Mutation>,
) -> Result<Response, AppError> {
    auth::origin(&headers, &state.config.origin)?;
    let session = auth::session(&state, &headers).await?;
    auth::csrf(&session, &input.csrf)?;
    let token = auth::cookie(&headers, state.config.cookie_name())?;
    sqlx::query("DELETE FROM staff_identity.sessions WHERE token_hash=$1")
        .bind(auth::hash(&token))
        .execute(&state.auth)
        .await?;
    let mut response = Redirect::to("/").into_response();
    set_cookie(&mut response, &state, state.config.cookie_name(), "", 0)?;
    set_cookie(&mut response, &state, &csrf_cookie(&state), "", 0)?;
    Ok(response)
}
