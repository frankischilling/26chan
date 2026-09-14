#![forbid(unsafe_code)]

mod archives;
mod board_snapshot;
pub use archives::{ArchiveEntry, ArchiveSnapshot, archive_snapshot};
pub mod legacy_media;
pub mod media;
pub mod media_assets;
pub mod media_intake;
pub mod monitoring;
pub mod post_media;
mod read;
mod write;
pub use board_snapshot::*;
use chrono::{DateTime, Utc};
pub use read::*;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::time::Duration;
pub use write::*;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("Not found.")]
    NotFound,
    #[error("Page not found.")]
    PageNotFound,
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
    pub max_comment_chars: i32,
    pub comment_code_spacing: bool,
    pub comment_sjis_spacing: bool,
    pub comment_max_lines: i32,
    pub comment_spoiler_cleanup: bool,
    pub require_subject: bool,
    pub op_markup: bool,
    pub forced_anon: bool,
    pub text_only: bool,
    pub reply_limit: i32,
    pub bump_limit: i32,
    pub permasage_hours: i32,
    pub op_bump_limit: bool,
    pub op_bump_initial_seconds: i32,
    pub op_bump_repeat_seconds: i32,
    pub thread_limit: i32,
    pub threads_per_page: i32,
    pub worksafe: bool,
    pub archive_retention_seconds: i32,
    pub archive_limit: i32,
    pub image_limit: i32,
}

impl Board {
    pub fn check_attachment_allowed(&self, parent: i64, attached: bool) -> Result<(), StoreError> {
        if self.text_only && parent != 0 && attached {
            return Err(StoreError::Invalid("You cannot upload files on this board"));
        }
        Ok(())
    }

    pub fn comment_spacing(&self) -> board_domain::CommentSpacing<'_> {
        board_domain::CommentSpacing::for_board(
            &self.slug,
            self.comment_code_spacing,
            self.comment_sjis_spacing,
        )
        .with_line_rules(
            self.comment_max_lines as usize,
            self.comment_spoiler_cleanup,
        )
    }
}

#[derive(Clone, sqlx::FromRow)]
pub struct Thread {
    pub id: i64,
    pub board: String,
    pub created_at: DateTime<Utc>,
    pub bumped_at: DateTime<Utc>,
    pub modified_at: DateTime<Utc>,
    pub http_modified_at: DateTime<Utc>,
    pub reply_count: i32,
    pub sticky: bool,
    pub permasage: bool,
    pub permaage: bool,
    pub undead: bool,
    pub closed: bool,
    pub deleted: bool,
    pub archived_at: Option<DateTime<Utc>>,
    pub archive_expires_at: Option<DateTime<Utc>>,
}

#[derive(Clone, sqlx::FromRow)]
pub struct Post {
    pub id: i64,
    pub board: String,
    pub thread_id: i64,
    pub name: String,
    pub subject: String,
    pub comment: String,
    pub comment_format: i16,
    pub created_at: DateTime<Utc>,
    pub deleted: bool,
    #[sqlx(skip)]
    pub attachment: Option<post_media::PostAttachment>,
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
