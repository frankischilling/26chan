//! Private late-posting history bridges. The application must select these
//! after the ordered source checks; this module grants no posting bypass.
use crate::{StoreError, anonymous_session::PostingSession};
use sqlx::{PgConnection, PgPool, Postgres, Transaction};

#[derive(sqlx::FromRow)]
pub struct History {
    pub edge_blocked: bool,
    pub country_blocked: bool,
}
fn mapped(error: sqlx::Error) -> StoreError {
    if error
        .as_database_error()
        .and_then(|error| error.code())
        .as_deref()
        == Some("28000")
    {
        StoreError::AuthorizationChanged
    } else {
        StoreError::Database(error)
    }
}

/// Read under the caller's board/session transaction locks. An unavailable
/// query is an error, never an empty history or permission to post.
pub async fn history(
    connection: &mut PgConnection,
    board: &str,
    session: &PostingSession,
) -> Result<History, StoreError> {
    sqlx::query_as("SELECT * FROM content.posting_classifier_history($1,$2,$3)")
        .bind(board)
        .bind(session.fingerprints.token.as_slice())
        .bind(session.minted)
        .fetch_one(connection)
        .await
        .map_err(mapped)
}

/// Commit only the independently revalidated country refusal and session
/// refresh. Roll back any provisional posting transaction before calling;
/// otherwise its board/session locks would obstruct this fresh transaction.
/// Only a successful return permits the handler to refresh the session cookie.
pub async fn record_country_refusal(
    pool: &PgPool,
    board: &str,
    session: &PostingSession,
) -> Result<(), StoreError> {
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
        .execute(&mut *tx)
        .await?;
    record_country_refusal_in_transaction(&mut tx, board, session).await?;
    tx.commit().await?;
    Ok(())
}

/// The posting writer may retain its locked transaction after rolling back all
/// provisional content and replaying earlier state-only effects. This helper
/// revalidates the country refusal in that transaction without committing it.
/// Any failure must discard the transaction; only the final commit authorizes
/// a refreshed refusal cookie.
pub(crate) async fn record_country_refusal_in_transaction(
    tx: &mut Transaction<'_, Postgres>,
    board: &str,
    session: &PostingSession,
) -> Result<(), StoreError> {
    let fingerprints = session.fingerprints;
    let authorized: bool = sqlx::query_scalar(
        "SELECT content.record_country_classifier_refusal($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(board)
    .bind(fingerprints.token.as_slice())
    .bind(fingerprints.network.as_slice())
    .bind(fingerprints.address.as_slice())
    .bind(fingerprints.environment.as_slice())
    .bind(session.minted)
    .bind(session.now.timestamp())
    .fetch_one(&mut **tx)
    .await
    .map_err(mapped)?;
    if !authorized {
        return Err(StoreError::AuthorizationChanged);
    }
    Ok(())
}
