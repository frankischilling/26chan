//! Identity-bearing fixture calls. These exercise the production admission path;
//! no production policy or database state is changed by the posting helpers.
#![allow(dead_code)]
use board_domain::{anonymous_session::Capability, poster_id::PosterIdKey};
use board_store::{NewPost, PostIdentityKeys, PostMetadata, PostingContext, StoreError};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
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
/// Remove only this fixture's recorded posting actors from its owned board.
/// Deletion quotas, Robot9000 history and anonymous sessions are untouched.
pub async fn cleanup_posting(owner: &PgPool, board: &str) {
    let actors = fixtures()
        .lock()
        .unwrap()
        .get(board)
        .map(|f| f.actors.clone())
        .unwrap_or_default();
    for actor in actors {
        sqlx::query(
            "DELETE FROM post_secrets.posting_thread_actions WHERE board=$1 AND actor_hash=$2",
        )
        .bind(board)
        .bind(actor.as_slice())
        .execute(owner)
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
