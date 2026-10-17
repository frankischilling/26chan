//! Report-time public presentation. It carries no moderation authority.
use crate::{
    AppError,
    store::{Attachment, Report},
    views::Preview,
};
use board_domain::{PostLimits, WordfilterLimits, formatting::Line};
use chrono::{DateTime, Utc};
use serde::Deserialize;
use sqlx::{Postgres, Transaction};
use std::collections::HashMap;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CapturedPost {
    board: String,
    post_id: i64,
    thread_id: i64,
    created_at: DateTime<Utc>,
    name: String,
    trip: Option<String>,
    poster_id: Option<String>,
    capcode: Option<String>,
    country: Option<String>,
    country_name: Option<String>,
    board_flag: Option<String>,
    board_flag_type: String,
    flag_name: Option<String>,
    subject: String,
    comment: String,
    #[serde(default)]
    staff_notices: Vec<String>,
    comment_format: i16,
    file_deleted: bool,
    staff_authorized_limits: bool,
    wordfilter_payload: Option<String>,
    saved_presentation: Option<String>,
    drawing_time_seconds: Option<i32>,
    drawing_source_post_id: Option<i64>,
    archived: bool,
    relative_subject: Option<String>,
    attachment: Option<CapturedAttachment>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CapturedAttachment {
    post_id: i64,
    filename: String,
    bytes: i64,
    width: i32,
    height: i32,
    spoiler: bool,
    tim: i64,
    thumbnail_width: Option<i32>,
    thumbnail_height: Option<i32>,
    available: bool,
    md5: Option<String>,
    output_format: String,
}

#[derive(sqlx::FromRow)]
struct Row {
    report_id: i64,
    board: String,
    post_id: i64,
    version: i16,
    captured_at: DateTime<Utc>,
    payload: Option<String>,
    media_available: bool,
    media_spoiler: bool,
}

pub(crate) struct ReportPostSnapshot {
    post: CapturedPost,
    captured_at: DateTime<Utc>,
    lines: Vec<Line>,
    wordfilter: Option<Vec<u8>>,
    attachment: Option<Attachment>,
}

pub struct SnapshotLabel {
    pub captured_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
    pub thread_id: i64,
    pub archived: bool,
    pub file_deleted: bool,
    pub relative_subject: Option<String>,
}

pub(crate) async fn ensure_ready(tx: &mut Transaction<'_, Postgres>) -> Result<(), AppError> {
    let ready: bool = sqlx::query_scalar("SELECT content.report_snapshots_ready()")
        .fetch_one(&mut **tx)
        .await?;
    if !ready {
        return Err(AppError::Internal);
    }
    Ok(())
}

/// IDs must come from the caller's authorized group selection in this same
/// read-only snapshot. A cumulative 16 MiB cap bounds payload materialization;
/// exceeding it refuses the page instead of disguising a capture as legacy.
pub(crate) async fn load(
    tx: &mut Transaction<'_, Postgres>,
    ids: &[i64],
) -> Result<HashMap<i64, ReportPostSnapshot>, AppError> {
    if ids.len() > 100 {
        return Err(AppError::Internal);
    }
    let rows: Vec<Row> = sqlx::query_as(
        r#"
        WITH selected AS (
            SELECT s.*,sum(octet_length(payload::text)::bigint+128)
                OVER (ORDER BY report_id) AS page_bytes
            FROM content.report_post_snapshots s WHERE report_id=ANY($1)
        )
        SELECT s.report_id,s.board,s.post_id,s.version,s.captured_at,
               CASE WHEN s.page_bytes<=16777216 THEN s.payload::text END AS payload,
               coalesce(m.available AND m.tim=(s.payload->'attachment'->>'tim')::bigint
                   AND m.output_format::text=s.payload->'attachment'->>'output_format',false)
                   AS media_available,
               coalesce(m.spoiler,false) AS media_spoiler
        FROM selected s LEFT JOIN content.staff_post_media m ON m.post_id=s.post_id
        ORDER BY s.report_id
    "#,
    )
    .bind(ids)
    .fetch_all(&mut **tx)
    .await?;
    let mut snapshots = HashMap::with_capacity(rows.len());
    for row in rows {
        let payload = row.payload.ok_or(AppError::Capacity)?;
        let post: CapturedPost = serde_json::from_str(&payload).map_err(|_| AppError::Internal)?;
        board_domain::post_notice::validate_saved(&post.staff_notices)
            .map_err(|_| AppError::Internal)?;
        if row.version != 1
            || post.board != row.board
            || post.post_id != row.post_id
            || post.post_id <= 0
            || post.thread_id <= 0
            || board_domain::BoardSlug::parse(&post.board).is_err()
        {
            return Err(AppError::Internal);
        }
        let limits = if post.staff_authorized_limits {
            PostLimits::authorized(board_domain::MAX_AUTHORIZED_COMMENT_CHARS)
                .expect("finite persisted staff bound")
        } else {
            PostLimits::ordinary(board_domain::MAX_COMMENT_CHARS)
        };
        let bounds = WordfilterLimits::for_post(limits);
        if post.comment.len() > bounds.output_bytes() {
            return Err(AppError::Internal);
        }
        let wordfilter = decode_hex(post.wordfilter_payload.as_deref(), bounds.stored_bytes())?;
        let frozen = decode_hex(post.saved_presentation.as_deref(), bounds.stored_bytes())?;
        let lines = if let Some(bytes) = frozen.as_deref() {
            if wordfilter.is_some() {
                return Err(AppError::Internal);
            }
            let saved = board_domain::saved_presentation::FrozenPresentation::decode(bytes, limits)
                .map_err(|_| AppError::Internal)?;
            if saved.source_projection() != post.comment {
                return Err(AppError::Internal);
            }
            saved.unresolved_lines()
        } else {
            board_domain::formatting::parse_saved_comment_with_limits(
                &post.comment,
                post.comment_format,
                &post.board,
                wordfilter.as_deref(),
                limits,
            )
        };
        let attachment = post
            .attachment
            .as_ref()
            .map(|a| {
                if a.post_id != post.post_id
                    || a.tim <= 0
                    || a.bytes <= 0
                    || a.width <= 0
                    || a.height <= 0
                    || a.thumbnail_width.is_some_and(|w| w <= 0)
                    || a.thumbnail_height.is_some_and(|h| h <= 0)
                {
                    return Err(AppError::Internal);
                }
                let output_format = match a.output_format.as_str() {
                    "png" => board_store::media_assets::MediaFormat::Png,
                    "jpeg" => board_store::media_assets::MediaFormat::Jpeg,
                    "gif" => board_store::media_assets::MediaFormat::Gif,
                    "pdf" => board_store::media_assets::MediaFormat::Pdf,
                    _ => return Err(AppError::Internal),
                };
                Ok(Attachment {
                    post_id: a.post_id,
                    filename: a.filename.clone(),
                    bytes: a.bytes,
                    width: a.width,
                    height: a.height,
                    spoiler: a.spoiler || row.media_spoiler,
                    tim: a.tim,
                    thumbnail_width: a.thumbnail_width,
                    thumbnail_height: a.thumbnail_height,
                    available: a.available && row.media_available,
                    md5: a.md5.clone(),
                    output_format,
                })
            })
            .transpose()?;
        if snapshots
            .insert(
                row.report_id,
                ReportPostSnapshot {
                    post,
                    captured_at: row.captured_at,
                    lines,
                    wordfilter,
                    attachment,
                },
            )
            .is_some()
        {
            return Err(AppError::Internal);
        }
    }
    Ok(snapshots)
}

fn decode_hex(value: Option<&str>, maximum: usize) -> Result<Option<Vec<u8>>, AppError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if !value.len().is_multiple_of(2) || value.len() > maximum * 2 {
        return Err(AppError::Internal);
    }
    fn digit(b: u8) -> Result<u8, AppError> {
        match b {
            b'0'..=b'9' => Ok(b - b'0'),
            b'a'..=b'f' => Ok(b - b'a' + 10),
            _ => Err(AppError::Internal),
        }
    }
    value
        .as_bytes()
        .chunks_exact(2)
        .map(|p| Ok(digit(p[0])? * 16 + digit(p[1])?))
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

impl ReportPostSnapshot {
    pub(crate) fn matches(&self, board: &str, post_id: i64) -> bool {
        self.post.board == board && self.post.post_id == post_id
    }

    pub(crate) fn preview(self, mut report: Report) -> Preview {
        // Keep current IDs, thread action flags, disposition and permissions.
        // Only display values and independently rechecked media are replaced.
        let label = SnapshotLabel {
            captured_at: self.captured_at,
            created_at: self.post.created_at,
            thread_id: self.post.thread_id,
            archived: self.post.archived,
            file_deleted: self.post.file_deleted,
            relative_subject: self.post.relative_subject,
        };
        report.name = self.post.name;
        report.trip = self.post.trip;
        report.poster_id = self.post.poster_id;
        report.capcode = self.post.capcode;
        report.country = self.post.country;
        report.country_name = self.post.country_name;
        report.board_flag = self.post.board_flag;
        report.board_flag_type = self.post.board_flag_type;
        report.flag_name = self.post.flag_name;
        report.subject = self.post.subject;
        report.comment = self.post.comment;
        report.staff_notices = self.post.staff_notices;
        report.comment_format = self.post.comment_format;
        report.staff_authorized_limits = self.post.staff_authorized_limits;
        report.wordfilter_payload = self.wordfilter;
        report.drawing_time_seconds = self.post.drawing_time_seconds;
        report.drawing_source_post_id = self.post.drawing_source_post_id;
        report.attachment = self.attachment;
        let drawing = board_domain::drawing_annotation::DrawingAnnotation::from_saved(
            report.drawing_time_seconds,
            report.drawing_source_post_id,
        );
        Preview {
            has_open: report.state == "open",
            moderator_summary: None,
            report,
            lines: self.lines,
            drawing,
            snapshot: Some(label),
        }
    }

    pub(crate) fn history_preview(self, report_id: i64, state: String) -> Preview {
        let report = Report {
            id: report_id,
            board: self.post.board.clone(),
            post_id: self.post.post_id,
            thread_id: self.post.thread_id,
            archived: self.post.archived,
            state,
            deleted: true,
            ..Report::default()
        };
        self.preview(report)
    }
}
