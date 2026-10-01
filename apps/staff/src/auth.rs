use crate::{
    AppError, AppState,
    access::{Level, Permissions},
};
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
    #[sqlx(flatten)]
    pub permissions: Permissions,
}

impl Session {
    pub fn at_least(&self, level: Level) -> bool {
        Level::parse(&self.role).is_some_and(|current| current >= level)
    }
}

pub struct Authority {
    pub session: Session,
    transaction: sqlx::Transaction<'static, sqlx::Postgres>,
    token_hash: Vec<u8>,
    idle_seconds: i32,
}

impl Authority {
    /// Account and session locks stay held while the content transaction runs.
    /// Recheck the clock after content locks, before making its changes durable.
    pub async fn ensure_current(&mut self, require_recent: bool) -> Result<(), AppError> {
        let status: Option<(bool,bool)> = sqlx::query_as(
            "SELECT s.expires_at>clock_timestamp() AND s.last_activity_at>clock_timestamp()-make_interval(secs=>$2) \
             AND a.revoked_at IS NULL AND a.role IN ('janitor','moderator','manager','admin'), \
             s.authenticated_at>clock_timestamp()-interval '10 minutes' \
             FROM staff_identity.sessions s JOIN staff_identity.accounts a ON a.id=s.account_id \
             WHERE s.token_hash=$1",
        ).bind(&self.token_hash).bind(self.idle_seconds).fetch_optional(&mut *self.transaction).await?;
        let (live, recent) = status.ok_or(AppError::Unauthorized)?;
        if !live {
            return Err(AppError::Unauthorized);
        }
        if require_recent && !recent {
            return Err(AppError::Recent);
        }
        Ok(())
    }

    pub async fn finish(self) -> Result<Session, AppError> {
        self.transaction.commit().await?;
        Ok(self.session)
    }
}

pub async fn guard(state: &AppState, headers: &HeaderMap) -> Result<Authority, AppError> {
    let value = cookie(headers, state.config.cookie_name())?;
    let token_hash = hash(&value);
    let idle_seconds = state.config.idle_timeout.as_secs() as i32;
    let mut transaction = state.auth.begin().await?;
    let session: Session = sqlx::query_as("SELECT * FROM staff_identity.lock_session($1,$2)")
        .bind(&token_hash)
        .bind(idle_seconds)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(AppError::Unauthorized)?;
    Ok(Authority {
        session,
        transaction,
        token_hash,
        idle_seconds,
    })
}

pub async fn session(state: &AppState, headers: &HeaderMap) -> Result<Session, AppError> {
    guard(state, headers).await?.finish().await
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
    let posting: bool = match expected {
        "board_auth" => {
            sqlx::query("SELECT public_capcode,allow_boards,deny_boards,flags FROM staff_identity.accounts LIMIT 0")
                .execute(pool)
                .await?;
            sqlx::query_scalar("SELECT has_function_privilege(current_user,'staff_identity.issue_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz)','EXECUTE') AND coalesce(has_function_privilege(current_user,to_regprocedure('staff_identity.lock_session(bytea,integer)'),'EXECUTE'),false) AND NOT has_column_privilege(current_user,'staff_identity.accounts','allow_boards','UPDATE') AND NOT has_column_privilege(current_user,'staff_identity.accounts','deny_boards','UPDATE') AND NOT has_column_privilege(current_user,'staff_identity.accounts','flags','UPDATE') AND NOT EXISTS(SELECT 1 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='content' AND p.proname='consume_staff_post_authority' AND has_function_privilege(current_user,p.oid,'EXECUTE'))").fetch_one(pool).await?
        }
        "board_staff" => {
            sqlx::query("SELECT capcode FROM content.posts LIMIT 0")
                .execute(pool)
                .await?;
            sqlx::query("SELECT id FROM content.visible_threads LIMIT 0")
                .execute(pool)
                .await?;
            sqlx::query_scalar("SELECT has_function_privilege(current_user,'content.consume_staff_post_authority(bytea,bigint,text,bigint,text,text,text,timestamptz)','EXECUTE') AND NOT EXISTS(SELECT 1 FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='staff_identity' AND p.proname='issue_post_authority' AND has_function_privilege(current_user,p.oid,'EXECUTE'))").fetch_one(pool).await?
        }
        _ => false,
    };
    if !posting {
        return Err(AppError::Forbidden);
    }
    Ok(())
}
