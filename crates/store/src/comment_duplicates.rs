//! Private source comparison history. Never expose the peer or projection key.
use crate::StoreError;
use board_domain::{
    comment_admission_projection::{AdmissionProjection, DrawingAnnotation},
    poster_id::PublicPostingRateIdentity,
};
use sqlx::{Postgres, Transaction};

pub(crate) async fn check(
    tx: &mut Transaction<'_, Postgres>,
    actor: &PublicPostingRateIdentity,
    board: &str,
    request: i64,
    projection: Option<&AdmissionProjection>,
) -> Result<(), StoreError> {
    let Some(projection) = projection.filter(|value| value.source_check_applies()) else {
        return Ok(());
    };
    let remaining: Option<i64> =
        sqlx::query_scalar("SELECT content.comment_duplicate_remaining($1,$2,$3,$4)")
            .bind(actor.as_bytes().as_slice())
            .bind(board)
            .bind(projection.comparison_hash().as_slice())
            .bind(request)
            .fetch_one(&mut **tx)
            .await?;
    match remaining {
        None => Ok(()),
        Some(seconds) if seconds > 0 => Err(StoreError::ContentRejected(
            board_domain::comment_duplicates::refusal(seconds),
        )),
        Some(_) => Err(sqlx::Error::Protocol("Invalid duplicate comment duration".into()).into()),
    }
}

pub(crate) async fn record(
    tx: &mut Transaction<'_, Postgres>,
    actor: &PublicPostingRateIdentity,
    post: i64,
    projection: &AdmissionProjection,
) -> Result<(), StoreError> {
    let (seconds, source): (Option<i32>, Option<i64>) = sqlx::query_as(
        "SELECT drawing_time_seconds,drawing_source_post_id FROM content.posts WHERE id=$1 AND NOT deleted")
        .bind(post).fetch_one(&mut **tx).await?;
    let replay: Option<i64> = sqlx::query_scalar(
        "SELECT tim FROM content.visible_post_media WHERE post_id=$1 AND has_replay",
    )
    .bind(post)
    .fetch_optional(&mut **tx)
    .await?;
    let drawing = seconds.map(|seconds| DrawingAnnotation {
        seconds: i64::from(seconds),
        source_post: source,
        replay_tim: replay,
    });
    sqlx::query("SELECT content.record_comment_projection($1,$2,$3)")
        .bind(post)
        .bind(actor.as_bytes().as_slice())
        .bind(projection.persisted_hash(drawing).as_slice())
        .execute(&mut **tx)
        .await?;
    Ok(())
}
