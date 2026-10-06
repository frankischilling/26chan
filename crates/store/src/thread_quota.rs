//! IP-only active OP quota. Private actor evidence never leaves the SQL API.
use crate::StoreError;
use board_domain::poster_id::PublicPostingRateIdentity;
use sqlx::{Postgres, Transaction};

#[derive(sqlx::FromRow)]
struct Decision {
    rejected: bool,
    user_thread_limit: i32,
    user_thread_period_hours: i32,
}

/// Call for every OP, including badged staff on private boards, after trusted
/// authorization and actor/global OP/board locks, and before any rollover.
/// The supplied actor is transport-derived; neither passwords nor Pass values
/// are accepted by this bounded slice. Replies must not call this function.
pub(crate) async fn check(
    tx: &mut Transaction<'_, Postgres>,
    actor: &PublicPostingRateIdentity,
    board: &str,
    request_epoch: i64,
) -> Result<(), StoreError> {
    let rows: Vec<Decision> = sqlx::query_as(
        "SELECT rejected,user_thread_limit,user_thread_period_hours FROM content.check_user_thread_quota($1,$2,$3) LIMIT 2",
    )
    .bind(actor.as_bytes().as_slice())
    .bind(board)
    .bind(request_epoch)
    .fetch_all(&mut **tx)
    .await?;
    decode_result(&rows)
}

fn decode_result(rows: &[Decision]) -> Result<(), StoreError> {
    let row = match rows {
        [row]
            if (0..=100000).contains(&row.user_thread_limit)
                && (0..=876000).contains(&row.user_thread_period_hours)
                && (row.user_thread_limit != 0 || row.rejected) =>
        {
            row
        }
        _ => {
            return Err(StoreError::Database(sqlx::Error::Protocol(
                "Invalid thread quota result.".into(),
            )));
        }
    };
    if row.rejected {
        return Err(StoreError::ContentRejected(source_message(
            row.user_thread_limit,
        )));
    }
    Ok(())
}

// global_strings.ini S_TOOMANYTHREADS, including singular wording at zero.
fn source_message(limit: i32) -> String {
    format!(
        "Error: You may not post more than {limit} active thread{} at a time.",
        if limit > 1 { "s" } else { "" }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decision(rejected: bool, limit: i32, hours: i32) -> Decision {
        Decision {
            rejected,
            user_thread_limit: limit,
            user_thread_period_hours: hours,
        }
    }

    #[test]
    fn valid_decisions_preserve_zero_and_maximum_policy_values() {
        assert!(decode_result(&[decision(false, 5, 24)]).is_ok());
        assert!(decode_result(&[decision(false, 100000, 876000)]).is_ok());
        assert!(decode_result(&[decision(false, 1, 0)]).is_ok());
        for limit in [0, 1, 3, 5, 50, 100000] {
            match decode_result(&[decision(true, limit, 24)]) {
                Err(StoreError::ContentRejected(message)) => {
                    assert_eq!(message, source_message(limit));
                }
                _ => panic!("expected source thread quota rejection"),
            }
        }
    }

    #[test]
    fn malformed_or_missing_decisions_fail_closed_without_a_quota_error() {
        for rows in [
            vec![],
            vec![decision(false, 5, 24), decision(false, 5, 24)],
            vec![decision(false, 0, 24)],
            vec![decision(true, -1, 24)],
            vec![decision(true, 100001, 24)],
            vec![decision(true, 5, -1)],
            vec![decision(true, 5, 876001)],
        ] {
            assert!(matches!(
                decode_result(&rows),
                Err(StoreError::Database(sqlx::Error::Protocol(_)))
            ));
        }
    }

    #[test]
    fn source_wording_has_exact_zero_singular_and_plural_forms() {
        assert_eq!(
            source_message(0),
            "Error: You may not post more than 0 active thread at a time."
        );
        assert_eq!(
            source_message(1),
            "Error: You may not post more than 1 active thread at a time."
        );
        assert_eq!(
            source_message(5),
            "Error: You may not post more than 5 active threads at a time."
        );
    }
}
