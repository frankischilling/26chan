use crate::{Board, PageSnapshot, PgPool, Post, StoreError, Thread};
use std::collections::BTreeMap;

pub enum BoardSelection {
    Page(i64),
    All,
}

/// Explicit internal preview bounds remain distinct from source board policy.
#[derive(Clone, Copy)]
enum PreviewSelection {
    Metadata,
    Fixed(i64),
    Source,
}
impl From<Option<i64>> for PreviewSelection {
    fn from(replies: Option<i64>) -> Self {
        replies.map_or(Self::Metadata, Self::Fixed)
    }
}

/// Source-configured public previews. Board policy and selected posts share
/// the same read snapshot; callers must not fetch policy in a separate read.
pub async fn source_board_snapshot(
    pool: &PgPool,
    slug: &str,
    selection: BoardSelection,
) -> Result<BoardSnapshot, StoreError> {
    Ok(read_board_snapshot(
        pool,
        slug,
        selection,
        PreviewSelection::Source,
        false,
        false,
        MAX_BOARD_READ_THREADS,
    )
    .await?
    .snapshot)
}
pub async fn source_json_board_snapshot(
    pool: &PgPool,
    slug: &str,
    selection: BoardSelection,
) -> Result<BoardSnapshot, StoreError> {
    Ok(read_board_snapshot(
        pool,
        slug,
        selection,
        PreviewSelection::Source,
        false,
        true,
        MAX_BOARD_READ_THREADS,
    )
    .await?
    .snapshot)
}
pub async fn source_board_page_snapshot(
    pool: &PgPool,
    slug: &str,
    selection: BoardSelection,
) -> Result<PageSnapshot<BoardSnapshot>, StoreError> {
    read_board_snapshot(
        pool,
        slug,
        selection,
        PreviewSelection::Source,
        true,
        false,
        MAX_BOARD_READ_THREADS,
    )
    .await
}

pub struct ThreadPreview {
    pub thread: Thread,
    pub posts: Vec<Post>,
    pub visible_posts: i64,
    pub visible_images: i64,
    pub unique_ips: Option<i32>,
    pub latest_reply_id: Option<i64>,
    pub catalog_last_reply: Option<CatalogReply>,
    pub capcode_replies: Vec<(i64, String)>,
}

#[derive(sqlx::FromRow)]
pub struct CatalogReply {
    pub thread_id: i64,
    pub id: i64,
    pub name: String,
    pub trip: Option<String>,
    pub capcode: Option<String>,
    pub poster_id: Option<String>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

pub struct BoardSnapshot {
    pub quote_targets: crate::QuoteTargets,
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
    Ok(read_board_snapshot(
        pool,
        slug,
        selection,
        replies,
        false,
        false,
        MAX_BOARD_READ_THREADS,
    )
    .await?
    .snapshot)
}

/// Meta-board JSON includes badge IDs from replies omitted from the preview.
/// Load only public headers, inside the same snapshot and a separate ID budget.
pub async fn json_board_snapshot(
    pool: &PgPool,
    slug: &str,
    selection: BoardSelection,
    replies: i64,
) -> Result<BoardSnapshot, StoreError> {
    Ok(read_board_snapshot(
        pool,
        slug,
        selection,
        Some(replies),
        false,
        true,
        MAX_BOARD_READ_THREADS,
    )
    .await?
    .snapshot)
}

/// Independent of ordinary board capacity: protected OPs also need listing.
/// Complete listings fail closed rather than silently dropping excess threads.
pub const MAX_BOARD_READ_THREADS: usize = 1000;

/// A release-owned complete projection can impose a smaller metadata budget.
pub async fn board_snapshot_all_bounded(
    pool: &PgPool,
    slug: &str,
    replies: Option<i64>,
    max_threads: usize,
) -> Result<BoardSnapshot, StoreError> {
    if max_threads == 0 || max_threads > MAX_BOARD_READ_THREADS {
        return Err(StoreError::Invalid("Invalid snapshot thread budget."));
    }
    Ok(read_board_snapshot(
        pool,
        slug,
        BoardSelection::All,
        replies,
        false,
        false,
        max_threads,
    )
    .await?
    .snapshot)
}

pub const MAX_JSON_CAPCODE_REPLY_IDS: usize = 100_000;

/// Include navigation in the same transaction as HTML board or catalog content.
pub async fn board_page_snapshot(
    pool: &PgPool,
    slug: &str,
    selection: BoardSelection,
    replies: Option<i64>,
) -> Result<PageSnapshot<BoardSnapshot>, StoreError> {
    read_board_snapshot(
        pool,
        slug,
        selection,
        replies,
        true,
        false,
        MAX_BOARD_READ_THREADS,
    )
    .await
}

async fn read_board_snapshot(
    pool: &PgPool,
    slug: &str,
    selection: BoardSelection,
    replies: impl Into<PreviewSelection>,
    include_navigation: bool,
    include_capcode_replies: bool,
    max_threads: usize,
) -> Result<PageSnapshot<BoardSnapshot>, StoreError> {
    board_domain::BoardSlug::parse(slug).map_err(|_| StoreError::NotFound)?;
    let replies = replies.into();
    if matches!(replies, PreviewSelection::Fixed(limit) if !(0..=5).contains(&limit)) {
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
    if !(0..=board_domain::preview::MAX_PREVIEW_REPLIES as i32).contains(&board.replies_shown) {
        return Err(StoreError::Invalid("Invalid source preview policy."));
    }
    let per_page = i64::from(board.threads_per_page);
    let maximum = i64::from(board.thread_limit);
    let max_pages = (maximum + per_page - 1) / per_page;
    let complete = matches!(selection, BoardSelection::All);
    let (offset, limit, later_page) = match selection {
        BoardSelection::Page(page) if (1..=max_pages).contains(&page) => {
            ((page - 1) * per_page, per_page, page < max_pages)
        }
        BoardSelection::Page(_) => return Err(StoreError::PageNotFound),
        BoardSelection::All => (0, max_threads as i64, false),
    };
    // One extra metadata row detects a successor for numbered pages, or an
    // incomplete full listing. Reject excess complete listings before bodies/media.
    // Body-bearing public previews must have a live OP. Otherwise a reply can
    // masquerade as the OP and refer to a thread the reader cannot navigate.
    // This is backend visibility hardening, not a source SQL compatibility claim.
    // Preserve existing body-free metadata and internal staff projections.
    let mut threads: Vec<Thread> = sqlx::query_as("SELECT t.* FROM content.visible_threads t WHERE t.board=$1 AND NOT t.deleted AND t.archived_at IS NULL AND ($4::boolean OR EXISTS (SELECT 1 FROM content.posts op WHERE op.board=t.board AND op.thread_id=t.id AND op.id=t.id AND NOT op.deleted)) ORDER BY t.sticky DESC,CASE WHEN t.sticky THEN t.sticky_rank ELSE 0 END DESC,t.bumped_at DESC,t.id DESC OFFSET $2 LIMIT $3")
        .bind(slug).bind(offset).bind(limit + i64::from(later_page || complete)).bind(matches!(replies,PreviewSelection::Metadata) || board.staff_only).fetch_all(&mut *tx).await?;
    if complete && threads.len() > max_threads {
        return Err(StoreError::ReadLimit);
    }
    let has_next = later_page && threads.len() > limit as usize;
    threads.truncate(limit as usize);
    let ids: Vec<i64> = threads.iter().map(|thread| thread.id).collect();
    let preview_limits: Option<Vec<i64>> = match replies {
        PreviewSelection::Metadata => None,
        PreviewSelection::Fixed(limit) => Some(vec![limit; ids.len()]),
        PreviewSelection::Source => Some(
            threads
                .iter()
                .map(|thread| board.preview_reply_limit(thread.sticky) as i64)
                .collect(),
        ),
    };
    let counts: Vec<(i64, i64, Option<i64>)> = sqlx::query_as("SELECT thread_id,count(*),max(id) FILTER (WHERE id<>thread_id) FROM content.posts WHERE board=$1 AND thread_id=ANY($2) AND NOT deleted GROUP BY thread_id")
        .bind(slug).bind(&ids).fetch_all(&mut *tx).await?;
    let mut capcode_replies: BTreeMap<i64, Vec<(i64, String)>> = BTreeMap::new();
    if include_capcode_replies && board.meta_board {
        if counts.iter().any(|entry| !(0..=1001).contains(&entry.1)) {
            return Err(StoreError::ReadLimit);
        }
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1 AND thread_id=ANY($2) AND id<>thread_id AND NOT deleted AND capcode IS NOT NULL")
            .bind(slug).bind(&ids).fetch_one(&mut *tx).await?;
        if !(0..=MAX_JSON_CAPCODE_REPLY_IDS as i64).contains(&count) {
            return Err(StoreError::ReadLimit);
        }
        let rows: Vec<(i64, i64, String)> = sqlx::query_as("SELECT thread_id,id,capcode FROM content.posts WHERE board=$1 AND thread_id=ANY($2) AND id<>thread_id AND NOT deleted AND capcode IS NOT NULL ORDER BY thread_id,id LIMIT $3")
            .bind(slug).bind(&ids).bind(MAX_JSON_CAPCODE_REPLY_IDS as i64 + 1).fetch_all(&mut *tx).await?;
        if rows.len() != count as usize {
            return Err(StoreError::Invalid("Incomplete badge reply snapshot."));
        }
        for (thread_id, id, capcode) in rows {
            capcode_replies
                .entry(thread_id)
                .or_default()
                .push((id, capcode));
        }
    }
    // Catalog hover details need only the latest visible reply's public header.
    // The count and this bounded batch share the same repeatable-read snapshot.
    let mut catalog_replies: BTreeMap<i64, CatalogReply> = if matches!(
        replies,
        PreviewSelection::Fixed(0)
    ) {
        let latest: Vec<i64> = counts.iter().filter_map(|entry| entry.2).collect();
        sqlx::query_as::<_, CatalogReply>("SELECT thread_id,id,name,trip,capcode,poster_id,created_at FROM content.posts WHERE board=$1 AND id=ANY($2) AND NOT deleted")
            .bind(slug).bind(latest).fetch_all(&mut *tx).await?
            .into_iter().map(|reply| (reply.thread_id, reply)).collect()
    } else {
        BTreeMap::new()
    };
    let images: Vec<(i64, i64)> = sqlx::query_as("SELECT p.thread_id,count(*) FROM content.posts p JOIN content.visible_post_media m ON m.post_id=p.id WHERE p.board=$1 AND p.thread_id=ANY($2) AND p.id<>p.thread_id AND NOT m.file_deleted GROUP BY p.thread_id")
        .bind(slug).bind(&ids).fetch_all(&mut *tx).await?;
    // At most 1,000 selected threads, each with its OP and five latest replies.
    // Lateral limits keep unselected comment bodies out of the web process.
    let mut posts: Vec<Post> = if let Some(limits) = preview_limits.as_ref() {
        // Only selected posts with a persisted staff proof add a larger format
        // allowance. Ordinary slots retain their former raw-comment ceiling.
        let slots: usize = limits.iter().map(|limit| *limit as usize + 1).sum();
        let ordinary_bytes = board_domain::MAX_COMMENT_BYTES * slots;
        let max_replies = limits.iter().copied().max().unwrap_or(0) as usize;
        let (body_bytes, authorized_posts): (i64,i64) = sqlx::query_as("SELECT coalesce(sum(octet_length(p.comment)+coalesce(octet_length(p.wordfilter_payload),0)+coalesce(octet_length(p.wordfilter_search),0)),0)::bigint,count(*) FILTER (WHERE p.staff_authorized_limits) FROM unnest($2::bigint[],$3::bigint[]) AS selected(id,reply_limit) CROSS JOIN LATERAL ((SELECT comment,wordfilter_payload,wordfilter_search,staff_authorized_limits FROM content.posts WHERE board=$1 AND thread_id=selected.id AND id=selected.id AND NOT deleted) UNION ALL (SELECT comment,wordfilter_payload,wordfilter_search,staff_authorized_limits FROM content.posts WHERE board=$1 AND thread_id=selected.id AND id<>selected.id AND NOT deleted ORDER BY id DESC LIMIT selected.reply_limit)) p")
            .bind(slug).bind(&ids).bind(limits).fetch_one(&mut *tx).await?;
        let extra = board_domain::WordfilterLimits::Authorized.saved_post_read_bytes()
            - board_domain::MAX_COMMENT_BYTES;
        // Keep the former maximum for 1,000 selected threads. A larger staff
        // slot must not enlarge the overall database-to-process read ceiling.
        let max_bytes = (ordinary_bytes + authorized_posts as usize * extra)
            .min(board_domain::MAX_COMMENT_BYTES * 1000 * (max_replies + 1));
        if body_bytes > max_bytes as i64 {
            return Err(StoreError::ReadLimit);
        }
        sqlx::query_as("SELECT p.* FROM unnest($2::bigint[],$3::bigint[]) AS selected(id,reply_limit) CROSS JOIN LATERAL ((SELECT * FROM content.posts WHERE board=$1 AND thread_id=selected.id AND id=selected.id AND NOT deleted) UNION ALL (SELECT * FROM content.posts WHERE board=$1 AND thread_id=selected.id AND id<>selected.id AND NOT deleted ORDER BY id DESC LIMIT selected.reply_limit)) p ORDER BY p.thread_id,p.id")
            .bind(slug).bind(&ids).bind(limits).fetch_all(&mut *tx).await?
    } else {
        Vec::new()
    };
    crate::post_media::load(&mut tx, &mut posts).await?;
    let quote_targets = if board.staff_only {
        crate::QuoteTargets::default()
    } else {
        crate::quote_targets::load(&mut tx, &posts).await?
    };
    let poster_counts: Vec<(i64, Option<i32>)> = sqlx::query_as(
        "SELECT id,content.unique_posters($1,id) FROM unnest($2::bigint[]) selected(id)",
    )
    .bind(slug)
    .bind(&ids)
    .fetch_all(&mut *tx)
    .await?;
    let navigation_boards = crate::read::snapshot_navigation(&mut tx, include_navigation).await?;
    let blotter = crate::blotter::snapshot_blotter(
        &mut tx,
        include_navigation && board.show_blotter && !complete,
    )
    .await?;
    tx.commit().await?;

    let counts: BTreeMap<_, _> = counts
        .into_iter()
        .map(|(id, count, latest)| (id, (count, latest)))
        .collect();
    let images: BTreeMap<_, _> = images.into_iter().collect();
    let poster_counts: BTreeMap<_, _> = poster_counts.into_iter().collect();
    let mut previews: BTreeMap<i64, Vec<Post>> = BTreeMap::new();
    for post in posts {
        previews.entry(post.thread_id).or_default().push(post);
    }
    let threads = threads
        .into_iter()
        .map(|thread| ThreadPreview {
            posts: previews.remove(&thread.id).unwrap_or_default(),
            visible_posts: counts.get(&thread.id).map_or(0, |value| value.0),
            visible_images: images.get(&thread.id).copied().unwrap_or(0),
            unique_ips: poster_counts.get(&thread.id).copied().flatten(),
            latest_reply_id: counts.get(&thread.id).and_then(|value| value.1),
            catalog_last_reply: catalog_replies.remove(&thread.id),
            capcode_replies: capcode_replies.remove(&thread.id).unwrap_or_default(),
            thread,
        })
        .collect();
    Ok(PageSnapshot {
        snapshot: BoardSnapshot {
            quote_targets,
            board,
            threads,
            has_next,
        },
        navigation_boards,
        blotter,
    })
}
