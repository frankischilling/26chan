use chrono::{DateTime, Duration, NaiveDate, Utc};
use sqlx::error::BoxDynError;
use sqlx::postgres::{PgTypeInfo, PgValueFormat, PgValueRef};
use sqlx::{Decode, Postgres, Type, ValueRef};

/// A missing finite deadline means active or unlimited, distinguished by
/// Thread::archived_at. PostgreSQL infinity must never enter Chrono arithmetic.
pub(crate) struct ArchiveDeadline(Option<DateTime<Utc>>);

impl Type<Postgres> for ArchiveDeadline {
    fn type_info() -> PgTypeInfo {
        <DateTime<Utc> as Type<Postgres>>::type_info()
    }
}

impl<'r> Decode<'r, Postgres> for ArchiveDeadline {
    fn decode(value: PgValueRef<'r>) -> Result<Self, BoxDynError> {
        if value.is_null() {
            return Ok(Self(None));
        }
        match value.format() {
            PgValueFormat::Binary => {
                let micros = <i64 as Decode<Postgres>>::decode(value)?;
                Ok(Self(decode_micros(micros)?))
            }
            PgValueFormat::Text if value.as_str()? == "infinity" => Ok(Self(None)),
            PgValueFormat::Text => Ok(Self(Some(<DateTime<Utc> as Decode<Postgres>>::decode(
                value,
            )?))),
        }
    }
}

fn decode_micros(micros: i64) -> Result<Option<DateTime<Utc>>, BoxDynError> {
    if micros == i64::MAX {
        return Ok(None);
    }
    let epoch = NaiveDate::from_ymd_opt(2000, 1, 1)
        .expect("valid PostgreSQL epoch")
        .and_hms_opt(0, 0, 0)
        .expect("valid midnight");
    let finite = epoch
        .checked_add_signed(Duration::microseconds(micros))
        .ok_or("Archive deadline is outside the supported timestamp range.")?;
    Ok(Some(finite.and_utc()))
}

impl From<ArchiveDeadline> for Option<DateTime<Utc>> {
    fn from(value: ArchiveDeadline) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use super::decode_micros;

    #[test]
    fn finite_unlimited_and_out_of_range_deadlines_are_distinct() {
        assert_eq!(decode_micros(0).unwrap().unwrap().timestamp(), 946684800);
        assert!(decode_micros(i64::MAX).unwrap().is_none());
        assert!(decode_micros(i64::MIN).is_err());
        assert!(decode_micros(i64::MAX - 1).is_err());
    }
}
