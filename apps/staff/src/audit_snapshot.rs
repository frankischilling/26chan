//! Immutable saved-content evidence for the two source snapshot actions.
//!
//! These are logical stored values, not a rendered archive. In particular the
//! wordfilter marker carries no discarded text or URL context from its payload.
use crate::AppError;
use sqlx::{Postgres, Transaction};

#[derive(sqlx::FromRow)]
pub(crate) struct TargetSnapshot {
    name: String,
    trip: Option<String>,
    capcode: Option<String>,
    subject: String,
    comment: String,
    comment_format: i16,
    staff_authorized_limits: bool,
    wordfiltered: bool,
    image_spoiler: bool,
    filename: Option<String>,
    dice_result: Option<String>,
    fortune_text: Option<String>,
    fortune_color: Option<String>,
}

pub(crate) enum SnapshotAction {
    ThreadOptions { before: i16, after: i16 },
    Spoiler,
    Unspoiler,
}

impl SnapshotAction {
    fn audit_fields(&self) -> (&'static str, Option<i16>, Option<i16>) {
        match *self {
            Self::ThreadOptions { before, after } => ("thread-options", Some(before), Some(after)),
            Self::Spoiler => ("spoiler", None, None),
            Self::Unspoiler => ("unspoiler", None, None),
        }
    }
}

/// Caller already holds the board and containing thread locks, in that order.
/// Lock only the post side of the outer join. The authorized metadata view also
/// retains filenames for removed attachments; availability is not a predicate.
pub(crate) async fn capture(
    tx: &mut Transaction<'_, Postgres>,
    board: &str,
    target: i64,
    thread: i64,
) -> Result<Option<TargetSnapshot>, AppError> {
    Ok(sqlx::query_as(
        "SELECT p.name,p.trip,p.capcode,p.subject,p.comment,p.comment_format,\
         p.staff_authorized_limits,p.wordfilter_payload IS NOT NULL AS wordfiltered,\
         p.image_spoiler,m.filename,p.dice_result,p.fortune_text,p.fortune_color \
         FROM content.posts p LEFT JOIN content.staff_post_media m ON m.post_id=p.id \
         WHERE p.board=$1 AND p.id=$2 AND p.thread_id=$3 AND NOT p.deleted FOR UPDATE OF p",
    )
    .bind(board)
    .bind(target)
    .bind(thread)
    .fetch_optional(&mut **tx)
    .await?)
}

pub(crate) async fn append(
    tx: &mut Transaction<'_, Postgres>,
    account_id: i64,
    board: &str,
    target: i64,
    action: SnapshotAction,
    snapshot: &TargetSnapshot,
) -> Result<(), AppError> {
    let (action, before, after) = action.audit_fields();
    sqlx::query(
        "INSERT INTO content.moderation_audit(\
         account_id,board,target_id,action,before_mask,after_mask,snapshot_version,\
         snapshot_name,snapshot_trip,snapshot_capcode,snapshot_subject,snapshot_comment,\
         snapshot_comment_format,snapshot_staff_authorized_limits,snapshot_wordfiltered,\
         snapshot_image_spoiler,snapshot_filename,snapshot_dice_result,\
         snapshot_fortune_text,snapshot_fortune_color) \
         VALUES ($1,$2,$3,$4,$5,$6,1,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19)",
    )
    .bind(account_id)
    .bind(board)
    .bind(target)
    .bind(action)
    .bind(before)
    .bind(after)
    .bind(&snapshot.name)
    .bind(&snapshot.trip)
    .bind(&snapshot.capcode)
    .bind(&snapshot.subject)
    .bind(&snapshot.comment)
    .bind(snapshot.comment_format)
    .bind(snapshot.staff_authorized_limits)
    .bind(snapshot.wordfiltered)
    .bind(snapshot.image_spoiler)
    .bind(&snapshot.filename)
    .bind(&snapshot.dice_result)
    .bind(&snapshot.fortune_text)
    .bind(&snapshot.fortune_color)
    .execute(&mut **tx)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::SnapshotAction;

    #[test]
    fn snapshot_actions_keep_existing_audit_identity_and_masks() {
        assert_eq!(
            SnapshotAction::ThreadOptions {
                before: 1,
                after: 30,
            }
            .audit_fields(),
            ("thread-options", Some(1), Some(30))
        );
        assert_eq!(
            SnapshotAction::Spoiler.audit_fields(),
            ("spoiler", None, None)
        );
        assert_eq!(
            SnapshotAction::Unspoiler.audit_fields(),
            ("unspoiler", None, None)
        );
    }
}
