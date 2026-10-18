use crate::StoreError;
use board_domain::robot9000::{Prepared, low_signal_reason, pretty_duration, violation_message};
use chrono::{DateTime, Utc};
use sqlx::{Postgres, Transaction};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Decision {
    Allow,
    Reject(String),
}

/// An evaluated check keeps its exact inputs for replay after the caller rolls
/// back provisional content. The caller must retain the board lock throughout.
/// In particular, replay must not observe a later clock or re-normalized text.
pub(crate) struct Checked {
    board: String,
    actor: [u8; 32],
    prepared: Prepared,
    now: DateTime<Utc>,
    decision: Decision,
}
impl Checked {
    pub(crate) async fn evaluate(
        tx: &mut Transaction<'_, Postgres>,
        board: &str,
        actor: [u8; 32],
        prepared: Prepared,
        now: DateTime<Utc>,
    ) -> Result<Self, StoreError> {
        let decision = check(tx, board, &actor, &prepared, now).await?;
        Ok(Self {
            board: board.into(),
            actor,
            prepared,
            now,
            decision,
        })
    }

    pub(crate) fn decision(&self) -> &Decision {
        &self.decision
    }

    /// Call only after rolling back the savepoint preceding this check. Both
    /// allowed originality state and rejected mute state need this replay when
    /// their provisional post is discarded. A changed decision returns an error;
    /// the caller must discard the transaction.
    pub(crate) async fn replay(
        &self,
        tx: &mut Transaction<'_, Postgres>,
    ) -> Result<(), StoreError> {
        if check(tx, &self.board, &self.actor, &self.prepared, self.now).await? != self.decision {
            return Err(StoreError::Database(sqlx::Error::Protocol(
                "Robot9000 decision changed under the board lock.".into(),
            )));
        }
        Ok(())
    }
}

#[derive(sqlx::FromRow)]
struct ResultRow {
    kind: String,
    power: i16,
    seconds: i64,
    until_time: Option<DateTime<Utc>>,
}

async fn check(
    tx: &mut Transaction<'_, Postgres>,
    board: &str,
    actor: &[u8; 32],
    prepared: &Prepared,
    now: DateTime<Utc>,
) -> Result<Decision, StoreError> {
    let row: ResultRow = sqlx::query_as(
        "SELECT kind,power,seconds,until_time FROM content.check_robot9000($1,$2,$3,$4,$5)",
    )
    .bind(board)
    .bind(actor.as_slice())
    .bind(prepared.digest.as_slice())
    .bind(prepared.low_signal_percent)
    .bind(now)
    .fetch_one(&mut **tx)
    .await?;
    if row.kind == "allow" && row.power == 0 && row.seconds == 0 && row.until_time.is_none() {
        return Ok(Decision::Allow);
    }
    let invalid =
        || StoreError::Database(sqlx::Error::Protocol("Invalid Robot9000 result.".into()));
    if !(0..=24).contains(&row.power)
        || !(1..=board_domain::robot9000::MAX_MUTE_SECONDS).contains(&row.seconds)
        || row.until_time != now.checked_add_signed(chrono::Duration::seconds(row.seconds))
    {
        return Err(invalid());
    }
    let reason = match row.kind.as_str() {
        "muted" => {
            let local = row
                .until_time
                .ok_or_else(invalid)?
                .with_timezone(&chrono_tz::America::New_York);
            let when = local.format("%m/%d/%y %H:%M:%S");
            return Ok(Decision::Reject(format!(
                "You're muted! You cannot post until {when}, {} from now",
                pretty_duration(row.seconds)
            )));
        }
        "duplicate" => board_domain::robot9000::DUPLICATE_TEXT.to_owned(),
        "low_signal" => low_signal_reason(prepared.low_signal_percent.ok_or_else(invalid)?),
        _ => return Err(invalid()),
    };
    Ok(Decision::Reject(violation_message(row.seconds, &reason)))
}

#[cfg(all(test, feature = "database-tests"))]
mod tests;
