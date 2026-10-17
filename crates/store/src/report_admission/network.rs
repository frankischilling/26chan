//! Request-bound comparison data for the atomic SQL report boundary.
use crate::{ProtectedReportingContext, StoreError};
use board_domain::content_admission::network::{ProviderValue, asn::Database, hints::DerivedHints};
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use sqlx::PgConnection;
use std::net::IpAddr;

pub struct Context<'a> {
    pub request_facts: Option<&'a super::request::RequestFacts>,
    pub protected: ProtectedReportingContext,
    pub request_start: DateTime<Utc>,
    pub peer: Option<IpAddr>,
    pub asn_database: Option<&'a Database>,
    pub hints: Option<&'a DerivedHints>,
}

impl Context<'_> {
    pub(crate) fn unavailable(protected: ProtectedReportingContext) -> Self {
        Self {
            request_facts: None,
            request_start: protected.session.now,
            protected,
            peer: None,
            asn_database: None,
            hints: None,
        }
    }

    pub(crate) fn prepare(
        &self,
        board: &str,
        post: i64,
        actor: &board_domain::poster_id::PublicReportRateIdentity,
    ) -> Result<Value, StoreError> {
        Source {
            request_start: self.request_start,
            peer: self.peer,
            asn_database: self.asn_database,
            hints: self.hints,
        }
        .prepare(board, post, actor)
    }
}

/// Trusted comparison inputs for the staff-only legacy reporting API.
pub struct Source<'a> {
    pub request_start: DateTime<Utc>,
    pub peer: Option<IpAddr>,
    pub asn_database: Option<&'a Database>,
    pub hints: Option<&'a DerivedHints>,
}

impl Source<'_> {
    pub(crate) fn unavailable() -> Self {
        Self {
            request_start: Utc::now(),
            peer: None,
            asn_database: None,
            hints: None,
        }
    }
    /// Resolve the same canonical peer before opening the transaction. Provider
    /// failure stays explicit; only applicable SQL rules can reject for it.
    pub(crate) fn prepare(
        &self,
        board: &str,
        post: i64,
        actor: &board_domain::poster_id::PublicReportRateIdentity,
    ) -> Result<Value, StoreError> {
        let now = u64::try_from(self.request_start.timestamp())
            .map_err(|_| StoreError::Invalid("Invalid report request time."))?;
        let peer = self.peer.map(|peer| peer.to_canonical());
        let ipv4 = match peer {
            Some(IpAddr::V4(address)) => Some(u32::from(address)),
            _ => None,
        };
        let (asn_known, asn) = match (self.asn_database, peer) {
            (Some(database), Some(peer)) => match database.lookup(peer) {
                Ok(value) => (true, value),
                Err(_) => (false, None),
            },
            _ => (false, None),
        };
        let hints = self.hints.map(DerivedHints::signatures);
        let hint = |value| match value {
            Some(ProviderValue::Available(value)) => value,
            _ => None,
        };
        let mut actor_hex = String::with_capacity(64);
        for byte in actor.as_bytes() {
            use std::fmt::Write;
            write!(actor_hex, "{byte:02x}").expect("String writes do not fail");
        }
        Ok(json!({
            "board": board, "post": post, "actor": actor_hex,
            "request_at": now, "peer_known": peer.is_some(), "ipv4": ipv4,
            "asn_known": asn_known, "asn": asn,
            "browser": hint(hints.map(|hints| hints.browser_id)),
            "request": hint(hints.map(|hints| hints.request_signature)),
        }))
    }
}

pub(crate) async fn install(
    connection: &mut PgConnection,
    context: &Value,
) -> Result<(), StoreError> {
    sqlx::query("SELECT set_config('board.report_network_context',$1,true)")
        .bind(context.to_string())
        .execute(connection)
        .await?;
    Ok(())
}
