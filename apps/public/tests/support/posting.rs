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

fn transport(router: axum::Router) -> axum::Router {
    router.layer(axum::middleware::from_fn(
        |mut request: axum::extract::Request, next: axum::middleware::Next| async move {
            if request
                .extensions()
                .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
                .is_none()
            {
                request.extensions_mut().insert(axum::extract::ConnectInfo(
                    std::net::SocketAddr::new(peer(), 40000),
                ));
            }
            next.run(request).await
        },
    ))
}
fn options(
    board: &str,
    origin: String,
    production: bool,
    media: Option<board_config::PublicMediaSettings>,
    limits: board_config::PublicRequestLimits,
) -> board_public::PublicRouterOptions {
    board_public::PublicRouterOptions {
        origin,
        production,
        media,
        limits,
        proxy_uid: None,
        poster_id_key: Some(key(board)),
        tripcode_key: None,
        country_database: None,
    }
}
pub fn router(pool: PgPool, board: &str, origin: String, production: bool) -> axum::Router {
    routers(pool, board, origin, production).0
}
pub fn routers(
    pool: PgPool,
    board: &str,
    origin: String,
    production: bool,
) -> (axum::Router, axum::Router) {
    routers_with_media(pool, board, origin, production, None)
}
pub fn routers_with_media(
    pool: PgPool,
    board: &str,
    origin: String,
    production: bool,
    media: Option<board_config::PublicMediaSettings>,
) -> (axum::Router, axum::Router) {
    routers_with_limits(
        pool,
        board,
        origin,
        production,
        media,
        board_config::PublicRequestLimits::default(),
    )
}
pub fn routers_with_limits(
    pool: PgPool,
    board: &str,
    origin: String,
    production: bool,
    media: Option<board_config::PublicMediaSettings>,
    limits: board_config::PublicRequestLimits,
) -> (axum::Router, axum::Router) {
    let (web, api) =
        board_public::routers_with_options(pool, options(board, origin, production, media, limits));
    (transport(web), transport(api))
}
pub fn observed_routers_with_limits(
    pool: PgPool,
    board: &str,
    origin: String,
    production: bool,
    api_enabled: bool,
    media: Option<board_config::PublicMediaSettings>,
    limits: board_config::PublicRequestLimits,
) -> (board_observe::Metrics, axum::Router, axum::Router) {
    let (metrics, web, api) = board_public::observed_routers_with_options(
        pool,
        api_enabled,
        options(board, origin, production, media, limits),
    );
    (metrics, transport(web), transport(api))
}

/// Share a deployment identity across boards participating in one fixture.
pub fn register_alias(board: &str, primary: &str) {
    let primary_key = key(primary);
    let mut all = fixtures().lock().unwrap();
    assert!(
        !all.contains_key(board),
        "Register fixture aliases before posting"
    );
    all.insert(
        board.into(),
        Identity {
            key: primary_key,
            actors: Vec::new(),
        },
    );
}
/// Separate independent admission setups without changing deletion/session/R9K state.
pub async fn cleanup_actor_posting(owner: &PgPool, board: &str, key: &PosterIdKey, peer: IpAddr) {
    let actor = key.public_posting_rate_identity(peer);
    for query in [
        "DELETE FROM post_secrets.posting_history WHERE board=$1 AND actor_hash=$2",
        "DELETE FROM post_secrets.posting_thread_actions WHERE board=$1 AND actor_hash=$2",
    ] {
        sqlx::query(query)
            .bind(board)
            .bind(actor.as_bytes().as_slice())
            .execute(owner)
            .await
            .unwrap();
    }
}
