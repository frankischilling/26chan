//! Bump-only evidence; never used to attribute public OP markup or poster counts.
use crate::StoreError;
use board_domain::poster_id::PublicPostingRateIdentity;
use chrono::{DateTime, Utc};
use sqlx::{Postgres, Transaction};
use std::net::IpAddr;

#[derive(Default, sqlx::FromRow)]
struct Evidence {
    own_reply: bool,
    latest_post_id: Option<i64>,
    latest_created_at: Option<DateTime<Utc>>,
}

impl Evidence {
    fn candidate(&self) -> Result<Option<(i64, DateTime<Utc>)>, StoreError> {
        match (self.latest_post_id, self.latest_created_at) {
            (None, None) => Ok(None),
            (Some(id), Some(time)) if id > 0 => Ok(Some((id, time))),
            _ => Err(StoreError::Database(sqlx::Error::Protocol(
                "Invalid OP bump context result.".into(),
            ))),
        }
    }
}

/// Called only after issuer-authenticated timer eligibility and board policy.
/// Legacy ownership can survive key rotation, while current-key reply evidence
/// can exist even when that key cannot prove the OP. Merge by post ID, not time.
pub(crate) async fn context(
    tx: &mut Transaction<'_, Postgres>,
    actor: &PublicPostingRateIdentity,
    board: &str,
    thread: i64,
    staff: bool,
    peer: Option<IpAddr>,
    legacy_public_own: bool,
) -> Result<(bool, Option<DateTime<Utc>>), StoreError> {
    let fresh: Evidence = sqlx::query_as(
        "SELECT own_reply,latest_post_id,latest_created_at FROM content.posting_op_bump_context($1,$2,$3)",
    )
    .bind(actor.as_bytes().as_slice())
    .bind(board)
    .bind(thread)
    .fetch_optional(&mut **tx)
    .await?
    .unwrap_or_default();
    let legacy = if staff {
        sqlx::query_as(
            "SELECT own_reply,latest_post_id,latest_created_at FROM content.staff_op_bump_context($1,$2,$3)",
        )
        .bind(board)
        .bind(thread)
        .bind(peer.map(|peer| peer.to_canonical().to_string()))
        .fetch_optional(&mut **tx)
        .await?
        .unwrap_or_default()
    } else if legacy_public_own {
        let latest: Option<(i64, DateTime<Utc>)> = sqlx::query_as(
            "SELECT p.id,p.created_at FROM post_secrets.op_replies r JOIN content.posts p ON p.id=r.post_id WHERE r.thread_id=$1 AND p.board=$2 AND p.thread_id=$1 AND p.id<>$1 AND NOT p.deleted ORDER BY p.id DESC LIMIT 1",
        )
        .bind(thread)
        .bind(board)
        .fetch_optional(&mut **tx)
        .await?;
        Evidence {
            own_reply: true,
            latest_post_id: latest.map(|(id, _)| id),
            latest_created_at: latest.map(|(_, time)| time),
        }
    } else {
        Evidence::default()
    };
    merge(fresh, legacy)
}

fn merge(fresh: Evidence, legacy: Evidence) -> Result<(bool, Option<DateTime<Utc>>), StoreError> {
    let own = fresh.own_reply || legacy.own_reply;
    let latest = fresh
        .candidate()?
        .into_iter()
        .chain(legacy.candidate()?)
        .max_by_key(|(id, _)| *id)
        .map(|(_, time)| time);
    Ok((own, latest))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn evidence(own_reply: bool, latest: Option<(i64, i64)>) -> Evidence {
        Evidence {
            own_reply,
            latest_post_id: latest.map(|(id, _)| id),
            latest_created_at: latest.map(|(_, time)| DateTime::from_timestamp(time, 0).unwrap()),
        }
    }

    #[test]
    fn highest_id_wins_even_when_its_content_time_is_older() {
        let (own, latest) = merge(
            evidence(true, Some((2, 500))),
            evidence(true, Some((3, 100))),
        )
        .unwrap();
        assert!(own);
        assert_eq!(latest.unwrap().timestamp(), 100);
    }

    #[test]
    fn legacy_ownership_combines_with_current_key_reply_after_rotation() {
        let (own, latest) = merge(evidence(false, Some((3, 100))), evidence(true, None)).unwrap();
        assert!(own);
        assert_eq!(latest.unwrap().timestamp(), 100);
        let (own, latest) = merge(evidence(true, None), evidence(false, None)).unwrap();
        assert!(own);
        assert!(latest.is_none());
    }

    #[test]
    fn malformed_candidates_fail_closed_from_either_source() {
        for (id, time) in [
            (Some(1), None),
            (None, Some(100)),
            (Some(0), Some(100)),
            (Some(-1), Some(100)),
        ] {
            for malformed_fresh in [true, false] {
                let malformed = Evidence {
                    own_reply: false,
                    latest_post_id: id,
                    latest_created_at: time
                        .map(|value| DateTime::from_timestamp(value, 0).unwrap()),
                };
                let result = if malformed_fresh {
                    merge(malformed, evidence(true, Some((2, 200))))
                } else {
                    merge(evidence(true, Some((2, 200))), malformed)
                };
                assert!(matches!(
                    result,
                    Err(StoreError::Database(sqlx::Error::Protocol(_)))
                ));
            }
        }
    }

    #[test]
    fn unknown_evidence_is_not_an_error_or_ownership() {
        let (own, latest) = merge(Evidence::default(), Evidence::default()).unwrap();
        assert!(!own);
        assert!(latest.is_none());
    }

    #[test]
    fn a_reply_alone_never_proves_op_ownership() {
        assert!(
            !merge(evidence(false, Some((3, 100))), Evidence::default())
                .unwrap()
                .0
        );
    }
}
