use crate::{AppError, AppState};
use axum::http::HeaderMap;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use rand::{RngCore, rngs::OsRng};
use sha2::{Digest, Sha256};
use sqlx::PgPool;

pub fn token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}
pub fn hash(value: &str) -> Vec<u8> {
    Sha256::digest(value.as_bytes()).to_vec()
}
pub fn cookie(headers: &HeaderMap, name: &str) -> Result<String, AppError> {
    let mut values = headers
        .get_all("cookie")
        .iter()
        .filter_map(|h| h.to_str().ok())
        .flat_map(|s| s.split(';'))
        .filter_map(|s| s.trim().split_once('='))
        .filter(|(k, _)| *k == name)
        .map(|(_, v)| v);
    let value = values.next().ok_or(AppError::Unauthorized)?;
    if values.next().is_some()
        || value.len() != 43
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err(AppError::Unauthorized);
    }
    Ok(value.to_owned())
}
pub fn origin(headers: &HeaderMap, expected: &str) -> Result<(), AppError> {
    if headers.get_all("origin").iter().count() != 1
        || headers.get("origin").and_then(|v| v.to_str().ok()) != Some(expected)
        || headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()) != Some("same-origin")
    {
        return Err(AppError::Forbidden);
    }
    Ok(())
}
#[derive(sqlx::FromRow)]
pub struct Session {
    pub account_id: i64,
    pub role: String,
    pub csrf_hash: Vec<u8>,
    pub recent: bool,
}
pub async fn session(state: &AppState, headers: &HeaderMap) -> Result<Session, AppError> {
    let value = cookie(headers, state.config.cookie_name())?;
    let session: Session = sqlx::query_as("SELECT s.account_id,a.role,s.csrf_hash,(s.authenticated_at>clock_timestamp()-interval '10 minutes') AS recent FROM staff_identity.sessions s JOIN staff_identity.accounts a ON a.id=s.account_id JOIN staff_identity.credentials c ON c.id=s.credential_id AND c.account_id=a.id WHERE s.token_hash=$1 AND s.expires_at>clock_timestamp() AND a.revoked_at IS NULL")
        .bind(hash(&value)).fetch_optional(&state.auth).await?.ok_or(AppError::Unauthorized)?;
    if !matches!(session.role.as_str(), "moderator" | "admin") {
        return Err(AppError::Forbidden);
    }
    Ok(session)
}
pub fn csrf(session: &Session, value: &str) -> Result<(), AppError> {
    if value.len() != 43 || hash(value) != session.csrf_hash {
        return Err(AppError::Forbidden);
    }
    Ok(())
}
pub async fn check_identity(pool: &PgPool, expected: &str) -> Result<(), AppError> {
    let (name, privileged): (String,bool) = sqlx::query_as("SELECT current_user::text,rolsuper OR rolcreatedb OR rolcreaterole OR rolreplication OR rolbypassrls FROM pg_roles WHERE rolname=current_user").fetch_one(pool).await?;
    if name != expected || privileged {
        return Err(AppError::Forbidden);
    }
    let authority: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_auth_members WHERE member=(SELECT oid FROM pg_roles WHERE rolname=current_user)) OR EXISTS (SELECT 1 FROM pg_database WHERE datname=current_database() AND datdba=(SELECT oid FROM pg_roles WHERE rolname=current_user)) OR EXISTS (SELECT 1 FROM pg_namespace WHERE nspowner=(SELECT oid FROM pg_roles WHERE rolname=current_user)) OR has_schema_privilege(current_user,'deployment','USAGE') OR has_schema_privilege(current_user,'post_secrets','USAGE')").fetch_one(pool).await?;
    if authority {
        return Err(AppError::Forbidden);
    }
    let cross: bool = sqlx::query_scalar("SELECT CASE WHEN current_user='board_auth' THEN has_schema_privilege(current_user,'content','USAGE') ELSE has_schema_privilege(current_user,'staff_identity','USAGE') END").fetch_one(pool).await?;
    if cross {
        return Err(AppError::Forbidden);
    }
    Ok(())
}
