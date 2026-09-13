//! One-use attachment authorization. Public credentials cannot approve media.
use crate::{StoreError, media_intake::IntakeReservation};
use sqlx::PgPool;

/// Deliberately has no Debug implementation: the capability is a bearer secret.
pub struct NewAttachment {
    pub upload: IntakeReservation,
    pub spoiler: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, sqlx::FromRow)]
pub struct PostAttachment {
    pub post_id: i64,
    pub asset_id: String,
    pub filename: String,
    pub bytes: i64,
    pub width: i32,
    pub height: i32,
    pub spoiler: bool,
    pub file_deleted: bool,
}

pub async fn attachment(pool: &PgPool, post_id: i64) -> Result<Option<PostAttachment>, StoreError> {
    Ok(
        sqlx::query_as("SELECT * FROM content.visible_post_media WHERE post_id = $1")
            .bind(post_id)
            .fetch_optional(pool)
            .await?,
    )
}

pub(crate) async fn load(
    connection: &mut sqlx::PgConnection,
    posts: &mut [crate::Post],
) -> Result<(), StoreError> {
    let ids: Vec<i64> = posts.iter().map(|p| p.id).collect();
    let rows: Vec<PostAttachment> =
        sqlx::query_as("SELECT * FROM content.visible_post_media WHERE post_id=ANY($1)")
            .bind(&ids)
            .fetch_all(connection)
            .await?;
    let mut rows: std::collections::BTreeMap<_, _> =
        rows.into_iter().map(|a| (a.post_id, a)).collect();
    for post in posts {
        post.attachment = rows.remove(&post.id);
    }
    Ok(())
}

pub async fn check_upload(pool: &PgPool, job_id: &str, capability: &str) -> Result<(), StoreError> {
    sqlx::query("SELECT content.check_attachment_upload($1,$2)")
        .bind(job_id)
        .bind(capability)
        .execute(pool)
        .await
        .map_err(scoped_error)?;
    Ok(())
}

pub async fn cancel_upload(
    pool: &PgPool,
    job_id: &str,
    capability: &str,
) -> Result<(), StoreError> {
    sqlx::query("SELECT content.cancel_attachment_upload($1,$2)")
        .bind(job_id)
        .bind(capability)
        .execute(pool)
        .await
        .map_err(scoped_error)?;
    Ok(())
}

/// Caller verifies the post's deletion password or the staff session first.
pub async fn delete_attachment(pool: &PgPool, board: &str, post_id: i64) -> Result<(), StoreError> {
    sqlx::query("SELECT content.delete_post_attachment($1, $2)")
        .bind(board)
        .bind(post_id)
        .execute(pool)
        .await
        .map_err(scoped_error)?;
    Ok(())
}

pub(crate) fn scoped_error(error: sqlx::Error) -> StoreError {
    match error.as_database_error().and_then(|e| e.code()).as_deref() {
        Some("P0002") => StoreError::NotFound,
        Some("P0001") => StoreError::Conflict(
            "Attachment is unavailable, already used, or the image limit was reached.",
        ),
        Some("22023") => StoreError::Invalid("Invalid attachment request."),
        _ => StoreError::Database(error),
    }
}
