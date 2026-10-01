#![forbid(unsafe_code)]

mod api;
mod api_http;
pub mod catalog;
mod derefer;
mod handlers;
mod intake;
mod legacy_form;
mod legacy_report;
mod native_board_snapshot;
mod native_thread_stats;
mod native_updater_snapshot;
mod output;
mod post_preferences;
mod post_receipts;
mod posting_form;
mod posting_response;
mod rss;
mod security;
pub mod themes;
pub mod transport;
mod ui_assets;
mod uploads;
mod views;
use axum::{
    Router,
    extract::DefaultBodyLimit,
    middleware,
    routing::{get, post},
};
use sqlx::PgPool;
use std::sync::Arc;

#[derive(Clone)]
pub struct AppState {
    pool: PgPool,
    origin: String,
    production: bool,
    limits: Arc<security::Limits>,
    media: Option<intake::IntakeClient>,
    proxy_uid: Option<u32>,
    poster_id_key: Option<Arc<board_domain::poster_id::PosterIdKey>>,
    tripcode_key: Option<Arc<board_domain::identity::SecureKey>>,
    country_database: Option<Arc<board_domain::country::CountryDatabase>>,
}

pub struct PublicRouterOptions {
    pub origin: String,
    pub production: bool,
    pub media: Option<board_config::PublicMediaSettings>,
    pub limits: board_config::PublicRequestLimits,
    pub proxy_uid: Option<u32>,
    pub poster_id_key: Option<Arc<board_domain::poster_id::PosterIdKey>>,
    pub tripcode_key: Option<Arc<board_domain::identity::SecureKey>>,
    pub country_database: Option<Arc<board_domain::country::CountryDatabase>>,
}

pub fn router(pool: PgPool, origin: String, production: bool) -> Router {
    routers(pool, origin, production).0
}

/// Build the serving routers and a process-local, fixed-label metrics snapshot.
pub fn observed_routers(
    pool: PgPool,
    origin: String,
    production: bool,
    api_enabled: bool,
) -> (board_observe::Metrics, Router, Router) {
    observed_routers_with_media(pool, origin, production, api_enabled, None)
}

pub fn observed_routers_with_media(
    pool: PgPool,
    origin: String,
    production: bool,
    api_enabled: bool,
    media: Option<board_config::PublicMediaSettings>,
) -> (board_observe::Metrics, Router, Router) {
    observed_routers_with_limits(
        pool,
        origin,
        production,
        api_enabled,
        media,
        board_config::PublicRequestLimits::default(),
    )
}

pub fn observed_routers_with_limits(
    pool: PgPool,
    origin: String,
    production: bool,
    api_enabled: bool,
    media: Option<board_config::PublicMediaSettings>,
    limits: board_config::PublicRequestLimits,
) -> (board_observe::Metrics, Router, Router) {
    observed_routers_with_proxy(pool, origin, production, api_enabled, media, limits, None)
}

pub fn observed_routers_with_proxy(
    pool: PgPool,
    origin: String,
    production: bool,
    api_enabled: bool,
    media: Option<board_config::PublicMediaSettings>,
    limits: board_config::PublicRequestLimits,
    proxy_uid: Option<u32>,
) -> (board_observe::Metrics, Router, Router) {
    observed_routers_with_options(
        pool,
        api_enabled,
        PublicRouterOptions {
            origin,
            production,
            media,
            limits,
            proxy_uid,
            poster_id_key: None,
            tripcode_key: None,
            country_database: None,
        },
    )
}

pub fn observed_routers_with_options(
    pool: PgPool,
    api_enabled: bool,
    options: PublicRouterOptions,
) -> (board_observe::Metrics, Router, Router) {
    use board_observe::{Listener, Metrics, Pool, PoolSample};
    let mut metrics = Metrics::new();
    let observed_pool = pool.clone();
    metrics
        .register_pool(Pool::Public, move || PoolSample {
            size: observed_pool.size(),
            idle: observed_pool.num_idle(),
            max: observed_pool.options().get_max_connections(),
        })
        .expect("one pool registered before sharing metrics");
    let (public, api) = routers_with_options(pool, options);
    let public = metrics.layer(public, Listener::Public);
    let api = if api_enabled {
        metrics.layer(api, Listener::Api)
    } else {
        api
    };
    (metrics, public, api)
}

pub fn routers(pool: PgPool, origin: String, production: bool) -> (Router, Router) {
    routers_with_media(pool, origin, production, None)
}

pub fn routers_with_media(
    pool: PgPool,
    origin: String,
    production: bool,
    media: Option<board_config::PublicMediaSettings>,
) -> (Router, Router) {
    routers_with_limits(
        pool,
        origin,
        production,
        media,
        board_config::PublicRequestLimits::default(),
    )
}

pub fn routers_with_limits(
    pool: PgPool,
    origin: String,
    production: bool,
    media: Option<board_config::PublicMediaSettings>,
    limits: board_config::PublicRequestLimits,
) -> (Router, Router) {
    routers_with_proxy(pool, origin, production, media, limits, None)
}

fn routers_with_proxy(
    pool: PgPool,
    origin: String,
    production: bool,
    media: Option<board_config::PublicMediaSettings>,
    limits: board_config::PublicRequestLimits,
    proxy_uid: Option<u32>,
) -> (Router, Router) {
    routers_with_options(
        pool,
        PublicRouterOptions {
            origin,
            production,
            media,
            limits,
            proxy_uid,
            poster_id_key: None,
            tripcode_key: None,
            country_database: None,
        },
    )
}

pub fn routers_with_options(pool: PgPool, options: PublicRouterOptions) -> (Router, Router) {
    let PublicRouterOptions {
        origin,
        production,
        media,
        limits,
        proxy_uid,
        tripcode_key,
        country_database,
        poster_id_key,
    } = options;
    assert!(
        !production || media.is_none(),
        "Production media is not qualified"
    );
    let state = AppState {
        tripcode_key,
        country_database,
        poster_id_key,
        pool,
        origin,
        production,
        limits: Arc::new(security::Limits::new(limits)),
        media: media.map(|settings| intake::IntakeClient { settings }),
        proxy_uid,
    };
    let mut public = Router::new()
        .merge(themes::routes_with_limits(
            state.origin.clone(),
            state.production,
            state.limits.clone(),
        ))
        .merge(ui_assets::routes())
        .route("/", get(handlers::home))
        .route("/derefer", get(derefer::get))
        .route("/healthz", get(|| async { "ok" }))
        .route("/readyz", get(handlers::ready))
        .route("/static/board.css", get(handlers::css))
        .route("/boards.json", get(api::boards))
        .route("/_watch/{board}/thread/{key}", get(api::watcher_thread))
        .route(
            "/_watch/{board}/page/{key}",
            get(native_board_snapshot::get),
        )
        .route("/_watch/boards", get(native_board_snapshot::directory))
        .route(
            "/_watch/{board}/thread/{key}/stats",
            get(native_thread_stats::get),
        )
        .route(
            "/_watch/{board}/post/{key}",
            get(native_updater_snapshot::get_preview),
        )
        .route(
            "/_watch/{board}/thread/{key}/posts",
            get(native_updater_snapshot::get),
        )
        .route(
            "/_watch/{board}/thread/{key}/posts-tail",
            get(native_updater_snapshot::get_tail),
        )
        .route("/_watch/{board}/catalog.json", get(watcher_catalog::get))
        .route("/{board}", get(handlers::board_redirect))
        .route("/{board}/", get(handlers::board_index))
        .route("/{board}/thread/{key}", get(handlers::thread))
        .route("/{board}/post/{id}", get(handlers::quote))
        .route("/{board}/post", post(handlers::post))
        .route(
            "/{board}/imgboard.php",
            get(legacy_report::get).post(legacy_form::submit),
        )
        .route("/{board}/delete", post(handlers::delete))
        .route("/{board}/report", post(handlers::report));
    if state.media.is_some() {
        public = public
            .route(
                "/{board}/upload",
                post(uploads::upload).layer(DefaultBodyLimit::max(8_388_608 + 16_384)),
            )
            .route("/{board}/upload/status", post(uploads::status))
            .route("/{board}/upload/cancel", post(uploads::cancel));
    }
    let public = public
        .route("/{board}/{page}", get(handlers::page))
        .fallback(handlers::not_found)
        // A 16,000-character comment can occupy 192,000 bytes when four-byte
        // UTF-8 characters are percent-encoded. Bound the collected body too.
        .layer(DefaultBodyLimit::max(262_144))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            security::protect,
        ))
        .layer(middleware::from_fn(board_http::retain_response_body))
        .with_state(state.clone());
    let mut api_state = state;
    // The separate JSON listener cannot post and retains its own TCP transport.
    api_state.proxy_uid = None;
    let api = api_http::router(api_state);
    (public, api)
}

pub async fn media_ready(settings: &board_config::PublicMediaSettings) -> Result<(), &'static str> {
    intake::IntakeClient {
        settings: settings.clone(),
    }
    .ready()
    .await
    .map_err(|_| "Media intake is unavailable.")
}

mod proxy_peer;
mod watcher_catalog;
