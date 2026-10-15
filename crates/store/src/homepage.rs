//! Bounded frontpage snapshots with live visibility rechecks for every card.
use crate::StoreError;
use chrono::{DateTime, Utc};
use sqlx::{Connection, PgConnection, PgPool};

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Statistics {
    pub generation: i64,
    pub generated_at: DateTime<Utc>,
    pub recorded_posts: i64,
    pub active_content_bytes: i64,
    pub active_sessions: i64,
}
impl Statistics {
    pub fn stale(&self) -> bool {
        (Utc::now() - self.generated_at).num_seconds() > 900
    }
    pub fn date(&self) -> String {
        self.generated_at.format("%Y-%m-%d %H:%M UTC").to_string()
    }
}
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct Card {
    pub kind: String,
    pub slot: i16,
    pub id: i64,
    pub board: String,
    pub thread_id: i64,
    pub board_title: String,
    pub subject: String,
    pub excerpt: String,
    pub created_at: DateTime<Utc>,
    pub reply_count: i32,
    pub image_count: i32,
    pub tim: Option<i64>,
    pub bytes: Option<i64>,
    pub thumbnail_width: Option<i32>,
    pub thumbnail_height: Option<i32>,
}
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct BoardStatistics {
    pub board: String,
    pub generated_at: DateTime<Utc>,
    pub recorded_posts: i64,
    pub last_post_id: i64,
    pub active_threads: i64,
}
impl BoardStatistics {
    pub fn date(&self) -> String {
        self.generated_at.format("%Y-%m-%d %H:%M UTC").to_string()
    }
}
#[derive(Default)]
pub struct Snapshot {
    pub statistics: Option<Statistics>,
    pub cards: Vec<Card>,
}

pub async fn snapshot(pool: &PgPool) -> Result<Snapshot, StoreError> {
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await?;
    let statistics:Option<Statistics>=sqlx::query_as("SELECT generation,generated_at,recorded_posts,active_content_bytes,active_sessions FROM content.homepage_statistics").fetch_optional(&mut *tx).await?;
    let cards:Vec<Card>=sqlx::query_as("SELECT kind,slot,id,board,thread_id,board_title,subject,excerpt,created_at,reply_count,image_count,tim,bytes,thumbnail_width,thumbnail_height FROM content.homepage_cards ORDER BY kind,slot LIMIT 36").fetch_all(&mut *tx).await?;
    tx.commit().await?;
    if statistics.as_ref().is_some_and(|s| {
        s.generation < 1
            || s.recorded_posts < 0
            || s.active_content_bytes < 0
            || !(0..=1000000).contains(&s.active_sessions)
    }) || cards.iter().any(|c| {
        !matches!(c.kind.as_str(), "popular" | "latest" | "images")
            || !(1..=12).contains(&c.slot)
            || c.id <= 0
            || c.thread_id <= 0
            || board_domain::BoardSlug::parse(&c.board).is_err()
            || c.board_title.len() > 480
            || c.subject.len() > 480
            || c.excerpt.len() > 1200
            || c.tim.is_some_and(|t| t <= 0)
            || c.reply_count < 0
            || c.image_count < 0
            || c.image_count > c.reply_count
    }) {
        return Err(StoreError::ReadLimit);
    }
    Ok(Snapshot { statistics, cards })
}
pub async fn board_statistics(
    pool: &PgPool,
    board: Option<&str>,
) -> Result<Vec<BoardStatistics>, StoreError> {
    if board.is_some_and(|b| board_domain::BoardSlug::parse(b).is_err()) {
        return Err(StoreError::NotFound);
    }
    let rows: Vec<BoardStatistics> = if let Some(board) = board {
        sqlx::query_as("SELECT board,generated_at,recorded_posts,last_post_id,active_threads FROM content.homepage_board_statistics WHERE board=$1 ORDER BY generated_at DESC LIMIT 64").bind(board).fetch_all(pool).await?
    } else {
        sqlx::query_as("SELECT DISTINCT ON(board) board,generated_at,recorded_posts,last_post_id,active_threads FROM content.homepage_board_statistics ORDER BY board,generated_at DESC LIMIT 100").fetch_all(pool).await?
    };
    if rows.iter().any(|r| {
        board_domain::BoardSlug::parse(&r.board).is_err()
            || r.recorded_posts < 0
            || r.last_post_id < 0
            || r.active_threads < 0
    }) {
        return Err(StoreError::ReadLimit);
    }
    Ok(rows)
}
#[derive(Debug, thiserror::Error)]
pub enum RefreshError {
    #[error("Homepage refresh requires the actual board_migrator login.")]
    Role,
    #[error("Homepage generation budget exhausted.")]
    Generation,
    #[error("Homepage refresh failed; the previous snapshot is retained.")]
    Database,
}

pub async fn refresh(connection: &mut PgConnection) -> Result<i64, RefreshError> {
    let mut tx = connection
        .begin()
        .await
        .map_err(|_| RefreshError::Database)?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *tx)
        .await
        .map_err(|_| RefreshError::Database)?;
    let allowed: bool = sqlx::query_scalar(
        "SELECT session_user='board_migrator' AND current_user='board_migrator'",
    )
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| RefreshError::Database)?;
    if !allowed {
        return Err(RefreshError::Role);
    }
    sqlx::query("SET LOCAL lock_timeout='5s'")
        .execute(&mut *tx)
        .await
        .map_err(|_| RefreshError::Database)?;
    sqlx::query("SET LOCAL statement_timeout='30s'")
        .execute(&mut *tx)
        .await
        .map_err(|_| RefreshError::Database)?;
    sqlx::query("LOCK TABLE site_private.frontpage_state,site_private.frontpage_cards,site_private.board_statistics IN SHARE ROW EXCLUSIVE MODE").execute(&mut *tx).await.map_err(|_|RefreshError::Database)?;
    let previous: Option<i64> =
        sqlx::query_scalar("SELECT generation FROM site_private.frontpage_state")
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| RefreshError::Database)?;
    let generation = previous
        .unwrap_or(0)
        .checked_add(1)
        .filter(|n| *n <= 2147483647)
        .ok_or(RefreshError::Generation)?;
    let boards: Vec<String> = sqlx::query_scalar(
        "SELECT slug FROM content.boards WHERE NOT staff_only ORDER BY source_order,slug LIMIT 100",
    )
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| RefreshError::Database)?;
    let recorded_posts: i64 =
        sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=ANY($1)")
            .bind(&boards)
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| RefreshError::Database)?;
    let active_content_bytes:i64=sqlx::query_scalar("SELECT coalesce(sum(m.bytes),0)::bigint FROM content.visible_post_media m JOIN content.posts p ON p.id=m.post_id JOIN content.threads t ON t.id=p.thread_id WHERE p.board=ANY($1) AND NOT p.deleted AND NOT t.deleted AND t.archived_at IS NULL AND NOT m.file_deleted").bind(&boards).fetch_one(&mut *tx).await.map_err(|_|RefreshError::Database)?;
    let active_sessions:i64=sqlx::query_scalar("SELECT count(DISTINCT s.token_hash) FROM post_secrets.anonymous_sessions s JOIN post_secrets.anonymous_posts a ON a.token_hash=s.token_hash JOIN content.posts p ON p.id=a.post_id JOIN content.threads t ON t.id=p.thread_id WHERE p.board=ANY($1) AND NOT p.deleted AND NOT t.deleted AND t.archived_at IS NULL AND s.activity_at>=extract(epoch FROM CURRENT_TIMESTAMP)::bigint-900 AND s.expires_at>extract(epoch FROM CURRENT_TIMESTAMP)::bigint").bind(&boards).fetch_one(&mut *tx).await.map_err(|_|RefreshError::Database)?;
    sqlx::query("INSERT INTO site_private.frontpage_state(singleton,generation,generated_at,board_slugs,recorded_posts,active_content_bytes,active_sessions) VALUES(true,$1,CURRENT_TIMESTAMP,$2,$3,$4,$5) ON CONFLICT(singleton) DO UPDATE SET generation=excluded.generation,generated_at=excluded.generated_at,board_slugs=excluded.board_slugs,recorded_posts=excluded.recorded_posts,active_content_bytes=excluded.active_content_bytes,active_sessions=excluded.active_sessions")
        .bind(generation).bind(&boards).bind(recorded_posts).bind(active_content_bytes).bind(active_sessions).execute(&mut *tx).await.map_err(|_|RefreshError::Database)?;
    sqlx::query("DELETE FROM site_private.frontpage_cards")
        .execute(&mut *tx)
        .await
        .map_err(|_| RefreshError::Database)?;
    for (kind, image) in [("latest", false), ("images", true)] {
        // The source limits each board to five candidates before check_post.
        sqlx::query("INSERT INTO site_private.frontpage_cards(kind,slot,post_id) SELECT $1,row_number() OVER(ORDER BY e.created_at DESC,e.id DESC)::smallint,e.id FROM unnest($2::text[]) board(slug) CROSS JOIN LATERAL (SELECT p.id FROM content.posts p JOIN content.threads t ON t.id=p.thread_id AND t.board=p.board WHERE p.board=board.slug AND NOT p.deleted AND NOT t.deleted AND t.archived_at IS NULL AND p.created_at<CURRENT_TIMESTAMP-interval '10 minutes' AND (NOT $3 OR EXISTS(SELECT 1 FROM content.visible_post_media m WHERE m.post_id=p.id AND NOT m.file_deleted)) ORDER BY p.created_at DESC,p.id DESC LIMIT 5) candidate JOIN site_private.frontpage_eligible e ON e.id=candidate.id ORDER BY e.created_at DESC,e.id DESC LIMIT 12")
            .bind(kind).bind(&boards).bind(image).execute(&mut *tx).await.map_err(|_|RefreshError::Database)?;
    }
    sqlx::query("INSERT INTO site_private.frontpage_cards(kind,slot,post_id) SELECT 'popular',row_number() OVER(ORDER BY e.reply_count DESC,e.bumped_at DESC,e.id DESC)::smallint,e.id FROM site_private.frontpage_eligible e WHERE e.board=ANY($1) AND e.id=e.thread_id AND e.reply_count>=15 AND (e.sticky OR e.bumped_at>CURRENT_TIMESTAMP-interval '10 minutes') ORDER BY e.reply_count DESC,e.bumped_at DESC,e.id DESC LIMIT 12")
        .bind(&boards).execute(&mut *tx).await.map_err(|_|RefreshError::Database)?;
    sqlx::query("DELETE FROM site_private.board_statistics WHERE board<>ALL($1)")
        .bind(&boards)
        .execute(&mut *tx)
        .await
        .map_err(|_| RefreshError::Database)?;
    sqlx::query("INSERT INTO site_private.board_statistics(board,slot,generated_at,recorded_posts,last_post_id,active_threads) SELECT board.slug,$2,CURRENT_TIMESTAMP,(SELECT count(*) FROM content.posts p WHERE p.board=board.slug),(SELECT coalesce(max(id),0) FROM content.posts p WHERE p.board=board.slug),(SELECT count(*) FROM content.threads t WHERE t.board=board.slug AND NOT t.deleted AND t.archived_at IS NULL) FROM unnest($1::text[]) board(slug) ON CONFLICT(board,slot) DO UPDATE SET generated_at=excluded.generated_at,recorded_posts=excluded.recorded_posts,last_post_id=excluded.last_post_id,active_threads=excluded.active_threads")
        .bind(&boards).bind(((generation-1)%64+1) as i16).execute(&mut *tx).await.map_err(|_|RefreshError::Database)?;
    tx.commit().await.map_err(|_| RefreshError::Database)?;
    Ok(generation)
}

/// Fail readiness on changed visibility predicates, bounds, ownership or grants.
pub const READINESS_SQL: &str = include_str!("homepage_readiness.sql");
