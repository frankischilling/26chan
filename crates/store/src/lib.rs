#![forbid(unsafe_code)]

pub mod media;
mod read;
mod write;
use chrono::{DateTime, Utc};
pub use read::*;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::time::Duration;
pub use write::*;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("Not found.")]
    NotFound,
    #[error("{0}")]
    Invalid(&'static str),
    #[error("{0}")]
    Conflict(&'static str),
    #[error("Database unavailable.")]
    Database(#[from] sqlx::Error),
    #[error("Unsafe database role.")]
    UnsafeRole,
}

#[derive(Clone, sqlx::FromRow)]
pub struct Board {
    pub slug: String,
    pub title: String,
    pub description: String,
    pub max_comment_bytes: i32,
    pub reply_limit: i32,
    pub bump_limit: i32,
    pub thread_limit: i32,
    pub threads_per_page: i32,
    pub worksafe: bool,
}

#[derive(Clone, sqlx::FromRow)]
pub struct Thread {
    pub id: i64,
    pub board: String,
    pub created_at: DateTime<Utc>,
    pub bumped_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
    pub reply_count: i32,
    pub sticky: bool,
    pub closed: bool,
    pub deleted: bool,
}

#[derive(Clone, sqlx::FromRow)]
pub struct Post {
    pub id: i64,
    pub board: String,
    pub thread_id: i64,
    pub name: String,
    pub subject: String,
    pub comment: String,
    pub created_at: DateTime<Utc>,
    pub deleted: bool,
}

pub async fn connect_public(url: &str) -> Result<PgPool, StoreError> {
    let pool = PgPoolOptions::new()
        .max_connections(12)
        .acquire_timeout(Duration::from_secs(3))
        .connect(url)
        .await?;
    let safe: bool = sqlx::query_scalar("SELECT current_user = 'board_public' AND NOT rolsuper AND NOT rolcreatedb AND NOT rolcreaterole AND NOT rolreplication AND NOT rolbypassrls AND NOT EXISTS (SELECT 1 FROM pg_auth_members WHERE member = pg_roles.oid) AND NOT EXISTS (SELECT 1 FROM pg_database WHERE datname = current_database() AND datdba = pg_roles.oid) AND NOT has_schema_privilege(current_user, 'staff_identity', 'USAGE') AND NOT has_schema_privilege(current_user, 'deployment', 'USAGE') FROM pg_roles WHERE rolname = current_user").fetch_one(&pool).await?;
    if !safe {
        pool.close().await;
        return Err(StoreError::UnsafeRole);
    }
    Ok(pool)
}
