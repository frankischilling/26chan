//! Posting admission. Private identities never reach errors or UI.
use crate::StoreError;
use board_domain::poster_id::PublicPostingRateIdentity;
use sqlx::{Postgres, Transaction};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PostingCooldownReason {
    Reply,
    ImageReply,
    Thread,
    CrossBoardThread,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PostingCooldownRejection {
    pub reason: PostingCooldownReason,
    pub remaining_seconds: i64,
}

impl PostingCooldownRejection {
    /// Source S_RENZOKU/S_RENZOKU2/S_RENZOKU3. Pass promotion is deliberately
    /// absent: this backend does not implement a trusted Pass discount.
    pub fn source_message(&self) -> String {
        match self.reason {
            PostingCooldownReason::Reply => format!(
                "Error: You must wait {} before posting a reply.",
                source_duration(self.remaining_seconds)
            ),
            PostingCooldownReason::ImageReply => format!(
                "Error: You must wait {} before posting an image reply.",
                source_duration(self.remaining_seconds)
            ),
            PostingCooldownReason::Thread | PostingCooldownReason::CrossBoardThread => {
                "Error: You must wait longer before posting a new thread.".into()
            }
        }
    }
}

impl std::fmt::Display for PostingCooldownRejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.source_message())
    }
}

// Exact sec2hms(seconds, false, true) behavior from lib/util.php, including
// its leading space below a minute and zero minutes at whole-hour boundaries.
fn source_duration(seconds: i64) -> String {
    let mut result = String::new();
    let hours = seconds / 3600;
    if hours != 0 {
        result.push_str(&format!(
            "{hours} hour{} ",
            if hours == 1 { "" } else { "s" }
        ));
    }
    if seconds / 60 != 0 {
        let minutes = (seconds / 60) % 60;
        result.push_str(&format!(
            "{minutes} minute{}",
            if minutes == 1 { "" } else { "s" }
        ));
    }
    let seconds = seconds % 60;
    if seconds != 0 {
        result.push_str(&format!(
            " {seconds} second{}",
            if seconds == 1 { "" } else { "s" }
        ));
    }
    result
}

pub(crate) async fn lock(
    tx: &mut Transaction<'_, Postgres>,
    actor: &PublicPostingRateIdentity,
    new_thread: bool,
) -> Result<(), StoreError> {
    sqlx::query("SELECT content.lock_posting_actor($1,$2)")
        .bind(actor.as_bytes().as_slice())
        .bind(new_thread)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct ResultRow {
    kind: String,
    remaining_seconds: i64,
}

pub(crate) async fn check(
    tx: &mut Transaction<'_, Postgres>,
    actor: &PublicPostingRateIdentity,
    board: &str,
    parent: i64,
    has_image: bool,
    request_epoch: i64,
) -> Result<(), StoreError> {
    let rows: Vec<ResultRow> = sqlx::query_as(
        "SELECT kind,remaining_seconds FROM content.check_posting_cooldown($1,$2,$3,$4,$5)",
    )
    .bind(actor.as_bytes().as_slice())
    .bind(board)
    .bind(parent)
    .bind(has_image)
    .bind(request_epoch)
    .fetch_all(&mut **tx)
    .await?;
    decode_result(&rows, CheckContext::Ordinary { parent, has_image })
}

/// Only called after the server-owned staff authority issuer succeeds.
pub(crate) async fn check_staff(
    tx: &mut Transaction<'_, Postgres>,
    actor: &PublicPostingRateIdentity,
    board: &str,
    request_epoch: i64,
) -> Result<(), StoreError> {
    let rows: Vec<ResultRow> = sqlx::query_as(
        "SELECT kind,remaining_seconds FROM content.check_staff_posting_cooldown($1,$2,$3)",
    )
    .bind(actor.as_bytes().as_slice())
    .bind(board)
    .bind(request_epoch)
    .fetch_all(&mut **tx)
    .await?;
    decode_result(&rows, CheckContext::Staff)
}

#[derive(Clone, Copy)]
enum CheckContext {
    Ordinary { parent: i64, has_image: bool },
    Staff,
}

fn decode_result(rows: &[ResultRow], context: CheckContext) -> Result<(), StoreError> {
    let invalid = || {
        StoreError::Database(sqlx::Error::Protocol(
            "Invalid posting cooldown result.".into(),
        ))
    };
    let row = match rows {
        [] => return Ok(()),
        [row] if row.remaining_seconds > 0 => row,
        _ => return Err(invalid()),
    };
    let reason = match (context, row.kind.as_str()) {
        (CheckContext::Staff, "reply") => PostingCooldownReason::Reply,
        (
            CheckContext::Ordinary {
                parent,
                has_image: false,
            },
            "reply",
        ) if parent > 0 => PostingCooldownReason::Reply,
        (
            CheckContext::Ordinary {
                parent,
                has_image: true,
            },
            "image",
        ) if parent > 0 => PostingCooldownReason::ImageReply,
        (CheckContext::Ordinary { parent: 0, .. }, "thread") => PostingCooldownReason::Thread,
        (CheckContext::Ordinary { parent: 0, .. }, "cross_board_thread") => {
            PostingCooldownReason::CrossBoardThread
        }
        _ => return Err(invalid()),
    };
    Err(StoreError::PostingCooldownRejected(
        PostingCooldownRejection {
            reason,
            remaining_seconds: row.remaining_seconds,
        },
    ))
}

/// The insert-only history trigger consumes this application-derived context.
/// It is never read from a request field or used as independent SQL authority.
pub(crate) async fn set_insert_actor(
    tx: &mut Transaction<'_, Postgres>,
    actor: &PublicPostingRateIdentity,
) -> Result<(), StoreError> {
    let encoded = actor
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    sqlx::query("SELECT set_config('board.posting_actor',$1,true)")
        .bind(encoded)
        .execute(&mut **tx)
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn malformed_decisions_fail_closed() {
        assert!(
            decode_result(
                &[],
                CheckContext::Ordinary {
                    parent: 1,
                    has_image: false
                }
            )
            .is_ok()
        );
        for (kind, seconds, parent, image) in [
            ("allow", 0, 1, false),
            ("reply", 0, 1, false),
            ("reply", -1, 1, false),
            ("unknown", 1, 1, false),
            ("reply", 1, 0, false),
            ("image", 1, 1, false),
            ("thread", 1, 1, false),
            ("cross_board_thread", 1, 1, false),
        ] {
            let rows = [ResultRow {
                kind: kind.into(),
                remaining_seconds: seconds,
            }];
            assert!(matches!(
                decode_result(
                    &rows,
                    CheckContext::Ordinary {
                        parent,
                        has_image: image
                    }
                ),
                Err(StoreError::Database(_))
            ));
        }
        let rows = [
            ResultRow {
                kind: "reply".into(),
                remaining_seconds: 1,
            },
            ResultRow {
                kind: "reply".into(),
                remaining_seconds: 1,
            },
        ];
        assert!(matches!(
            decode_result(
                &rows,
                CheckContext::Ordinary {
                    parent: 1,
                    has_image: false
                }
            ),
            Err(StoreError::Database(_))
        ));
    }

    #[test]
    fn sql_kinds_decode_into_typed_rejections() {
        for (kind, parent, image, reason) in [
            ("reply", 1, false, PostingCooldownReason::Reply),
            ("image", 1, true, PostingCooldownReason::ImageReply),
            ("thread", 0, false, PostingCooldownReason::Thread),
            (
                "cross_board_thread",
                0,
                true,
                PostingCooldownReason::CrossBoardThread,
            ),
        ] {
            let rows = [ResultRow {
                kind: kind.into(),
                remaining_seconds: 17,
            }];
            match decode_result(
                &rows,
                CheckContext::Ordinary {
                    parent,
                    has_image: image,
                },
            ) {
                Err(StoreError::PostingCooldownRejected(rejection)) => {
                    assert_eq!(rejection.reason, reason);
                    assert_eq!(rejection.remaining_seconds, 17);
                }
                _ => panic!("expected typed rejection"),
            }
        }
    }

    #[test]
    fn staff_decisions_use_reply_wording_without_a_parent_or_image_exception() {
        assert!(decode_result(&[], CheckContext::Staff).is_ok());
        let rows = [ResultRow {
            kind: "reply".into(),
            remaining_seconds: 5,
        }];
        match decode_result(&rows, CheckContext::Staff) {
            Err(StoreError::PostingCooldownRejected(rejection)) => {
                assert_eq!(rejection.reason, PostingCooldownReason::Reply);
                assert_eq!(
                    rejection.source_message(),
                    "Error: You must wait  5 seconds before posting a reply."
                );
            }
            _ => panic!("expected typed staff rejection"),
        }
        for (kind, remaining_seconds) in [
            ("reply", 0),
            ("reply", -1),
            ("image", 1),
            ("thread", 1),
            ("cross_board_thread", 1),
            ("unknown", 1),
        ] {
            let rows = [ResultRow {
                kind: kind.into(),
                remaining_seconds,
            }];
            assert!(matches!(
                decode_result(&rows, CheckContext::Staff),
                Err(StoreError::Database(_))
            ));
        }
        let rows = [
            ResultRow {
                kind: "reply".into(),
                remaining_seconds: 1,
            },
            ResultRow {
                kind: "reply".into(),
                remaining_seconds: 1,
            },
        ];
        assert!(matches!(
            decode_result(&rows, CheckContext::Staff),
            Err(StoreError::Database(_))
        ));
    }

    #[test]
    fn source_duration_preserves_seconds_minutes_and_hours() {
        for (seconds, expected) in [
            (1, " 1 second"),
            (2, " 2 seconds"),
            (60, "1 minute"),
            (61, "1 minute 1 second"),
            (120, "2 minutes"),
            (3600, "1 hour 0 minutes"),
            (3661, "1 hour 1 minute 1 second"),
            (86400, "24 hours 0 minutes"),
        ] {
            assert_eq!(source_duration(seconds), expected);
        }
    }

    #[test]
    fn typed_messages_match_source_errors() {
        let rejected = |reason| PostingCooldownRejection {
            reason,
            remaining_seconds: 61,
        };
        assert_eq!(
            rejected(PostingCooldownReason::Reply).source_message(),
            "Error: You must wait 1 minute 1 second before posting a reply."
        );
        assert_eq!(
            rejected(PostingCooldownReason::ImageReply).source_message(),
            "Error: You must wait 1 minute 1 second before posting an image reply."
        );
        for reason in [
            PostingCooldownReason::Thread,
            PostingCooldownReason::CrossBoardThread,
        ] {
            assert_eq!(
                rejected(reason).source_message(),
                "Error: You must wait longer before posting a new thread."
            );
        }
    }
}
