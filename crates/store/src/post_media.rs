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
