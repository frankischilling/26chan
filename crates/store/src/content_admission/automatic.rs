//! Private accepted-post comparison metadata; it grants no posting authority.
use crate::StoreError;
use board_domain::{
    content_admission::network::{ProviderValue, hints::DerivedHints},
    poster_id::{PosterIdKey, PublicPostingRateIdentity},
};
use sqlx::PgConnection;
use std::net::IpAddr;

mod readiness;
pub use readiness::READINESS_SQL;

pub(crate) struct Capture<'a> {
    pub board: &'a str,
    pub parent: i64,
    pub actor: &'a PublicPostingRateIdentity,
    pub key: &'a PosterIdKey,
    pub peer: IpAddr,
    pub hints: Option<&'a DerivedHints>,
}

pub(crate) async fn install_metadata(
    connection: &mut PgConnection,
    capture: Capture<'_>,
) -> Result<(), StoreError> {
    let metadata = match (
        capture.key.automatic_range_prefix(capture.peer),
        capture.hints.map(|hints| hints.signatures().browser_id),
    ) {
        (Some(prefix), Some(ProviderValue::Available(Some(browser))))
            if browser.starts_with('1') =>
        {
            let hex = |value: &[u8]| -> String {
                value.iter().map(|byte| format!("{byte:02x}")).collect()
            };
            serde_json::json!({
                "board": capture.board,
                "parent": capture.parent.to_string(),
                "actor": hex(capture.actor.as_bytes()),
                "browser": browser,
                "prefix": hex(prefix.as_bytes()),
            })
            .to_string()
        }
        _ => String::new(),
    };
    // Always replace pooled-session state, including when no evidence exists.
    // The insert trigger binds the context to its new row and consumes it once.
    sqlx::query("SELECT set_config('board.automatic_range_metadata',$1,true)")
        .bind(metadata)
        .execute(connection)
        .await?;
    Ok(())
}

#[derive(sqlx::FromRow)]
struct EventRow {
    id: i64,
    board: String,
    parent: i64,
    prefix: Vec<u8>,
    browser: String,
    created_at: i64,
    observed_at: i64,
}

pub const REJECTION: &str = "Posting from your IP range has been temporarily blocked due to abuse. Please verify your e-mail address or try again later.";

/// Private match receipt, never formatted, serialized or returned to a client.
/// Its recorder rechecks the scoped live event after posting rollback.
pub(crate) struct Hit {
    id: i64,
    board: String,
    parent: i64,
    prefix: [u8; 32],
    browser: String,
}

impl Hit {
    pub(crate) async fn record(
        &self,
        pool: &sqlx::PgPool,
        session: Option<crate::anonymous_session::PostingSession>,
    ) -> Result<bool, StoreError> {
        let mut tx = pool.begin().await?;
        sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
            .execute(&mut *tx)
            .await?;
        sqlx::query("SELECT content.record_automatic_range_hit($1,$2,$3,$4,$5)")
            .bind(self.id)
            .bind(&self.board)
            .bind(self.parent)
            .bind(self.prefix.as_slice())
            .bind(&self.browser)
            .execute(&mut *tx)
            .await?;
        // Cookie storage failure must not erase the independent refusal hit.
        // A cookie is emitted only after the outer transaction commits.
        let refreshed =
            if let Some(session) = session {
                sqlx::query("SAVEPOINT automatic_range_cookie")
                    .execute(&mut *tx)
                    .await?;
                let fingerprints = session.fingerprints;
                let result = sqlx::query_scalar::<_, bool>(
                "SELECT content.refresh_automatic_range_cookie($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)"
            ).bind(self.id).bind(&self.board).bind(self.parent)
                .bind(self.prefix.as_slice()).bind(&self.browser)
                .bind(fingerprints.token.as_slice()).bind(fingerprints.network.as_slice())
                .bind(fingerprints.address.as_slice()).bind(fingerprints.environment.as_slice())
                .bind(session.minted).bind(session.now.timestamp())
                .fetch_one(&mut *tx).await;
                let refreshed = match result {
                    Ok(value) => value,
                    Err(_) => {
                        sqlx::query("ROLLBACK TO SAVEPOINT automatic_range_cookie")
                            .execute(&mut *tx)
                            .await?;
                        false
                    }
                };
                sqlx::query("RELEASE SAVEPOINT automatic_range_cookie")
                    .execute(&mut *tx)
                    .await?;
                refreshed
            } else {
                false
            };
        tx.commit().await?;
        Ok(refreshed)
    }
}

impl super::Evaluation {
    pub(crate) async fn automatic_rejection(
        &self,
        connection: &mut PgConnection,
        key: &PosterIdKey,
    ) -> Result<Option<Hit>, StoreError> {
        use board_domain::content_admission::network::automatic::{Event, Policy, Request};
        if self.snapshot.network.policy.is_none() {
            return Ok(None);
        }
        let peer = self
            .snapshot
            .peer
            .as_deref()
            .ok_or_else(|| super::unavailable(()))?
            .parse::<IpAddr>()
            .map_err(super::unavailable)?;
        let Some(prefix) = key.automatic_range_prefix(peer) else {
            return Ok(None);
        };
        let browser = self.snapshot.hints.signatures().browser_id;
        let selector = match browser {
            ProviderValue::Available(Some(browser)) if browser.starts_with('1') => Some(browser),
            ProviderValue::Available(_) => return Ok(None),
            ProviderValue::Unavailable => None,
        };
        // The posting transaction already owns its shared policy generation.
        // The reader adds no board/actor lock, and returns one overflow sentinel.
        let rows: Vec<EventRow> =
            sqlx::query_as("SELECT * FROM content.automatic_range_events($1,$2,$3,$4)")
                .bind(&self.input.board)
                .bind(self.input.parent)
                .bind(prefix.as_bytes().as_slice())
                .bind(selector)
                .fetch_all(connection)
                .await?;
        let event_at = rows
            .first()
            .map(|row| row.observed_at)
            .map(u64::try_from)
            .transpose()
            .map_err(super::unavailable)?
            .unwrap_or(self.snapshot.now);
        let events = rows
            .into_iter()
            .map(|row| {
                Ok(Event {
                    id: row.id,
                    board: row.board,
                    parent: row.parent,
                    prefix: row.prefix.try_into().map_err(super::unavailable)?,
                    browser: row.browser,
                    created_at: row.created_at.try_into().map_err(super::unavailable)?,
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        let policy = Policy::compile(events).map_err(super::unavailable)?;
        let matched = policy
            .evaluate(Request {
                board: &self.input.board,
                parent: Some(self.input.parent),
                prefix: ProviderValue::Available(Some(prefix.as_bytes())),
                browser,
                request_at: self.snapshot.now,
                activity_at: self.snapshot.activity_now,
                event_at,
                actor: self.snapshot.activity.as_ref(),
            })
            .map_err(super::unavailable)?;
        Ok(matched.map(|event| Hit {
            id: event.id,
            board: event.board.clone(),
            parent: event.parent,
            prefix: event.prefix,
            browser: event.browser.clone(),
        }))
    }
}
