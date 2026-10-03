use crate::*;

/// Count rows without transferring comment bodies to a web process.
pub async fn visible_post_count(pool: &PgPool, slug: &str, id: i64) -> Result<i64, StoreError> {
    Ok(sqlx::query_scalar("SELECT count(*) FROM content.posts p JOIN content.visible_threads t ON t.id=p.thread_id WHERE p.board=$1 AND p.thread_id=$2 AND NOT p.deleted AND NOT t.deleted").bind(slug).bind(id).fetch_one(pool).await?)
}

/// Fetch only the OP and at most five latest replies. Limits apply in SQL.
pub async fn preview_posts(
    pool: &PgPool,
    slug: &str,
    id: i64,
    replies: i64,
) -> Result<Vec<Post>, StoreError> {
    if !(0..=5).contains(&replies) {
        return Err(StoreError::Invalid("Invalid preview limit."));
    }
    Ok(sqlx::query_as("SELECT p.* FROM ((SELECT * FROM content.posts WHERE board=$1 AND thread_id=$2 AND id=$2 AND NOT deleted) UNION ALL (SELECT * FROM content.posts WHERE board=$1 AND thread_id=$2 AND id<>$2 AND NOT deleted ORDER BY id DESC LIMIT $3)) p WHERE EXISTS (SELECT 1 FROM content.visible_threads WHERE board=$1 AND id=$2 AND NOT deleted) ORDER BY p.id").bind(slug).bind(id).bind(replies).fetch_all(pool).await?)
}

pub async fn boards(pool: &PgPool) -> Result<Vec<Board>, StoreError> {
    Ok(
        sqlx::query_as("SELECT * FROM content.boards ORDER BY source_order,slug LIMIT 100")
            .fetch_all(pool)
            .await?,
    )
}

/// Public page content and its bounded navigation directory share one snapshot.
pub struct PageSnapshot<T> {
    pub snapshot: T,
    pub navigation_boards: Vec<Board>,
}

pub(crate) async fn snapshot_navigation(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    include_navigation: bool,
) -> Result<Vec<Board>, StoreError> {
    if !include_navigation {
        return Ok(Vec::new());
    }
    Ok(
        sqlx::query_as("SELECT * FROM content.boards ORDER BY source_order,slug LIMIT 100")
            .fetch_all(&mut **tx)
            .await?,
    )
}
pub async fn board(pool: &PgPool, slug: &str) -> Result<Board, StoreError> {
    board_domain::BoardSlug::parse(slug).map_err(|_| StoreError::NotFound)?;
    sqlx::query_as("SELECT * FROM content.boards WHERE slug=$1")
        .bind(slug)
        .fetch_optional(pool)
        .await?
        .ok_or(StoreError::NotFound)
}
pub async fn thread(pool: &PgPool, slug: &str, id: i64) -> Result<Thread, StoreError> {
    sqlx::query_as("SELECT * FROM content.visible_threads WHERE board=$1 AND id=$2 AND NOT deleted")
        .bind(slug)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(StoreError::NotFound)
}
pub struct ThreadSnapshot {
    pub board: Board,
    pub thread: Thread,
    pub posts: Vec<Post>,
    pub replies: usize,
    pub images: usize,
    pub unique_ips: Option<i32>,
    pub tail_size: usize,
    pub tail_id: Option<i64>,
}

// Preserve the former maximum of 1001 raw 64,000-byte comments. Expanded filtered
// projections and their saved data share that ceiling before any rows transfer.
pub const MAX_THREAD_READ_BYTES: usize = board_domain::MAX_COMMENT_BYTES * 1001;

/// Read board settings, thread metadata and posts from one database snapshot.
/// Release the transaction before the caller renders the representation.
pub async fn thread_snapshot(
    pool: &PgPool,
    slug: &str,
    id: i64,
) -> Result<ThreadSnapshot, StoreError> {
    thread_snapshot_selection(pool, slug, id, false).await
}

/// Full counts, policy, boundary and selected posts share one read transaction.
pub async fn thread_snapshot_selection(
    pool: &PgPool,
    slug: &str,
    id: i64,
    tail: bool,
) -> Result<ThreadSnapshot, StoreError> {
    Ok(
        read_thread_snapshot(pool, slug, id, tail, false, MAX_THREAD_READ_BYTES)
            .await?
            .snapshot,
    )
}

/// A release-owned read projection can impose a smaller aggregate body budget.
pub async fn thread_snapshot_selection_bounded(
    pool: &PgPool,
    slug: &str,
    id: i64,
    tail: bool,
    max_bytes: usize,
) -> Result<ThreadSnapshot, StoreError> {
    if max_bytes == 0 || max_bytes > MAX_THREAD_READ_BYTES {
        return Err(StoreError::Invalid("Invalid snapshot byte budget."));
    }
    Ok(read_thread_snapshot(pool, slug, id, tail, false, max_bytes)
        .await?
        .snapshot)
}

/// Include navigation in the same transaction as the full HTML thread.
pub async fn thread_page_snapshot(
    pool: &PgPool,
    slug: &str,
    id: i64,
) -> Result<PageSnapshot<ThreadSnapshot>, StoreError> {
    read_thread_snapshot(pool, slug, id, false, true, MAX_THREAD_READ_BYTES).await
}

async fn read_thread_snapshot(
    pool: &PgPool,
    slug: &str,
    id: i64,
    tail: bool,
    include_navigation: bool,
    max_bytes: usize,
) -> Result<PageSnapshot<ThreadSnapshot>, StoreError> {
    board_domain::BoardSlug::parse(slug).map_err(|_| StoreError::NotFound)?;
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await?;
    let board = sqlx::query_as("SELECT * FROM content.boards WHERE slug=$1")
        .bind(slug)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(StoreError::NotFound)?;
    let metadata: Thread = sqlx::query_as(
        "SELECT * FROM content.visible_threads WHERE board=$1 AND id=$2 AND NOT deleted",
    )
    .bind(slug)
    .bind(id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(StoreError::NotFound)?;
    let (configured, undead): (i32, bool) = sqlx::query_as("SELECT b.json_tail_size,t.undead FROM content.boards b JOIN content.threads t ON t.board=b.slug WHERE b.slug=$1 AND t.id=$2")
        .bind(slug).bind(id).fetch_one(&mut *tx).await?;
    let (reply_count, image_count): (i64, i64) = sqlx::query_as("SELECT count(*),count(*) FILTER (WHERE m.post_id IS NOT NULL AND NOT m.file_deleted) FROM content.posts p LEFT JOIN content.visible_post_media m ON m.post_id=p.id WHERE p.board=$1 AND p.thread_id=$2 AND p.id<>$2 AND NOT p.deleted")
        .bind(slug).bind(id).fetch_one(&mut *tx).await?;
    if !(0..=1000).contains(&reply_count) || !(0..=500).contains(&configured) {
        return Err(StoreError::Invalid("Thread exceeds snapshot limits."));
    }
    let replies = reply_count as usize;
    let images = image_count as usize;
    let tail_size =
        board_domain::thread_tail_size(configured as u16, metadata.sticky, undead, replies);
    if tail && tail_size == 0 {
        return Err(StoreError::NotFound);
    }
    let tail_id = if tail {
        Some(sqlx::query_scalar::<_, i64>("SELECT id FROM content.posts WHERE board=$1 AND thread_id=$2 AND id<>$2 AND NOT deleted ORDER BY id DESC OFFSET $3 LIMIT 1")
            .bind(slug).bind(id).bind(tail_size as i64).fetch_one(&mut *tx).await?)
    } else {
        None
    };
    let body_bytes: i64 = sqlx::query_scalar("SELECT coalesce(sum(octet_length(comment)+coalesce(octet_length(wordfilter_payload),0)+coalesce(octet_length(wordfilter_search),0)),0)::bigint FROM content.posts WHERE board=$1 AND thread_id=$2 AND NOT deleted AND ($3::bigint IS NULL OR id=$2 OR id>$3)")
        .bind(slug).bind(id).bind(tail_id).fetch_one(&mut *tx).await?;
    if body_bytes > max_bytes as i64 {
        return Err(StoreError::ReadLimit);
    }
    let mut entries = sqlx::query_as("SELECT * FROM content.posts WHERE board=$1 AND thread_id=$2 AND NOT deleted AND ($3::bigint IS NULL OR id=$2 OR id>$3) ORDER BY id LIMIT 1001")
        .bind(slug)
        .bind(id)
        .bind(tail_id)
        .fetch_all(&mut *tx)
        .await?;
    let expected = 1 + if tail { tail_size } else { replies };
    if entries.len() != expected {
        return Err(StoreError::Invalid("Incomplete thread snapshot."));
    }
    crate::post_media::load(&mut tx, &mut entries).await?;
    let unique_ips = sqlx::query_scalar("SELECT content.unique_posters($1,$2)")
        .bind(slug)
        .bind(id)
        .fetch_one(&mut *tx)
        .await?;
    let navigation_boards = snapshot_navigation(&mut tx, include_navigation).await?;
    tx.commit().await?;
    Ok(PageSnapshot {
        snapshot: ThreadSnapshot {
            board,
            thread: metadata,
            posts: entries,
            replies,
            images,
            unique_ips,
            tail_size,
            tail_id,
        },
        navigation_boards,
    })
}
pub async fn threads(
    pool: &PgPool,
    slug: &str,
    offset: i64,
    limit: i64,
) -> Result<Vec<Thread>, StoreError> {
    if !(0..=1000).contains(&offset) || !(1..=1000).contains(&limit) {
        return Err(StoreError::NotFound);
    }
    Ok(sqlx::query_as("SELECT * FROM content.visible_threads WHERE board=$1 AND NOT deleted AND archived_at IS NULL ORDER BY sticky DESC,bumped_at DESC,id DESC OFFSET $2 LIMIT $3").bind(slug).bind(offset).bind(limit).fetch_all(pool).await?)
}
pub async fn posts(pool: &PgPool, slug: &str, id: i64) -> Result<Vec<Post>, StoreError> {
    Ok(sqlx::query_as("SELECT p.* FROM content.posts p JOIN content.visible_threads t ON t.id=p.thread_id WHERE p.board=$1 AND p.thread_id=$2 AND NOT p.deleted AND NOT t.deleted ORDER BY p.id LIMIT 1001").bind(slug).bind(id).fetch_all(pool).await?)
}
pub async fn find_post(pool: &PgPool, slug: &str, id: i64) -> Result<Post, StoreError> {
    sqlx::query_as("SELECT p.* FROM content.posts p JOIN content.visible_threads t ON t.id=p.thread_id WHERE p.board=$1 AND p.id=$2 AND NOT p.deleted AND NOT t.deleted").bind(slug).bind(id).fetch_optional(pool).await?.ok_or(StoreError::NotFound)
}

pub const SEARCH_PAGE_SIZE: i64 = 10;
pub const SEARCH_MAX_PAGES: i64 = 10;
pub const SEARCH_SCAN_POSTS: i64 = 20_000;
pub const SEARCH_MATCHING_REPLIES_PER_THREAD: i64 = 5;
pub const SEARCH_COMMENT_CHARS: i64 = 1024;

pub struct SearchThread {
    pub board: Board,
    pub thread: Thread,
    pub posts: Vec<Post>,
    pub visible_images: i64,
}

pub struct SearchResult {
    pub threads: Vec<SearchThread>,
    pub offset: i64,
    pub nhits: i64,
}

#[derive(sqlx::FromRow)]
struct SearchHitRow {
    board: Option<String>,
    thread_id: Option<i64>,
    nhits: i64,
}

fn validate_search_request(
    query: &str,
    board: Option<&str>,
    offset: i64,
) -> Result<(), StoreError> {
    if query.is_empty()
        || query.contains('\0')
        || query.encode_utf16().count() > 512
        || !(0..SEARCH_PAGE_SIZE * SEARCH_MAX_PAGES).contains(&offset)
        || offset % SEARCH_PAGE_SIZE != 0
    {
        return Err(StoreError::Invalid("Invalid search request."));
    }
    if let Some(board) = board {
        board_domain::BoardSlug::parse(board)
            .map_err(|_| StoreError::Invalid("Invalid search board."))?;
    }
    Ok(())
}

/// Search a bounded window of visible public posts and group matches by thread.
/// The old client presents at most ten pages of ten thread results. We scan only
/// the newest SEARCH_SCAN_POSTS rows, then return the OP plus a bounded set of
/// matching replies for each selected thread.
pub async fn search(
    pool: &PgPool,
    query: &str,
    board: Option<&str>,
    offset: i64,
) -> Result<SearchResult, StoreError> {
    validate_search_request(query, board, offset)?;
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await?;

    let rows: Vec<SearchHitRow> = sqlx::query_as(
        "WITH candidates AS MATERIALIZED (\
            SELECT p.board,p.thread_id,p.id,p.subject,coalesce(p.wordfilter_search,p.comment) AS comment \
            FROM content.posts p \
            JOIN content.visible_threads t ON t.board=p.board AND t.id=p.thread_id \
            WHERE NOT p.deleted AND ($2::text IS NULL OR p.board=$2) \
            ORDER BY p.id DESC LIMIT $4\
         ), hits AS MATERIALIZED (\
            SELECT board,thread_id,max(id) AS match_id \
            FROM candidates \
            WHERE strpos(lower(subject || E'\\n' || comment), lower($1)) > 0 \
            GROUP BY board,thread_id\
         ), counted AS (SELECT count(*)::bigint AS nhits FROM hits), page AS (\
            SELECT board,thread_id,match_id FROM hits \
            ORDER BY match_id DESC,thread_id DESC OFFSET $3 LIMIT 10\
         ) \
         SELECT page.board,page.thread_id,counted.nhits \
         FROM counted LEFT JOIN page ON true \
         ORDER BY page.match_id DESC NULLS LAST,page.thread_id DESC NULLS LAST",
    )
    .bind(query)
    .bind(board)
    .bind(offset)
    .bind(SEARCH_SCAN_POSTS)
    .fetch_all(&mut *tx)
    .await?;
    let nhits = rows.first().map_or(0, |row| row.nhits);
    let selected: Vec<(String, i64)> = rows
        .into_iter()
        .filter_map(|row| row.board.zip(row.thread_id))
        .collect();
    if selected.is_empty() {
        tx.commit().await?;
        return Ok(SearchResult {
            threads: Vec::new(),
            offset,
            nhits,
        });
    }

    let ids: Vec<i64> = selected.iter().map(|(_, id)| *id).collect();
    let slugs: Vec<String> = selected.iter().map(|(board, _)| board.clone()).collect();
    let boards: Vec<Board> = sqlx::query_as("SELECT * FROM content.boards WHERE slug=ANY($1)")
        .bind(&slugs)
        .fetch_all(&mut *tx)
        .await?;
    let boards: std::collections::BTreeMap<_, _> = boards
        .into_iter()
        .map(|board| (board.slug.clone(), board))
        .collect();
    let threads: Vec<Thread> =
        sqlx::query_as("SELECT * FROM content.visible_threads WHERE id=ANY($1)")
            .bind(&ids)
            .fetch_all(&mut *tx)
            .await?;
    let threads: std::collections::BTreeMap<_, _> = threads
        .into_iter()
        .map(|thread| (thread.id, thread))
        .collect();

    let mut posts: Vec<Post> = sqlx::query_as(
        "SELECT p.id,p.board,p.thread_id,p.name,p.trip,p.poster_id,p.json_op_poster_id,p.capcode,p.country,p.country_name,p.board_flag,p.flag_name,\
                p.subject,p.image_spoiler,\
                CASE WHEN char_length(p.comment) <= $4 THEN p.comment \
                     WHEN strpos(lower(p.comment), lower($2)) > 0 THEN \
                       substring(p.comment FROM greatest(1, strpos(lower(p.comment), lower($2)) - ($4 / 4)::integer) FOR $4::integer) \
                     ELSE left(p.comment, $4::integer) END AS comment,\
                p.dice_result,p.fortune_text,p.fortune_color,p.comment_format,p.staff_authorized_limits,p.wordfilter_payload,p.created_at,p.deleted \
         FROM unnest($1::bigint[]) WITH ORDINALITY AS selected(thread_id,ord) \
         CROSS JOIN LATERAL (\
            SELECT picked.* FROM (\
                (SELECT op.* FROM content.posts op \
                 WHERE op.thread_id=selected.thread_id AND op.id=selected.thread_id AND NOT op.deleted) \
                UNION ALL \
                (SELECT reply.* FROM content.posts reply \
                 WHERE reply.thread_id=selected.thread_id AND reply.id<>selected.thread_id \
                   AND NOT reply.deleted \
                   AND strpos(lower(reply.subject || E'\\n' || coalesce(reply.wordfilter_search,reply.comment)), lower($2)) > 0 \
                 ORDER BY reply.id DESC LIMIT $3)\
            ) picked ORDER BY picked.id\
         ) p ORDER BY selected.ord,p.id",
    )
    .bind(&ids)
    .bind(query)
    .bind(SEARCH_MATCHING_REPLIES_PER_THREAD)
    .bind(SEARCH_COMMENT_CHARS)
    .fetch_all(&mut *tx)
    .await?;
    crate::post_media::load(&mut tx, &mut posts).await?;
    let mut posts_by_thread: std::collections::BTreeMap<i64, Vec<Post>> =
        std::collections::BTreeMap::new();
    for post in posts {
        posts_by_thread
            .entry(post.thread_id)
            .or_default()
            .push(post);
    }
    let images: Vec<(i64, i64)> = sqlx::query_as(
        "SELECT p.thread_id,count(*)::bigint FROM content.posts p \
         JOIN content.visible_post_media m ON m.post_id=p.id \
         WHERE p.thread_id=ANY($1) AND p.id<>p.thread_id AND NOT p.deleted AND NOT m.file_deleted \
         GROUP BY p.thread_id",
    )
    .bind(&ids)
    .fetch_all(&mut *tx)
    .await?;
    let images: std::collections::BTreeMap<_, _> = images.into_iter().collect();

    let mut results = Vec::with_capacity(selected.len());
    for (slug, id) in selected {
        let board = boards.get(&slug).cloned().ok_or(StoreError::NotFound)?;
        let thread = threads.get(&id).cloned().ok_or(StoreError::NotFound)?;
        let posts = posts_by_thread.remove(&id).unwrap_or_default();
        if posts.first().is_none_or(|post| post.id != id) {
            return Err(StoreError::Invalid("Incomplete search result."));
        }
        results.push(SearchThread {
            board,
            thread,
            posts,
            visible_images: images.get(&id).copied().unwrap_or(0),
        });
    }
    tx.commit().await?;
    Ok(SearchResult {
        threads: results,
        offset,
        nhits,
    })
}

pub struct PostSnapshot {
    pub board: Board,
    pub thread: Thread,
    pub post: Post,
}

/// Resolve one visible post and its approved attachment from the same snapshot
/// as the board policy and thread state, without loading other comment bodies.
pub async fn post_snapshot(pool: &PgPool, slug: &str, id: i64) -> Result<PostSnapshot, StoreError> {
    board_domain::BoardSlug::parse(slug).map_err(|_| StoreError::NotFound)?;
    if id <= 0 {
        return Err(StoreError::NotFound);
    }
    let mut tx = pool.begin().await?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
        .execute(&mut *tx)
        .await?;
    let board = sqlx::query_as("SELECT * FROM content.boards WHERE slug=$1")
        .bind(slug)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(StoreError::NotFound)?;
    let mut post: Post = sqlx::query_as("SELECT p.* FROM content.posts p JOIN content.visible_threads t ON t.id=p.thread_id AND t.board=p.board WHERE p.board=$1 AND p.id=$2 AND NOT p.deleted AND NOT t.deleted")
        .bind(slug)
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?
        .ok_or(StoreError::NotFound)?;
    let thread = sqlx::query_as(
        "SELECT * FROM content.visible_threads WHERE board=$1 AND id=$2 AND NOT deleted",
    )
    .bind(slug)
    .bind(post.thread_id)
    .fetch_optional(&mut *tx)
    .await?
    .ok_or(StoreError::NotFound)?;
    crate::post_media::load(&mut tx, std::slice::from_mut(&mut post)).await?;
    tx.commit().await?;
    Ok(PostSnapshot {
        board,
        thread,
        post,
    })
}

// Fetch only the bounded hash, with its board/post visibility check in the same
// statement. The mutation path repeats this read after acquiring its board lock.
pub(crate) const DELETION_HASH: &str = "SELECT d.password_hash FROM post_secrets.deletion d JOIN content.posts p ON p.id=d.post_id JOIN content.visible_threads t ON t.id=p.thread_id AND t.board=p.board WHERE p.board=$1 AND p.id=$2 AND NOT p.deleted AND NOT t.deleted";

pub async fn deletion_hash(pool: &PgPool, slug: &str, id: i64) -> Result<String, StoreError> {
    sqlx::query_scalar(DELETION_HASH)
        .bind(slug)
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or(StoreError::NotFound)
}

/// Missing historical password state cannot establish OP ownership.
pub(crate) const OP_DELETION_HASH: &str = "SELECT d.password_hash FROM post_secrets.deletion d JOIN content.posts p ON p.id=d.post_id JOIN content.visible_threads t ON t.id=p.thread_id WHERE p.board=$1 AND p.id=$2 AND p.id=p.thread_id AND NOT p.deleted";

pub async fn op_deletion_hash(
    pool: &PgPool,
    slug: &str,
    parent: i64,
) -> Result<Option<String>, StoreError> {
    Ok(sqlx::query_scalar(OP_DELETION_HASH)
        .bind(slug)
        .bind(parent)
        .fetch_optional(pool)
        .await?)
}

#[cfg(test)]
mod search_tests {
    use super::*;

    #[test]
    fn search_request_uses_the_source_page_window_and_hash_bound() {
        assert!(validate_search_request("owned", None, 0).is_ok());
        assert!(validate_search_request("owned", Some("g"), 90).is_ok());
        for offset in [-10, 5, 100] {
            assert!(validate_search_request("owned", None, offset).is_err());
        }
        assert!(validate_search_request("", None, 0).is_err());
        assert!(validate_search_request("owned", Some("../j"), 0).is_err());
        assert!(validate_search_request(&"x".repeat(513), None, 0).is_err());
        assert!(validate_search_request(&"😀".repeat(256), None, 0).is_ok());
        assert!(validate_search_request(&"😀".repeat(257), None, 0).is_err());
    }
}
