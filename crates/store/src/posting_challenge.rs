//! Public board switches for selecting challenge controls. Writes recheck them.
use crate::StoreError;
use sqlx::PgPool;

#[derive(sqlx::FromRow)]
pub struct BoardChallengePolicy {
    pub enabled: bool,
    pub twister: bool,
    pub noscript_only: bool,
}
pub async fn board_policy(pool: &PgPool, board: &str) -> Result<BoardChallengePolicy, StoreError> {
    board_domain::BoardSlug::parse(board).map_err(|_| StoreError::NotFound)?;
    sqlx::query_as("SELECT captcha_enabled AS enabled,captcha_twister AS twister,noscript_captcha_only AS noscript_only FROM content.boards WHERE slug=$1 AND NOT staff_only")
        .bind(board).fetch_optional(pool).await?.ok_or(StoreError::NotFound)
}
