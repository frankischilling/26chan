use crate::{Board, PgPool, Post, StoreError, Thread};
use std::collections::BTreeMap;

pub enum BoardSelection {
    Page(i64),
    All,
}

pub struct ThreadPreview {
    pub thread: Thread,
    pub posts: Vec<Post>,
    pub visible_posts: i64,
}

pub struct BoardSnapshot {
    pub board: Board,
    pub threads: Vec<ThreadPreview>,
    pub has_next: bool,
}

/// Read settings, ordering, visible counts and bounded previews in one snapshot.
/// The transaction is released before a caller renders the response.
pub async fn board_snapshot(
    pool: &PgPool,
    slug: &str,
    selection: BoardSelection,
    replies: Option<i64>,
) -> Result<BoardSnapshot, StoreError> {
    board_domain::BoardSlug::parse(slug).map_err(|_| StoreError::NotFound)?;
    if replies.is_some_and(|limit| !(0..=5).contains(&limit)) {
        return Err(StoreError::Invalid("Invalid preview limit."));
    }
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await?;
    let board: Board = sqlx::query_as("SELECT * FROM content.boards WHERE slug=$1")
        .bind(slug)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(StoreError::NotFound)?;
    let per_page = i64::from(board.threads_per_page);
    let maximum = i64::from(board.thread_limit);
    let max_pages = (maximum + per_page - 1) / per_page;
    let (offset, limit, later_page) = match selection {
        BoardSelection::Page(page) if (1..=max_pages).contains(&page) => {
            ((page - 1) * per_page, per_page, page < max_pages)
        }
        BoardSelection::Page(_) => return Err(StoreError::PageNotFound),
        BoardSelection::All => (0, maximum, false),
    };
    let threads: Vec<Thread> = sqlx::query_as("SELECT * FROM content.visible_threads WHERE board=$1 AND NOT deleted AND archived_at IS NULL ORDER BY sticky DESC,bumped_at DESC,id DESC OFFSET $2 LIMIT $3")
        .bind(slug).bind(offset).bind(limit).fetch_all(&mut *tx).await?;
    let has_next = later_page && threads.len() == limit as usize;
    let ids: Vec<i64> = threads.iter().map(|thread| thread.id).collect();
    let counts: Vec<(i64, i64)> = sqlx::query_as("SELECT thread_id,count(*) FROM content.posts WHERE board=$1 AND thread_id=ANY($2) AND NOT deleted GROUP BY thread_id")
        .bind(slug).bind(&ids).fetch_all(&mut *tx).await?;
    // At most 1,000 selected threads, each with its OP and five latest replies.
    // Lateral limits keep unselected comment bodies out of the web process.
    let mut posts: Vec<Post> = if let Some(replies) = replies {
        sqlx::query_as("SELECT p.* FROM unnest($2::bigint[]) AS selected(id) CROSS JOIN LATERAL ((SELECT * FROM content.posts WHERE board=$1 AND thread_id=selected.id AND id=selected.id AND NOT deleted) UNION ALL (SELECT * FROM content.posts WHERE board=$1 AND thread_id=selected.id AND id<>selected.id AND NOT deleted ORDER BY id DESC LIMIT $3)) p ORDER BY p.thread_id,p.id")
            .bind(slug).bind(&ids).bind(replies).fetch_all(&mut *tx).await?
    } else {
        Vec::new()
    };
    crate::post_media::load(&mut tx, &mut posts).await?;
    tx.commit().await?;

    let counts: BTreeMap<_, _> = counts.into_iter().collect();
    let mut previews: BTreeMap<i64, Vec<Post>> = BTreeMap::new();
    for post in posts {
        previews.entry(post.thread_id).or_default().push(post);
    }
    let threads = threads
        .into_iter()
        .map(|thread| ThreadPreview {
            posts: previews.remove(&thread.id).unwrap_or_default(),
            visible_posts: counts.get(&thread.id).copied().unwrap_or(0),
            thread,
        })
        .collect();
    Ok(BoardSnapshot {
        board,
        threads,
        has_next,
    })
}
