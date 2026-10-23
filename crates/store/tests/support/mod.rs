//! Owned database fixtures. Posting calls use production admission; teardown
//! is limited to fixture-owned rows and keeps production guards enabled.
#![allow(dead_code)]
use board_domain::{anonymous_session::Capability, poster_id::PosterIdKey};
use board_store::{NewPost, PostIdentityKeys, PostMetadata, PostingContext, StoreError};
use chrono::{DateTime, Utc};
use sqlx::{Acquire, PgPool, Postgres, Transaction};
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{Arc, Mutex, OnceLock},
};

struct Identity {
    key: Arc<PosterIdKey>,
    actors: Vec<[u8; 32]>,
}
static FIXTURES: OnceLock<Mutex<HashMap<String, Identity>>> = OnceLock::new();
fn fixtures() -> &'static Mutex<HashMap<String, Identity>> {
    FIXTURES.get_or_init(|| Mutex::new(HashMap::new()))
}
pub fn fresh_key() -> PosterIdKey {
    let bytes = Capability::generate().unwrap().storage_hash();
    PosterIdKey::parse(&bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()).unwrap()
}
pub fn key(board: &str) -> Arc<PosterIdKey> {
    fixtures()
        .lock()
        .unwrap()
        .entry(board.into())
        .or_insert_with(|| Identity {
            key: Arc::new(fresh_key()),
            actors: Vec::new(),
        })
        .key
        .clone()
}
pub fn peer() -> IpAddr {
    "192.0.2.201".parse().unwrap()
}
fn record_actor(board: &str, key: &PosterIdKey, peer: IpAddr) {
    let actor = *key.public_posting_rate_identity(peer).as_bytes();
    let mut all = fixtures().lock().unwrap();
    let entry = all.get_mut(board).expect("Owned fixture key registered");
    if !entry.actors.contains(&actor) {
        entry.actors.push(actor);
    }
}
/// Begin teardown while retaining locks on only the fixture's owned parents.
pub async fn begin_cleanup(owner: &PgPool, boards: &[String]) -> Transaction<'static, Postgres> {
    let mut tx = owner.begin().await.unwrap();
    // A rejected writer drops its transaction, queuing an asynchronous
    // rollback. Wait for its parent locks before touching private rows:
    // deletion guards deliberately reenter board locks with NOWAIT.
    // Keep canonical board -> thread order and retain locks until the caller
    // commits all cleanup deletes, rather than using separate autocommits.
    sqlx::query("SELECT slug FROM content.boards WHERE slug=ANY($1) ORDER BY slug FOR UPDATE")
        .bind(boards)
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM content.threads WHERE board=ANY($1) ORDER BY board,id FOR UPDATE")
        .bind(boards)
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    tx
}
/// Lock owned session parents before teardown cascades delete their memberships.
pub async fn begin_cleanup_with_sessions(
    owner: &PgPool,
    boards: &[String],
    tokens: &[[u8; 32]],
) -> Transaction<'static, Postgres> {
    let mut tx = begin_cleanup(owner, boards).await;
    // Expired-session collection locks session -> membership. Preserve that
    // order before deleting posts/reports, whose FK cascades lock memberships.
    // Only owned tokens are locked; concurrent mints can SKIP LOCKED sessions
    // while this transaction retains its canonical board -> thread locks.
    let tokens: Vec<&[u8]> = tokens.iter().map(|token| token.as_slice()).collect();
    sqlx::query("SELECT token_hash FROM post_secrets.anonymous_sessions WHERE token_hash=ANY($1::bytea[]) ORDER BY token_hash FOR UPDATE")
        .bind(tokens)
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    tx
}
/// Remove only this fixture's recorded posting actors from its owned board.
/// Deletion quotas, Robot9000 history and anonymous sessions are untouched.
/// Accept a pool or an existing cleanup transaction without committing it.
pub async fn cleanup_posting<'a>(owner: impl Acquire<'a, Database = Postgres>, board: &str) {
    let actors = fixtures()
        .lock()
        .unwrap()
        .get(board)
        .map(|f| f.actors.clone())
        .unwrap_or_default();
    let mut connection = owner.acquire().await.unwrap();
    for actor in actors {
        sqlx::query(
            "DELETE FROM post_secrets.posting_thread_actions WHERE board=$1 AND actor_hash=$2",
        )
        .bind(board)
        .bind(actor.as_slice())
        .execute(&mut *connection)
        .await
        .unwrap();
    }
}
pub async fn create_post(
    pool: &PgPool,
    board: &str,
    parent: i64,
    post: &NewPost,
) -> Result<i64, StoreError> {
    create_post_with_attachment(pool, board, parent, post, None).await
}
pub async fn create_post_with_attachment(
    pool: &PgPool,
    board: &str,
    parent: i64,
    post: &NewPost,
    attachment: Option<&board_store::post_media::NewAttachment>,
) -> Result<i64, StoreError> {
    create_post_with_attachment_at(pool, board, parent, post, attachment, Utc::now()).await
}
pub async fn create_post_with_attachment_at(
    pool: &PgPool,
    board: &str,
    parent: i64,
    post: &NewPost,
    attachment: Option<&board_store::post_media::NewAttachment>,
    request_start: DateTime<Utc>,
) -> Result<i64, StoreError> {
    create_post_with_context(
        pool,
        board,
        parent,
        post,
        attachment,
        PostingContext {
            request_start,
            peer: Some(peer()),
            op_password_proof: None,
        },
    )
    .await
}
pub async fn create_post_with_context(
    pool: &PgPool,
    board: &str,
    parent: i64,
    post: &NewPost,
    attachment: Option<&board_store::post_media::NewAttachment>,
    context: PostingContext,
) -> Result<i64, StoreError> {
    create_post_with_context_and_key(pool, board, parent, post, attachment, context, None).await
}
pub async fn create_post_with_context_and_key(
    pool: &PgPool,
    board: &str,
    parent: i64,
    post: &NewPost,
    attachment: Option<&board_store::post_media::NewAttachment>,
    context: PostingContext,
    tripcode: Option<&board_domain::identity::SecureKey>,
) -> Result<i64, StoreError> {
    create_post_with_metadata(
        pool,
        board,
        parent,
        post,
        attachment,
        context,
        PostMetadata {
            keys: PostIdentityKeys {
                tripcode,
                poster_id: None,
            },
            spoiler: false,
            country_database: None,
            flag: "",
            options: "",
            drawing: None,
        },
    )
    .await
}
pub async fn create_post_with_metadata(
    pool: &PgPool,
    board: &str,
    parent: i64,
    post: &NewPost,
    attachment: Option<&board_store::post_media::NewAttachment>,
    mut context: PostingContext,
    metadata: PostMetadata<'_>,
) -> Result<i64, StoreError> {
    let owned = key(board);
    let poster_id = metadata.keys.poster_id.unwrap_or(&owned);
    // Legacy fixtures supplied no transport. The harness owns this stable peer.
    context.peer = Some(context.peer.unwrap_or_else(peer));
    record_actor(board, poster_id, context.peer.unwrap());
    board_store::create_post_with_metadata(
        pool,
        board,
        parent,
        post,
        attachment,
        context,
        PostMetadata {
            keys: PostIdentityKeys {
                tripcode: metadata.keys.tripcode,
                poster_id: Some(poster_id),
            },
            spoiler: metadata.spoiler,
            country_database: metadata.country_database,
            flag: metadata.flag,
            options: metadata.options,
            drawing: metadata.drawing,
        },
    )
    .await
}
pub fn share_key(board: &str, source: &str) {
    let key = key(source);
    fixtures().lock().unwrap().insert(
        board.into(),
        Identity {
            key,
            actors: Vec::new(),
        },
    );
}
pub async fn create_post_with_anonymous_session(
    pool: &PgPool,
    board: &str,
    parent: i64,
    post: &NewPost,
    attachment: Option<&board_store::post_media::NewAttachment>,
    mut context: board_store::AnonymousPostingContext,
    metadata: PostMetadata<'_>,
) -> Result<i64, StoreError> {
    let owned = key(board);
    let poster_id = metadata.keys.poster_id.unwrap_or(&owned);
    context.posting.peer = Some(context.posting.peer.unwrap_or_else(peer));
    record_actor(board, poster_id, context.posting.peer.unwrap());
    board_store::create_post_with_anonymous_session(
        pool,
        board,
        parent,
        post,
        attachment,
        context,
        PostMetadata {
            keys: PostIdentityKeys {
                tripcode: metadata.keys.tripcode,
                poster_id: Some(poster_id),
            },
            spoiler: metadata.spoiler,
            country_database: metadata.country_database,
            flag: metadata.flag,
            options: metadata.options,
            drawing: metadata.drawing,
        },
    )
    .await
}
/// Reset only the recorded actors' posting rows between independent fixture
/// scenarios, for example before tests with deliberately historical clocks.
pub async fn reset_posting_history(owner: &PgPool, board: &str) {
    let actors = fixtures()
        .lock()
        .unwrap()
        .get(board)
        .map(|f| f.actors.clone())
        .unwrap_or_default();
    for actor in actors {
        sqlx::query("DELETE FROM post_secrets.posting_history WHERE board=$1 AND actor_hash=$2")
            .bind(board)
            .bind(actor.as_slice())
            .execute(owner)
            .await
            .unwrap();
    }
    cleanup_posting(owner, board).await;
}

/// Declared normalized metadata for independent synthetic upload fixtures.
/// These tests do not decode bytes; real worker manifests are tested separately.
pub fn synthetic_upload_variants(job: &str) -> board_store::media_assets::OutputVariants {
    assert!(job.len() == 32 && job.bytes().all(|b| b.is_ascii_hexdigit()));
    board_store::media_assets::OutputVariants {
        md5: job.into(),
        thumbnail: board_store::media_assets::OutputMetadata {
            sha256: "e".repeat(64),
            bytes: 50,
            width: 1,
            height: 1,
        },
    }
}

/// Give independent fixture posts distinct content without clearing admission history.
pub fn distinct_comment(prefix: &str) -> String {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    format!(
        "{prefix} {}",
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    )
}

/// Synthetic unchanged SWF metadata for database authority tests. This does not
/// claim parser or playback coverage; the owned VM/browser suites supply those.
pub async fn owned_swf(
    board: &str,
    digest: u8,
    filename: &str,
) -> board_store::post_media::NewAttachment {
    use board_store::{
        media::MediaQueue,
        media_assets::{InputComponent, InputReceipt, OutputMetadata},
        media_intake::IntakeStore,
    };
    let intake = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let upload = intake
        .reserve_image(filename, board, 0, false)
        .await
        .unwrap();
    intake
        .begin_upload(&upload.id, &upload.capability)
        .await
        .unwrap();
    intake
        .finish_upload(&upload.id, &upload.capability, 100)
        .await
        .unwrap();
    let job = queue.claim().await.unwrap().unwrap();
    assert_eq!(job.id, upload.id, "Use an idle disposable queue");
    let token = job.lease_token.as_deref().unwrap();
    let input = InputReceipt {
        component: InputComponent::Upload,
        sha256: format!("{digest:02x}").repeat(32),
        bytes: 100,
        md5: [digest; 16],
    };
    let output = OutputMetadata {
        sha256: input.sha256.clone(),
        bytes: 100,
        width: 200,
        height: 100,
    };
    let asset = queue
        .prepare_swf_output(&job.id, token, &output, &input)
        .await
        .unwrap();
    queue
        .record_input_receipt(&job.id, token, &asset.id, &input)
        .await
        .unwrap();
    queue
        .approve_output_with_input(&job.id, token, &asset.id, &input)
        .await
        .unwrap();
    board_store::post_media::NewAttachment {
        upload,
        spoiler: false,
    }
}

/// Borrow an empty native board because media intake deliberately accepts only
/// the source inventory. Preserve its entire policy and restore it after the
/// owned rows are removed; do not broaden the production intake allowlist.
pub async fn borrow_empty_source_board(
    owner: &PgPool,
    slug: &str,
    settings: serde_json::Value,
) -> String {
    let mut tx = owner.begin().await.unwrap();
    let before: String = sqlx::query_scalar(
        "SELECT to_jsonb(b)::text FROM content.boards b WHERE slug=$1 FOR UPDATE",
    )
    .bind(slug)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    let empty:bool=sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM content.threads WHERE board=$1) AND NOT EXISTS(SELECT 1 FROM content.posts WHERE board=$1)").bind(slug).fetch_one(&mut *tx).await.unwrap();
    assert!(empty, "Only borrow an empty source board");
    let temporary: String =
        sqlx::query_scalar("SELECT 'zz'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Owned temporary defaults','Synthetic',2000,100,100,100,10)").bind(&temporary).execute(&mut *tx).await.unwrap();
    let defaults: serde_json::Value =
        sqlx::query_scalar("SELECT to_jsonb(b) FROM content.boards b WHERE slug=$1")
            .bind(&temporary)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    sqlx::query("DELETE FROM content.boards WHERE slug=$1")
        .bind(temporary)
        .execute(&mut *tx)
        .await
        .unwrap();
    let mut policy = defaults.as_object().unwrap().clone();
    policy.extend(settings.as_object().unwrap().clone());
    policy.insert("slug".into(), slug.into());
    replace_source_policy(&mut tx, slug, &serde_json::Value::Object(policy)).await;
    tx.commit().await.unwrap();
    before
}
async fn replace_source_policy(
    tx: &mut Transaction<'_, Postgres>,
    slug: &str,
    policy: &serde_json::Value,
) {
    let columns:String=sqlx::query_scalar("SELECT string_agg(quote_ident(attname),',' ORDER BY attnum) FROM pg_attribute WHERE attrelid='content.boards'::regclass AND attnum>0 AND NOT attisdropped AND attname<>'slug'").fetch_one(&mut **tx).await.unwrap();
    let query = format!(
        "UPDATE content.boards SET ({columns})=(SELECT {columns} FROM jsonb_populate_record(NULL::content.boards,$2::jsonb)) WHERE slug=$1"
    );
    assert_eq!(
        sqlx::query(sqlx::AssertSqlSafe(query))
            .bind(slug)
            .bind(policy)
            .execute(&mut **tx)
            .await
            .unwrap()
            .rows_affected(),
        1
    );
}
pub async fn restore_empty_source_board(owner: &PgPool, slug: &str, before: &str) {
    let mut tx = owner.begin().await.unwrap();
    let title: String =
        sqlx::query_scalar("SELECT title FROM content.boards WHERE slug=$1 FOR UPDATE")
            .bind(slug)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert!(
        title.starts_with("Owned "),
        "Do not replace an unrelated board policy"
    );
    let empty:bool=sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM content.threads WHERE board=$1) AND NOT EXISTS(SELECT 1 FROM content.posts WHERE board=$1)").bind(slug).fetch_one(&mut *tx).await.unwrap();
    assert!(
        empty,
        "Remove only the owned fixture rows before restoring policy"
    );
    let policy: serde_json::Value = serde_json::from_str(before).unwrap();
    replace_source_policy(&mut tx, slug, &policy).await;
    let after: serde_json::Value =
        sqlx::query_scalar("SELECT to_jsonb(b) FROM content.boards b WHERE slug=$1")
            .bind(slug)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(after, policy);
    tx.commit().await.unwrap();
}
