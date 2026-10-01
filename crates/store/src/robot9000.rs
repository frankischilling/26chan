use crate::StoreError;
use board_domain::robot9000::{Prepared, low_signal_reason, pretty_duration, violation_message};
use chrono::{DateTime, Utc};
use sqlx::{Postgres, Transaction};

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Decision {
    Allow,
    Reject(String),
}

#[derive(sqlx::FromRow)]
struct ResultRow {
    kind: String,
    power: i16,
    seconds: i64,
    until_time: Option<DateTime<Utc>>,
}

pub(crate) async fn check(
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
