#![forbid(unsafe_code)]

use axum::{
    Router,
    extract::{Request, State},
    middleware::{self, Next},
    response::Response,
};
use std::{
    ffi::OsString,
    fmt::{self, Write},
    net::SocketAddr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};

pub struct Config {
    bind: SocketAddr,
    token: String,
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("bind", &self.bind)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConfigError;

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("metrics requires a loopback address with a nonzero port and a 64-character lowercase hexadecimal token")
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    pub fn parse(bind: Option<&str>, token: Option<&str>) -> Result<Option<Self>, ConfigError> {
        let (bind, token) = match (bind, token) {
            (None, None) => return Ok(None),
            (Some(bind), Some(token)) => (bind, token),
            _ => return Err(ConfigError),
        };
        let bind: SocketAddr = bind.parse().map_err(|_| ConfigError)?;
        if !bind.ip().is_loopback()
            || bind.port() == 0
            || token.len() != 64
            || !token
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(ConfigError);
        }
        Ok(Some(Self {
            bind,
            token: token.to_owned(),
        }))
    }

    pub fn from_env() -> Result<Option<Self>, ConfigError> {
        Self::from_os_values(
            std::env::var_os("METRICS_BIND_ADDR"),
            std::env::var_os("METRICS_TOKEN"),
        )
    }

    fn from_os_values(
        bind: Option<OsString>,
        token: Option<OsString>,
    ) -> Result<Option<Self>, ConfigError> {
        let bind = bind
            .as_ref()
            .map(|v| v.to_str().ok_or(ConfigError))
            .transpose()?;
        let token = token
            .as_ref()
            .map(|v| v.to_str().ok_or(ConfigError))
            .transpose()?;
        Self::parse(bind, token)
    }
}

#[cfg(test)]
mod tests;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Listener {
    Public,
    Api,
    Staff,
    Media,
}

impl Listener {
    const ALL: [Self; 4] = [Self::Public, Self::Api, Self::Staff, Self::Media];
    fn label(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::Api => "api",
            Self::Staff => "staff",
            Self::Media => "media",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pool {
    Public,
    StaffAuth,
    StaffContent,
    MediaRead,
}

impl Pool {
    const ALL: [Self; 4] = [
        Self::Public,
        Self::StaffAuth,
        Self::StaffContent,
        Self::MediaRead,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::Public => "public",
            Self::StaffAuth => "staff_auth",
            Self::StaffContent => "staff_content",
            Self::MediaRead => "media_read",
        }
    }
}

pub struct PoolSample {
    pub size: u32,
    pub idle: usize,
    pub max: u32,
}

#[derive(Debug)]
pub struct RegistrationError;

impl fmt::Display for RegistrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("metrics pools must be unique and registered before sharing metrics")
    }
}

impl std::error::Error for RegistrationError {}

#[derive(Default)]
struct Counters {
    installed: AtomicBool,
    responses: [AtomicU64; 6],
    write_rejections: AtomicU64,
    authorization_rejections: AtomicU64,
    capacity: AtomicU64,
    cancelled: AtomicU64,
    inflight: AtomicU64,
    duration_buckets: [AtomicU64; 6],
    duration_micros: AtomicU64,
}

type PoolCallback = Box<dyn Fn() -> PoolSample + Send + Sync>;

#[derive(Default)]
struct Inner {
    listeners: [Counters; 4],
    pools: [Option<PoolCallback>; 4],
}

/// Process-owned fixed-schema metrics. Register synchronous, bounded pool
/// snapshots before cloning or installing any layers. Snapshots must not perform
/// SQL, network or filesystem operations.
#[derive(Clone, Default)]
pub struct Metrics(Arc<Inner>);

impl Metrics {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn register_pool(
        &mut self,
        pool: Pool,
        callback: impl Fn() -> PoolSample + Send + Sync + 'static,
    ) -> Result<(), RegistrationError> {
        let inner = Arc::get_mut(&mut self.0).ok_or(RegistrationError)?;
        let slot = &mut inner.pools[pool as usize];
        if slot.is_some() {
            return Err(RegistrationError);
        }
        *slot = Some(Box::new(callback));
        Ok(())
    }

    /// Install outside application response layers to count their final status.
    /// Repeated installation with the same Metrics and Listener counts once.
    /// The response body and extensions pass through without replacement.
    pub fn layer(&self, router: Router, listener: Listener) -> Router {
        self.0.listeners[listener as usize]
            .installed
            .store(true, Ordering::Relaxed);
        router.layer(middleware::from_fn_with_state(
            (self.clone(), listener),
            account,
        ))
    }

    /// Bounded Prometheus text 0.0.4. Concurrent scrapes are approximate snapshots.
    pub fn render(&self) -> String {
        let mut text = String::with_capacity(16_384);
        for (name, kind, help) in [
            (
                "responses_total",
                "counter",
                "Completed handlers by final status class.",
            ),
            (
                "write_rejections_total",
                "counter",
                "POST PUT PATCH DELETE handlers returning status 400 or greater.",
            ),
            (
                "authorization_rejections_total",
                "counter",
                "Handlers returning 401 or 403 including CSRF rejections.",
            ),
            (
                "capacity_responses_total",
                "counter",
                "Handlers returning 429.",
            ),
            (
                "cancelled_total",
                "counter",
                "Handlers dropped before returning response headers.",
            ),
            (
                "handlers_inflight",
                "gauge",
                "Handlers awaiting response headers.",
            ),
            (
                "handler_duration_seconds",
                "histogram",
                "Completed handler time to response headers in seconds.",
            ),
        ] {
            writeln!(
                text,
                "# HELP board_http_{name} {help}\n# TYPE board_http_{name} {kind}"
            )
            .unwrap();
            for listener in Listener::ALL {
                let c = &self.0.listeners[listener as usize];
                if !c.installed.load(Ordering::Relaxed) {
                    continue;
                }
                let label = listener.label();
                match name {
                    "responses_total" => {
                        for (class, counter) in ["1xx", "2xx", "3xx", "4xx", "5xx", "other"]
                            .iter()
                            .zip(&c.responses)
                        {
                            writeln!(text, "board_http_{name}{{listener=\"{label}\",status_class=\"{class}\"}} {}", counter.load(Ordering::Relaxed)).unwrap();
                        }
                    }
                    "handler_duration_seconds" => {
                        let mut count = 0;
                        for (bound, counter) in ["0.01", "0.1", "1", "5", "10", "+Inf"]
                            .iter()
                            .zip(&c.duration_buckets)
                        {
                            count += counter.load(Ordering::Relaxed);
                            writeln!(text, "board_http_{name}_bucket{{listener=\"{label}\",le=\"{bound}\"}} {count}").unwrap();
                        }
                        writeln!(
                            text,
                            "board_http_{name}_sum{{listener=\"{label}\"}} {}",
                            c.duration_micros.load(Ordering::Relaxed) as f64 / 1_000_000.0
                        )
                        .unwrap();
                        writeln!(
                            text,
                            "board_http_{name}_count{{listener=\"{label}\"}} {count}"
                        )
                        .unwrap();
                    }
                    _ => {
                        let counter = match name {
                            "write_rejections_total" => &c.write_rejections,
                            "authorization_rejections_total" => &c.authorization_rejections,
                            "capacity_responses_total" => &c.capacity,
                            "cancelled_total" => &c.cancelled,
                            "handlers_inflight" => &c.inflight,
                            _ => unreachable!(),
                        };
                        writeln!(
                            text,
                            "board_http_{name}{{listener=\"{label}\"}} {}",
                            counter.load(Ordering::Relaxed)
                        )
                        .unwrap();
                    }
                }
            }
        }
        // Evaluate each callback once, so its three gauges share one sample.
        let pools: [_; 4] =
            std::array::from_fn(|i| self.0.pools[i].as_ref().map(|callback| callback()));
        for (name, help) in [
            ("connections", "Current pool connections."),
            ("idle", "Idle pool connections."),
            ("max_connections", "Configured pool connection limit."),
        ] {
            writeln!(
                text,
                "# HELP board_db_pool_{name} {help}\n# TYPE board_db_pool_{name} gauge"
            )
            .unwrap();
            for pool in Pool::ALL {
                if let Some(sample) = &pools[pool as usize] {
                    let value = match name {
                        "connections" => u64::from(sample.size),
                        "idle" => sample.idle as u64,
                        _ => u64::from(sample.max),
                    };
                    writeln!(
                        text,
                        "board_db_pool_{name}{{pool=\"{}\"}} {value}",
                        pool.label()
                    )
                    .unwrap();
                }
            }
        }
        text
    }
}

// Only code-installed layers add entries. No request-derived label data is kept.
#[derive(Clone, Default)]
struct Observed(Vec<(Metrics, Listener)>);

async fn account(
    State((metrics, listener)): State<(Metrics, Listener)>,
    mut request: Request,
    next: Next,
) -> Response {
    let observed = request.extensions_mut().get_or_insert_default::<Observed>();
    if observed
        .0
        .iter()
        .any(|(owner, seen)| *seen == listener && Arc::ptr_eq(&owner.0, &metrics.0))
    {
        return next.run(request).await;
    }
    observed.0.push((metrics.clone(), listener));
    let write = matches!(
        *request.method(),
        axum::http::Method::POST
            | axum::http::Method::PUT
            | axum::http::Method::PATCH
            | axum::http::Method::DELETE
    );
    let counters = &metrics.0.listeners[listener as usize];
    counters.inflight.fetch_add(1, Ordering::Relaxed);
    let mut guard = HandlerGuard {
        counters,
        start: Instant::now(),
        completed: false,
    };
    let response = next.run(request).await;
    let status = response.status().as_u16();
    let class = if (100..600).contains(&status) {
        usize::from(status / 100 - 1)
    } else {
        5
    };
    counters.responses[class].fetch_add(1, Ordering::Relaxed);
    if write && status >= 400 {
        counters.write_rejections.fetch_add(1, Ordering::Relaxed);
    }
    if matches!(status, 401 | 403) {
        counters
            .authorization_rejections
            .fetch_add(1, Ordering::Relaxed);
    }
    if status == 429 {
        counters.capacity.fetch_add(1, Ordering::Relaxed);
    }
    guard.complete();
    response
}

struct HandlerGuard<'a> {
    counters: &'a Counters,
    start: Instant,
    completed: bool,
}

impl HandlerGuard<'_> {
    fn complete(&mut self) {
        let micros = u64::try_from(self.start.elapsed().as_micros()).unwrap_or(u64::MAX);
        for (bound, counter) in [10_000, 100_000, 1_000_000, 5_000_000, 10_000_000, u64::MAX]
            .into_iter()
            .zip(&self.counters.duration_buckets)
        {
            if micros <= bound {
                // Disjoint atomic bins become cumulative at scrape time, keeping
                // every concurrent snapshot monotonic with +Inf equal to count.
                counter.fetch_add(1, Ordering::Relaxed);
                break;
            }
        }
        self.counters
            .duration_micros
            .fetch_add(micros, Ordering::Relaxed);
        self.completed = true;
    }
}

impl Drop for HandlerGuard<'_> {
    fn drop(&mut self) {
        self.counters.inflight.fetch_sub(1, Ordering::Relaxed);
        if !self.completed {
            self.counters.cancelled.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Separately bound private metrics socket. Acquire before database connections.
pub struct Endpoint(Option<(tokio::net::TcpListener, Config)>);

impl Endpoint {
    pub async fn bind(config: Option<Config>) -> std::io::Result<Self> {
        match config {
            Some(config) => Ok(Self(Some((
                tokio::net::TcpListener::bind(config.bind).await?,
                config,
            )))),
            None => Ok(Self(None)),
        }
    }

    /// End the paired service when either future completes. Dropping the exporter
    /// closes the listener and aborts its owned connections, including idle peers.
    pub async fn serve(
        self,
        metrics: Metrics,
        application: impl std::future::Future<Output = std::io::Result<()>>,
    ) -> std::io::Result<()> {
        match self.0 {
            Some((listener, config)) => {
                pair(
                    application,
                    serve_listener(listener, exporter_router(metrics, config)),
                )
                .await
            }
            None => application.await,
        }
    }
}

#[derive(Clone)]
struct ExportState {
    metrics: Metrics,
    token: Arc<str>,
    admission: Arc<tokio::sync::Semaphore>,
}

fn exporter_router(metrics: Metrics, config: Config) -> Router {
    let state = ExportState {
        metrics,
        token: config.token.into(),
        admission: Arc::new(tokio::sync::Semaphore::new(4)),
    };
    Router::new()
        .route("/metrics", axum::routing::get(scrape))
        .layer(middleware::from_fn_with_state(state.clone(), authorize))
        .layer(middleware::from_fn(board_http::retain_response_body))
        .with_state(state)
}

async fn authorize(State(state): State<ExportState>, request: Request, next: Next) -> Response {
    use axum::{
        http::{HeaderValue, StatusCode, header},
        response::IntoResponse,
    };
    use subtle::ConstantTimeEq;
    let mut values = request.headers().get_all(header::AUTHORIZATION).iter();
    let token = values
        .next()
        .and_then(|value| value.as_bytes().strip_prefix(b"Bearer "));
    let authenticated = values.next().is_none()
        && token.is_some_and(|token| {
            token.len() == state.token.len() && bool::from(token.ct_eq(state.token.as_bytes()))
        });
    let mut response = if authenticated {
        next.run(request).await
    } else {
        let mut response = StatusCode::UNAUTHORIZED.into_response();
        response
            .headers_mut()
            .insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        response
    };
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response
}

async fn scrape(State(state): State<ExportState>) -> Response {
    use axum::{
        http::{StatusCode, header},
        response::IntoResponse,
    };
    let Ok(permit) = state.admission.try_acquire_owned() else {
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    };
    board_http::hold_permit(
        (
            [(
                header::CONTENT_TYPE,
                "text/plain; version=0.0.4; charset=utf-8",
            )],
            state.metrics.render(),
        )
            .into_response(),
        permit,
    )
}

async fn serve_listener(listener: tokio::net::TcpListener, router: Router) -> std::io::Result<()> {
    serve_listener_with_deadline(listener, router, std::time::Duration::from_secs(10)).await
}

async fn serve_listener_with_deadline(
    listener: tokio::net::TcpListener,
    router: Router,
    deadline: std::time::Duration,
) -> std::io::Result<()> {
    let mut connections = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            accepted = listener.accept(), if connections.len() < 16 => {
                let (stream, _) = accepted?;
                let service = hyper_util::service::TowerToHyperService::new(router.clone());
                connections.spawn(async move {
                    // Peer disconnects and malformed requests are client errors.
                    // Do not log errors which could include incoming private data.
                    // A single request per connection and one total deadline
                    // bound idle headers, request handling and response writes.
                    let mut builder = hyper::server::conn::http1::Builder::new();
                    builder.keep_alive(false);
                    let _ = tokio::time::timeout(deadline, builder
                        .serve_connection(hyper_util::rt::TokioIo::new(stream), service)).await;
                });
            }
            completed = connections.join_next(), if !connections.is_empty() => {
                if completed.is_some_and(|result| result.is_err()) {
                    return Err(std::io::Error::other("metrics connection task failed"));
                }
            }
        }
    }
}

async fn pair(
    application: impl std::future::Future<Output = std::io::Result<()>>,
    exporter: impl std::future::Future<Output = std::io::Result<()>>,
) -> std::io::Result<()> {
    tokio::select! {
        result = application => result,
        result = exporter => result,
    }
}
