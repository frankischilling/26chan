use super::unavailable;
use crate::StoreError;
use board_domain::content_admission::network::{Leniency, Policy, Rule};
use sqlx::PgConnection;
use std::net::Ipv4Addr;

#[derive(sqlx::FromRow)]
struct Row {
    id: i64,
    range_start: Option<i64>,
    range_end: Option<i64>,
    asn: Option<i64>,
    active: bool,
    created_at: i64,
    updated_at: i64,
    expires_at: i64,
    boards: String,
    ops_only: bool,
    images_only: bool,
    reports_only: bool,
    leniency: i16,
    signatures: String,
}

/// Caller already holds the shared policy generation and locked board row.
/// The scoped definer exposes at most the bounded rule count plus one sentinel.
pub(super) struct Snapshot {
    pub policy: Option<Policy>,
}

pub(super) async fn snapshot(
    connection: &mut PgConnection,
    board: &str,
    now: i64,
    staff_bypass: bool,
) -> Result<Snapshot, StoreError> {
    if staff_bypass {
        return Ok(Snapshot { policy: None });
    }
    let rows: Vec<Row> = sqlx::query_as("SELECT * FROM content.network_admission_rules($1,$2)")
        .bind(board)
        .bind(now)
        .fetch_all(connection)
        .await?;
    let rules = rows
        .into_iter()
        .map(|row| {
            let range = match (row.range_start, row.range_end) {
                (None, None) => None,
                (Some(first), Some(last)) => Some((
                    Ipv4Addr::from(u32::try_from(first).map_err(unavailable)?),
                    Ipv4Addr::from(u32::try_from(last).map_err(unavailable)?),
                )),
                _ => return Err(unavailable(())),
            };
            Ok(Rule {
                id: row.id,
                range,
                asn: row
                    .asn
                    .map(u32::try_from)
                    .transpose()
                    .map_err(unavailable)?,
                active: row.active,
                created_at: row.created_at.try_into().map_err(unavailable)?,
                updated_at: row.updated_at.try_into().map_err(unavailable)?,
                expires_at: row.expires_at.try_into().map_err(unavailable)?,
                boards: row.boards,
                ops_only: row.ops_only,
                images_only: row.images_only,
                reports_only: row.reports_only,
                leniency: match row.leniency {
                    0 => Leniency::None,
                    1 => Leniency::KnownOrVerified,
                    2 => Leniency::Known,
                    3 => Leniency::Verified,
                    _ => return Err(unavailable(())),
                },
                signatures: row.signatures,
            })
        })
        .collect::<Result<Vec<_>, StoreError>>()?;
    Ok(Snapshot {
        policy: Some(Policy::compile(rules).map_err(unavailable)?),
    })
}
