#![forbid(unsafe_code)]

pub mod anonymous_session;
mod archive_deadline;
mod archives;
pub mod automatic_admission;
pub mod blotter;
mod board_snapshot;
pub mod site_content;
pub use blotter::BlotterMessage;
mod content_admission;
pub mod content_erasure;
pub mod historical_erasure;
pub mod homepage;
pub mod media_erasure;
pub use archives::{
    ArchiveEntry, ArchivePageEntry, ArchivePageSnapshot, ArchiveSnapshot, MAX_ARCHIVE_JSON_ENTRIES,
    MAX_ARCHIVE_PAGE_READ_BYTES, archive_page_snapshot, archive_snapshot,
};
pub mod legacy_media;
pub mod media;
pub mod media_assets;
pub mod media_intake;
pub mod monitoring;
mod op_bump;
pub mod paired_candidates;
mod poll_voting;
mod polls;
pub use poll_voting::{POLL_VOTE_READINESS_SQL, PollVoteOutcome, cast_poll_vote, has_poll_vote};
pub use polls::{
    POLL_READINESS_SQL, PollOption, PollSnapshot, PollSummary, poll_catalogue, poll_snapshot,
};
pub mod post_media;
pub mod posting_challenge;
mod posting_cooldown;
pub use posting_cooldown::{PostingCooldownReason, PostingCooldownRejection};
mod public_deletion;
pub use public_deletion::{
    PublicDeletionBatch, PublicDeletionContext, public_deletion_precheck,
    public_deletion_quota_precheck, public_deletion_target_exists,
};
pub mod quote_targets;
mod read;
pub use quote_targets::{MAX_QUOTE_TARGETS, QuoteTargets};
pub mod report_admission;
pub mod report_catalog;
pub mod report_categories;
pub mod report_challenge;
pub mod restrictions;
pub use report_categories::{CategoryChoice, CategoryForm, CategoryKind, category_form};
mod report_target;
pub use report_target::{ReportTarget, report_target};
mod robot9000;
mod rss;
pub mod sticky_retention;
mod thread_quota;
mod thread_statistics;
pub mod thread_transfer;
pub mod verification;
mod write;
mod write_challenge;
pub use board_snapshot::*;
use chrono::{DateTime, Utc};
pub use read::*;
pub use rss::{RssSnapshot, rss_snapshot};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::time::Duration;
pub use thread_statistics::{ThreadStatistics, thread_statistics};
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
    #[error("{0}")]
    PublicDeletionRejected(&'static str),
    #[error("Authorization is no longer valid.")]
    AuthorizationChanged,
    #[error("{0}")]
    ContentRejected(String),
    #[error("Post accepted.")]
    ContentQuiet { post: i64 },
    #[error("Database unavailable.")]
    Database(#[from] sqlx::Error),
    #[error("Unsafe database role.")]
    UnsafeRole,
    #[error("Server randomness is unavailable.")]
    RandomnessUnavailable,
    #[error("Snapshot content exceeds its read budget.")]
    ReadLimit,
    #[error("{0}")]
    Robot9000Rejected(String),
    #[error("{0}")]
    PostingCooldownRejected(PostingCooldownRejection),
}

/// Audited source SUBTITLE profiles. No stored HTML or configurable URL is rendered.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, sqlx::Type)]
#[sqlx(type_name = "text", rename_all = "snake_case")]
pub enum BoardSubtitle {
    #[default]
    None,
    Fiction,
    WorksafeGif,
}

impl BoardSubtitle {
    pub fn is_fiction(self) -> bool {
        self == Self::Fiction
    }

    pub fn is_worksafe_gif(self) -> bool {
        self == Self::WorksafeGif
    }
}

#[derive(Clone, sqlx::FromRow)]
pub struct Board {
    pub slug: String,
    pub source_order: i32,
    pub catalog_enabled: bool,
    pub json_enabled: bool,
    pub staff_only: bool,
    pub meta_board: bool,
    pub upload_board: bool,
    pub rss_enabled: bool,
    pub title: String,
    pub description: String,
    pub show_blotter: bool,
    pub board_subtitle: BoardSubtitle,
    pub max_comment_chars: i32,
    pub max_authorized_comment_chars: i32,
    pub comment_code_spacing: bool,
    pub comment_sjis_spacing: bool,
    pub math_tags: bool,
    /// Imported ENABLE_PAINTERJS policy, including boards needing unsupported replays.
    pub oekaki: bool,
    pub oekaki_replays: bool,
    pub oekaki_width: i32,
    pub oekaki_height: i32,
    pub comment_max_lines: i32,
    pub comment_spoiler_cleanup: bool,
    pub custom_spoiler_count: i32,
    pub spoiler_thumbnail_assets: Vec<String>,
    pub require_subject: bool,
    pub op_markup: bool,
    pub forced_anon: bool,
    pub strip_tripcode: bool,
    pub user_ids: bool,
    pub country_flags: bool,
    pub board_flag_type: String,
    pub board_flags: Vec<String>,
    pub text_only: bool,
    pub reply_limit: i32,
    pub bump_limit: i32,
    pub permasage_hours: i32,
    pub posting_reply_seconds: i32,
    pub posting_image_seconds: i32,
    pub posting_thread_seconds: i32,
    pub user_thread_limit: i32,
    pub user_thread_period_hours: i32,
    pub op_bump_limit: bool,
    pub op_bump_initial_seconds: i32,
    pub op_bump_repeat_seconds: i32,
    pub thread_limit: i32,
    pub expire_neglected: bool,
    pub threads_per_page: i32,
    pub replies_shown: i32,
    pub worksafe: bool,
    pub archive_retention_seconds: i32,
    pub archive_limit: i32,
    pub autoarchive_cap: i32,
    pub image_limit: i32,
    pub max_image_bytes: i64,
    pub min_image_width: i32,
    pub min_image_height: i32,
    pub max_image_dimension: i32,
    pub gif_only: bool,
    pub pdf_enabled: bool,
    pub video_enabled: bool,
    pub video_audio: bool,
    pub max_video_bytes: i64,
    pub max_video_duration: i32,
    pub max_video_dimension: i32,
    pub dice_roll: bool,
    pub fortune_trip: bool,
    pub robot9000: bool,
    pub robot9000_state_limit: i32,
    pub word_filter_enabled: bool,
    pub word_filter_profile: i16,
}

impl Board {
    /// Zero disables archives; -1 enables storage without an age limit.
    pub fn archives_enabled(&self) -> bool {
        self.archive_retention_seconds != 0
    }

    /// Preserve the source's ordinary autoarchive threshold independently of
    /// the existing sticky/undead reply window.
    pub fn ordinary_reply_limit(&self) -> i32 {
        if self.archives_enabled() {
            self.reply_limit.max(self.autoarchive_cap)
        } else {
            self.reply_limit
        }
    }

    /// The ordinary PNG editor uses an already-enabled public upload workflow.
    /// Keep the imported source switch separate from this supported capability.
    pub fn ordinary_drawing_enabled(&self, media_enabled: bool) -> bool {
        media_enabled
            && self.oekaki
            && !self.staff_only
            && !self.text_only
            && self.image_limit > 0
            && !self.gif_only
    }

    /// Original extension.js exposes existing-image Edit only on /i/. The QR
    /// import has replay disabled despite this board's optional replay support.
    pub fn drawing_edit_enabled(&self, media_enabled: bool) -> bool {
        media_enabled
            && self.slug == "i"
            && self.oekaki
            && self.oekaki_replays
            && !self.staff_only
            && !self.text_only
            && self.image_limit > 0
            && !self.gif_only
            && (1..=1024).contains(&self.oekaki_width)
            && (1..=1024).contains(&self.oekaki_height)
            && i64::from(self.oekaki_width) * i64::from(self.oekaki_height) <= 1024 * 1024
    }

    pub fn flag_options(&self) -> Vec<(&'static str, &'static str)> {
        board_domain::board_flags::flags(&self.board_flag_type)
            .iter()
            .filter(|flag| self.board_flags.iter().any(|enabled| enabled == flag.code))
            .map(|flag| (flag.code, flag.selector))
            .collect()
    }
    pub fn image_file_limit(&self) -> usize {
        let maximum = if board_domain::still_image_policy::configured(&self.slug) {
            10_485_760
        } else {
            8_388_608
        };
        self.max_image_bytes.max(0).min(maximum) as usize
    }

    pub fn attachment_file_limit(&self, filename: &str) -> usize {
        let filename = filename.to_ascii_lowercase();
        if self.video_enabled && (filename.ends_with(".webm") || filename.ends_with(".mp4")) {
            self.image_file_limit()
                .min(8_388_608)
                .min(self.max_video_bytes.max(0) as usize)
        } else if filename.ends_with(".gif") || filename.ends_with(".pdf") {
            self.image_file_limit().min(8_388_608)
        } else {
            self.image_file_limit()
        }
    }

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
    #[sqlx(try_from = "archive_deadline::ArchiveDeadline")]
    pub archive_expires_at: Option<DateTime<Utc>>,
}

impl Board {
    pub fn preview_reply_limit(&self, sticky: bool) -> usize {
        board_domain::preview::reply_limit(self.replies_shown as usize, sticky)
    }
}

#[derive(Clone, sqlx::FromRow)]
pub struct Post {
    pub id: i64,
    pub board: String,
    pub thread_id: i64,
    pub name: String,
    pub trip: Option<String>,
    pub poster_id: Option<String>,
    #[sqlx(default)]
    pub json_op_poster_id: Option<String>,
    pub capcode: Option<String>,
    pub country: Option<String>,
    pub country_name: Option<String>,
    pub board_flag: Option<String>,
    pub board_flag_type: String,
    pub flag_name: Option<String>,
    pub subject: String,
    pub image_spoiler: bool,
    pub comment: String,
    /// Source Oekaki metadata is generated after accepted image insertion.
    /// It does not prove that the submitted pixels came from this source.
    pub drawing_time_seconds: Option<i32>,
    pub drawing_source_post_id: Option<i64>,
    pub comment_format: i16,
    pub staff_authorized_limits: bool,
    pub wordfilter_payload: Option<Vec<u8>>,
    pub saved_presentation: Option<Vec<u8>>,
    pub dice_result: Option<String>,
    pub fortune_text: Option<String>,
    pub fortune_color: Option<String>,
    pub created_at: DateTime<Utc>,
    pub deleted: bool,
    /// A file-only action can target a post with no attachment.
    pub file_deleted: bool,
    #[sqlx(skip)]
    pub attachment: Option<post_media::PostAttachment>,
}

impl Post {
    /// Historical attachment flags remain effective without a data backfill.
    pub fn file_deleted(&self) -> bool {
        self.file_deleted || self.attachment.as_ref().is_some_and(|a| a.file_deleted)
    }

    pub fn formatted_lines(&self) -> Vec<board_domain::Line> {
        formatted_saved_lines(
            &self.comment,
            self.comment_format,
            &self.board,
            self.wordfilter_payload.as_deref(),
            self.saved_presentation.as_deref(),
            self.staff_authorized_limits,
        )
    }
}

/// Render saved typed data without applying destination-board source rules.
/// Search callers may supply an excerpt, so full projection equality belongs to
/// transfer admission rather than this reader.
pub fn formatted_saved_lines(
    comment: &str,
    format: i16,
    board: &str,
    wordfilter: Option<&[u8]>,
    frozen: Option<&[u8]>,
    staff_authorized_limits: bool,
) -> Vec<board_domain::Line> {
    let max_chars = if staff_authorized_limits {
        board_domain::MAX_AUTHORIZED_COMMENT_CHARS
    } else {
        board_domain::MAX_COMMENT_CHARS
    };
    let limits = if staff_authorized_limits {
        board_domain::PostLimits::authorized(max_chars).expect("finite persisted staff bound")
    } else {
        board_domain::PostLimits::ordinary(max_chars)
    };
    if let Some(bytes) = frozen {
        if wordfilter.is_none()
            && let Ok(saved) =
                board_domain::saved_presentation::FrozenPresentation::decode(bytes, limits)
        {
            return saved.unresolved_lines();
        }
        // Corruption fallback only: bounded literal text, escaped by renderers.
        // Never parse this projected text as markup, links, quotes or filters.
        let max_bytes = max_chars * 4;
        let end = comment
            .char_indices()
            .take(max_chars)
            .take_while(|(offset, ch)| offset + ch.len_utf8() <= max_bytes)
            .last()
            .map_or(0, |(offset, ch)| offset + ch.len_utf8());
        return vec![board_domain::Line {
            green: false,
            tokens: vec![board_domain::Token::Text(comment[..end].to_owned())],
        }];
    }
    board_domain::formatting::parse_saved_comment_with_limits(
        comment, format, board, wordfilter, limits,
    )
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
    let ready: bool = sqlx::query_scalar(
        "SELECT content.restrictions_ready() AND content.public_challenge_ready()",
    )
    .fetch_one(&pool)
    .await?;
    if !ready {
        pool.close().await;
        return Err(StoreError::UnsafeRole);
    }
    Ok(pool)
}
