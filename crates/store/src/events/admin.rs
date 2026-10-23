//! Offline-validated, migrator-only event configuration. No runtime credential
//! can activate/reset events or provide market quotes. Closing retains history.
use board_domain::source_events::{STOCKS, credentials::Kind};
use serde::{Deserialize, de};
use sqlx::{Connection, PgConnection, Postgres, Transaction};
use std::{collections::BTreeMap, io::Read};

const MAX_INPUT: u64 = 16384;
const MAX_TIMESTAMP: i64 = 253402300799;
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Expected bounded version-1 event JSON with valid times, capacity and quotes.")]
    Input,
    #[error("Event maintenance requires the actual board_migrator login.")]
    Role,
    #[error("Event authority has drifted; configuration was not changed.")]
    Authority,
    #[error("An event of this kind is already enabled. Close it before starting a new run.")]
    Active,
    #[error("The event window has already ended.")]
    Expired,
    #[error("No matching enabled market run exists.")]
    Missing,
    #[error("The retained event-run budget is full.")]
    Full,
    #[error("Event database operation failed; credentials and server details are not displayed.")]
    Database,
}
impl From<sqlx::Error> for Error {
    fn from(_: sqlx::Error) -> Self {
        Self::Database
    }
}
#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum InputKind {
    Market,
    Emotes,
}
impl InputKind {
    fn kind(self) -> Kind {
        match self {
            Self::Market => Kind::Market,
            Self::Emotes => Kind::Emotes,
        }
    }
}
#[derive(Debug, Default)]
struct Prices(BTreeMap<String, i64>);
impl<'de> Deserialize<'de> for Prices {
    fn deserialize<D: de::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> de::Visitor<'de> for Visitor {
            type Value = Prices;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("one positive integer quote for each source symbol")
            }
            fn visit_map<A: de::MapAccess<'de>>(self, mut map: A) -> Result<Prices, A::Error> {
                let mut values = BTreeMap::new();
                while let Some((name, price)) = map.next_entry::<String, i64>()? {
                    if values.len() >= 18
                        || !STOCKS.contains(&name.as_str())
                        || price <= 0
                        || values.insert(name, price).is_some()
                    {
                        return Err(de::Error::custom("Invalid or duplicate event quote"));
                    }
                }
                Ok(Prices(values))
            }
        }
        deserializer.deserialize_map(Visitor)
    }
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    version: u8,
    kind: InputKind,
    starts_at: i64,
    ends_at: i64,
    account_capacity: i32,
    #[serde(default)]
    prices: Prices,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuoteInput {
    version: u8,
    run_id: i64,
    prices: Prices,
}
impl Profile {
    fn validate(&self) -> Result<(), Error> {
        if self.version != 1
            || !(0..MAX_TIMESTAMP).contains(&self.starts_at)
            || self.ends_at <= self.starts_at
            || self.ends_at > MAX_TIMESTAMP
            || !(1..=100000).contains(&self.account_capacity)
        {
            return Err(Error::Input);
        }
        match self.kind {
            InputKind::Market if self.prices.0.len() != 18 => Err(Error::Input),
            InputKind::Emotes if !self.prices.0.is_empty() => Err(Error::Input),
            _ => Ok(()),
        }
    }
}
impl QuoteInput {
    fn validate(&self) -> Result<(), Error> {
        if self.version != 1 || self.run_id <= 0 || self.prices.0.len() != 18 {
            Err(Error::Input)
        } else {
            Ok(())
        }
    }
}
fn read<T: de::DeserializeOwned>(reader: impl Read) -> Result<T, Error> {
    let mut bytes = Vec::new();
    reader
        .take(MAX_INPUT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Input)?;
    if bytes.len() as u64 > MAX_INPUT {
        return Err(Error::Input);
    }
    serde_json::from_slice(&bytes).map_err(|_| Error::Input)
}
pub fn read_profile(reader: impl Read) -> Result<Profile, Error> {
    let value: Profile = read(reader)?;
    value.validate()?;
    Ok(value)
}
pub fn read_quotes(reader: impl Read) -> Result<QuoteInput, Error> {
    let value: QuoteInput = read(reader)?;
    value.validate()?;
    Ok(value)
}

async fn transaction(connection: &mut PgConnection) -> Result<Transaction<'_, Postgres>, Error> {
    let mut tx = connection.begin().await?;
    let allowed: bool = sqlx::query_scalar(
        "SELECT current_user='board_migrator' AND session_user='board_migrator'",
    )
    .fetch_one(&mut *tx)
    .await?;
    if !allowed {
        tx.rollback().await?;
        return Err(Error::Role);
    }
    sqlx::query("SET LOCAL lock_timeout='5s'")
        .execute(&mut *tx)
        .await?;
    sqlx::query("SET LOCAL statement_timeout='30s'")
        .execute(&mut *tx)
        .await?;
    sqlx::query("SET LOCAL ROLE board_event_owner")
        .execute(&mut *tx)
        .await?;
    let ready: bool = sqlx::query_scalar("SELECT content.events_ready()")
        .fetch_one(&mut *tx)
        .await?;
    sqlx::query("RESET ROLE").execute(&mut *tx).await?;
    if !ready {
        tx.rollback().await?;
        return Err(Error::Authority);
    }
    sqlx::query("LOCK TABLE event_private.runs IN SHARE ROW EXCLUSIVE MODE")
        .execute(&mut *tx)
        .await?;
    Ok(tx)
}
async fn save_quotes(
    tx: &mut Transaction<'_, Postgres>,
    id: i64,
    prices: &Prices,
) -> Result<(), Error> {
    for (stock, name) in STOCKS.into_iter().enumerate() {
        sqlx::query("INSERT INTO event_private.quotes(run_id,stock,price) VALUES($1,$2,$3) ON CONFLICT(run_id,stock) DO UPDATE SET price=EXCLUDED.price")
   .bind(id).bind(stock as i16).bind(prices.0[name]).execute(&mut **tx).await?;
    }
    Ok(())
}
/// Start a new immutable epoch. An active or scheduled run must be explicitly
/// closed first. Expired runs are disabled, but their accounts are retained.
pub async fn start(connection: &mut PgConnection, profile: &Profile) -> Result<i64, Error> {
    profile.validate()?;
    let mut tx = transaction(connection).await?;
    let now: i64 =
        sqlx::query_scalar("SELECT floor(extract(epoch FROM clock_timestamp()))::bigint")
            .fetch_one(&mut *tx)
            .await?;
    if profile.ends_at <= now {
        tx.rollback().await?;
        return Err(Error::Expired);
    }
    let kind = profile.kind.kind() as i16;
    sqlx::query(
        "UPDATE event_private.runs SET enabled=false WHERE kind=$1 AND ends_at<=$2 AND enabled",
    )
    .bind(kind)
    .bind(now)
    .execute(&mut *tx)
    .await?;
    let active: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM event_private.runs WHERE kind=$1 AND enabled)",
    )
    .bind(kind)
    .fetch_one(&mut *tx)
    .await?;
    if active {
        tx.rollback().await?;
        return Err(Error::Active);
    }
    let (last, count): (i64, i64) =
        sqlx::query_as("SELECT coalesce(max(id),0),count(*) FROM event_private.runs")
            .fetch_one(&mut *tx)
            .await?;
    let Some(id) = last.checked_add(1).filter(|_| count < 10000) else {
        tx.rollback().await?;
        return Err(Error::Full);
    };
    sqlx::query("INSERT INTO event_private.runs(id,kind,enabled,starts_at,ends_at,account_capacity) VALUES($1,$2,true,$3,$4,$5)")
  .bind(id).bind(kind).bind(profile.starts_at).bind(profile.ends_at).bind(profile.account_capacity).execute(&mut *tx).await?;
    if matches!(profile.kind, InputKind::Market) {
        save_quotes(&mut tx, id, &profile.prices).await?;
    }
    tx.commit().await?;
    Ok(id)
}
/// A quote update locks the run before prices, matching the trade lock order.
pub async fn quote(connection: &mut PgConnection, input: &QuoteInput) -> Result<(), Error> {
    input.validate()?;
    let mut tx = transaction(connection).await?;
    let found:Option<i64>=sqlx::query_scalar("SELECT id FROM event_private.runs WHERE id=$1 AND kind=1 AND enabled AND ends_at>extract(epoch FROM clock_timestamp()) FOR UPDATE")
  .bind(input.run_id).fetch_optional(&mut *tx).await?;
    if found.is_none() {
        tx.rollback().await?;
        return Err(Error::Missing);
    }
    save_quotes(&mut tx, input.run_id, &input.prices).await?;
    tx.commit().await?;
    Ok(())
}
pub async fn close(connection: &mut PgConnection, id: i64) -> Result<bool, Error> {
    if id <= 0 {
        return Err(Error::Input);
    }
    let mut tx = transaction(connection).await?;
    let changed =
        sqlx::query("UPDATE event_private.runs SET enabled=false WHERE id=$1 AND enabled")
            .bind(id)
            .execute(&mut *tx)
            .await?
            .rows_affected()
            == 1;
    tx.commit().await?;
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn profile(kind: &str) -> serde_json::Value {
        serde_json::json!({"version":1,"kind":kind,"starts_at":100,"ends_at":200,"account_capacity":10})
    }
    #[test]
    fn profiles_have_bounded_strict_operator_fields() {
        let valid = profile("emotes");
        assert!(read_profile(valid.to_string().as_bytes()).is_ok());
        for (key, value) in [
            ("version", serde_json::json!(2)),
            ("kind", serde_json::json!("unknown")),
            ("ends_at", serde_json::json!(100)),
            ("account_capacity", serde_json::json!(100001)),
            ("enabled", serde_json::json!(true)),
        ] {
            let mut bad = valid.clone();
            bad[key] = value;
            assert!(read_profile(bad.to_string().as_bytes()).is_err());
        }
        assert!(read_profile(&b" "[0..].repeat(16385)[..]).is_err());
        assert!(
            read_profile(b"{\"version\":1,\"version\":1,\"kind\":\"emotes\"}".as_slice()).is_err()
        );
    }
    #[test]
    fn market_quotes_require_every_source_symbol_and_reject_duplicates() {
        let mut valid = profile("market");
        let prices: BTreeMap<_, _> = STOCKS.into_iter().map(|stock| (stock, 10)).collect();
        valid["prices"] = serde_json::to_value(prices).unwrap();
        assert!(read_profile(valid.to_string().as_bytes()).is_ok());
        let mut missing = valid.clone();
        missing["prices"].as_object_mut().unwrap().remove("PEPE");
        assert!(read_profile(missing.to_string().as_bytes()).is_err());
        let mut zero = valid.clone();
        zero["prices"]["PEPE"] = serde_json::json!(0);
        assert!(read_profile(zero.to_string().as_bytes()).is_err());
        let encoded = valid
            .to_string()
            .replace("\"PEPE\":10", "\"PEPE\":10,\"PEPE\":11");
        assert!(read_profile(encoded.as_bytes()).is_err());
        let quote = serde_json::json!({"version":1,"run_id":1,"prices":valid["prices"]});
        assert!(read_quotes(quote.to_string().as_bytes()).is_ok());
    }
}
