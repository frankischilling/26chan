//! Issuance runs under operator authority. The public service only sees digests.
use super::{Error, verified_transaction};
use board_domain::site_submissions::{
    self,
    ownership::{Capability, Kind},
};
use sqlx::PgConnection;
use std::io::Read;

const MAX_TERMS: usize = 16_000;

pub fn read_terms(reader: impl Read) -> Result<String, Error> {
    let mut text = String::new();
    reader
        .take((MAX_TERMS * 4 + 1) as u64)
        .read_to_string(&mut text)
        .map_err(|_| Error::AgreementInput)?;
    site_submissions::text(&text, MAX_TERMS, true, true).map_err(|_| Error::AgreementInput)
}

pub async fn issue(
    connection: &mut PgConnection,
    application: i64,
    terms: &str,
) -> Result<Capability, Error> {
    let terms =
        site_submissions::text(terms, MAX_TERMS, true, true).map_err(|_| Error::AgreementInput)?;
    let invite =
        Capability::generate(Kind::ApplicationAgreement).map_err(|_| Error::AgreementRandomness)?;
    let mut tx = verified_transaction(connection).await?;
    let revision: Option<i32> = sqlx::query_scalar(
        "SELECT revision FROM submission_private.applications WHERE id=$1 AND state=2 FOR UPDATE",
    )
    .bind(application)
    .fetch_optional(&mut *tx)
    .await?;
    let revision = revision.ok_or(Error::AgreementUnavailable)?;
    let stored = sqlx::query(
        "WITH timing AS (SELECT clock_timestamp() AS issued)
         INSERT INTO submission_private.application_agreements
            (application_id, revision, invite_hash, terms, issued_at, expires_at)
         SELECT $1, $2, $3, $4, issued, issued + interval '168 hours' FROM timing
         ON CONFLICT (application_id) DO UPDATE
         SET revision=excluded.revision, invite_hash=excluded.invite_hash,
             terms=excluded.terms, issued_at=excluded.issued_at, expires_at=excluded.expires_at
         WHERE application_agreements.accepted_at IS NULL",
    )
    .bind(application)
    .bind(revision)
    .bind(invite.storage_hash().as_slice())
    .bind(terms)
    .execute(&mut *tx)
    .await?;
    if stored.rows_affected() != 1 {
        return Err(Error::AgreementUnavailable);
    }
    tx.commit().await?;
    Ok(invite)
}

pub async fn revoke(connection: &mut PgConnection, application: i64) -> Result<bool, Error> {
    let mut tx = verified_transaction(connection).await?;
    sqlx::query("SELECT id FROM submission_private.applications WHERE id=$1 FOR UPDATE")
        .bind(application)
        .execute(&mut *tx)
        .await?;
    let removed = sqlx::query(
        "DELETE FROM submission_private.application_agreements
         WHERE application_id=$1 AND accepted_at IS NULL",
    )
    .bind(application)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(removed.rows_affected() == 1)
}
