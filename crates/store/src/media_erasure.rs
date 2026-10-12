//! Receipt-bound cleanup of descriptive media metadata on already-erased posts.
//! Attachment identities remain consumed. This module has no runtime caller.
use crate::historical_erasure::{ErasureError, preflight};
use board_domain::BoardSlug;
use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{Connection, PgConnection};

#[derive(Debug, Serialize)]
pub struct Preview {
    pub database: String,
    pub board: String,
    pub post_ids: Vec<i64>,
    pub receipt: String,
    pub applied: bool,
}

pub async fn cleanup(
    connection: &mut PgConnection,
    board: &str,
    receipt: Option<&str>,
) -> Result<Preview, ErasureError> {
    BoardSlug::parse(board).map_err(|_| ErasureError::Input)?;
    if receipt.is_some_and(|r| {
        r.len() != 64
            || !r
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }) {
        return Err(ErasureError::Input);
    }
    let mut tx = connection
        .begin()
        .await
        .map_err(|_| ErasureError::Database)?;
    sqlx::raw_sql("SET TRANSACTION ISOLATION LEVEL READ COMMITTED; SET LOCAL statement_timeout='10s'; SET LOCAL lock_timeout='2s'; SET LOCAL idle_in_transaction_session_timeout='10s'; SET LOCAL search_path=pg_catalog; SET LOCAL TimeZone='UTC'; SET LOCAL DateStyle='ISO, YMD'")
        .execute(&mut *tx).await.map_err(|_| ErasureError::Database)?;
    let operator: bool = sqlx::query_scalar(
        "SELECT current_user='board_migrator' AND session_user='board_migrator'",
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| ErasureError::Database)?;
    if !operator {
        return Err(ErasureError::Role);
    }
    preflight(&mut tx).await?;
    let ready: bool = sqlx::query_scalar(crate::content_erasure::MEDIA_READINESS_SQL)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| ErasureError::Schema)?;
    if !ready {
        return Err(ErasureError::Schema);
    }
    let exists: Option<String> =
        sqlx::query_scalar("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
            .bind(board)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| ErasureError::Database)?;
    if exists.is_none() {
        return Err(ErasureError::Board);
    }
    let database: String = sqlx::query_scalar("SELECT current_database()::text")
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| ErasureError::Database)?;
    let rows: Vec<(i64,String)>=sqlx::query_as("SELECT m.post_id,encode(sha256(convert_to(to_jsonb(m)::text,'UTF8')),'hex') FROM content.post_media m JOIN content.posts p ON p.id=m.post_id WHERE p.board=$1 AND p.content_erased AND NOT m.metadata_erased ORDER BY m.post_id LIMIT 64 FOR UPDATE OF p,m")
        .bind(board).fetch_all(&mut *tx).await.map_err(|_| ErasureError::Database)?;
    let digest = serde_json::to_vec(&("media-erasure-v1", &database, board, &rows))
        .map_err(|_| ErasureError::Database)?;
    let preview = Preview {
        database,
        board: board.into(),
        post_ids: rows.iter().map(|r| r.0).collect(),
        receipt: format!("{:x}", Sha256::digest(digest)),
        applied: receipt.is_some(),
    };
    if let Some(receipt) = receipt {
        if receipt != preview.receipt {
            return Err(ErasureError::Stale);
        }
        let count=sqlx::query("UPDATE content.post_media SET metadata_erased=true WHERE post_id=ANY($1) AND NOT metadata_erased")
            .bind(&preview.post_ids).execute(&mut *tx).await.map_err(|_| ErasureError::Database)?.rows_affected();
        if count != preview.post_ids.len() as u64 {
            return Err(ErasureError::Stale);
        }
        preflight(&mut tx).await?;
        tx.commit().await.map_err(|_| ErasureError::Database)?;
    } else {
        tx.rollback().await.map_err(|_| ErasureError::Database)?;
    }
    Ok(preview)
}
