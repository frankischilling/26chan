//! Advisory optional-exemption state. Final admission always checks it again.
use crate::{StoreError, anonymous_session::PostingSession};
use board_domain::poster_id::PublicReportRateIdentity;
use sqlx::PgPool;

/// Does not record a report or advance browser activity. It may establish
/// identity continuity or prune expired private history; it reserves no bypass.
pub async fn advice(
    pool: &PgPool,
    board: &str,
    post: i64,
    identity: &PublicReportRateIdentity,
    session: PostingSession,
    source_cookies_absent: bool,
) -> Result<bool, StoreError> {
    if session.minted {
        return Ok(false);
    }
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL READ COMMITTED")
        .execute(&mut *tx)
        .await?;
    crate::write_challenge::install_report_epoch(&mut tx, identity).await?;
    crate::write_challenge::install_report_exemption(&mut tx, identity, source_cookies_absent)
        .await?;
    crate::restrictions::prepare_report(
        &mut tx,
        identity,
        Some(&session.fingerprints.token),
        board,
        post,
        session.now.timestamp(),
    )
    .await?;
    let f = session.fingerprints;
    let eligible =
        sqlx::query_scalar("SELECT content.report_challenge_advice($1,$2,$3,$4,$5,$6,$7,$8)")
            .bind(board)
            .bind(post)
            .bind(identity.as_bytes().as_slice())
            .bind(f.token.as_slice())
            .bind(f.network.as_slice())
            .bind(f.address.as_slice())
            .bind(f.environment.as_slice())
            .bind(session.now.timestamp())
            .fetch_one(&mut *tx)
            .await?;
    tx.commit().await?;
    Ok(eligible)
}
