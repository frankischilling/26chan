use crate::AppState;
use axum::{
    extract::{Request, State},
    http::{HeaderValue, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr},
    sync::Mutex,
    time::{Duration, Instant},
};
use tokio::sync::Semaphore;

pub struct Limits {
    settings: board_config::PublicRequestLimits,
    active: std::sync::Arc<Semaphore>,
    output: board_http::ResponseBudget,
    pub hashes: std::sync::Arc<Semaphore>,
    pub uploads: Semaphore,
    peers: Mutex<HashMap<IpAddr, (Instant, u32)>>,
}

impl Limits {
    pub fn new(settings: board_config::PublicRequestLimits) -> Self {
        Self {
            settings,
            active: std::sync::Arc::new(Semaphore::new(settings.active_requests())),
            output: board_http::ResponseBudget::new(settings.response_buffer_bytes())
                .expect("validated response buffer capacity"),
            hashes: std::sync::Arc::new(Semaphore::new(settings.hash_operations())),
            uploads: Semaphore::new(settings.uploads()),
            peers: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn response_writer(&self, limit: usize) -> board_http::ResponseWriter {
        self.output.writer(self.response_limit(limit))
    }

    pub(crate) fn response_limit(&self, limit: usize) -> usize {
        limit.min(self.settings.response_bytes())
    }
}

#[derive(Clone, Copy)]
pub(crate) struct RequestStart(pub chrono::DateTime<chrono::Utc>);

#[derive(Clone, Copy)]
pub(crate) struct RequestPeer(pub Option<IpAddr>);

pub async fn protect(State(state): State<AppState>, mut request: Request, next: Next) -> Response {
    // Replace even a pre-existing extension; headers/form fields have no clock authority.
    request
        .extensions_mut()
        .insert(RequestStart(chrono::Utc::now()));
    let peer = match crate::proxy_peer::resolve(&request, state.proxy_uid) {
        Ok(peer) => peer,
        Err(status) => {
            return headers(
                (status, "Verified transport identity is required.").into_response(),
                &state,
                None,
                None,
            );
        }
    };
    request.extensions_mut().insert(RequestPeer(peer));
    let Ok(permit) = state.limits.active.clone().try_acquire_owned() else {
        return headers(
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "Server is busy. Try again shortly.",
            )
                .into_response(),
            &state,
            None,
            None,
        );
    };
    let parts: Vec<_> = request
        .uri()
        .path()
        .trim_start_matches('/')
        .split('/')
        .collect();
    let board_page = match parts.as_slice() {
        [board, ""] => !board.is_empty(),
        [board, page] => {
            !board.is_empty()
                && (*page == "catalog" || page.parse::<u16>().is_ok_and(|page| page < 1000))
        }
        [board, "thread", id] => !board.is_empty() && id.parse::<i64>().is_ok_and(|id| id > 0),
        _ => false,
    };
    let upload_page = *request.method() == Method::POST
        && matches!(parts.as_slice(), [_, "upload"] | [_, "upload", "status"]);
    let page = ((board_page && matches!(*request.method(), Method::GET | Method::HEAD))
        || upload_page)
        .then_some(parts.last() == Some(&"catalog"));
    let posting = (page == Some(false)
        && parts[0].len() <= 10
        && parts[0]
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit()))
    .then(|| format!("{}/{}/imgboard.php", state.origin, parts[0]));
    let response = protect_inner(&state, request, next).await;
    headers(
        board_http::hold_permit(response, permit),
        &state,
        page,
        posting.as_deref(),
    )
}

async fn protect_inner(state: &AppState, request: Request, next: Next) -> Response {
    if !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) {
        if request
            .headers()
            .get("origin")
            .and_then(|h| h.to_str().ok())
            != Some(&state.origin)
        {
            return (StatusCode::FORBIDDEN, "A same-origin request is required.").into_response();
        }
        if request
            .headers()
            .get("sec-fetch-site")
            .is_some_and(|v| v != "same-origin" && v != "none")
        {
            return (StatusCode::FORBIDDEN, "Cross-origin request rejected.").into_response();
        }
        let peer = request
            .extensions()
            .get::<RequestPeer>()
            .and_then(|peer| peer.0)
            .unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
        let Ok(mut peers) = state.limits.peers.lock() else {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        };
        let now = Instant::now();
        peers.retain(|_, (start, _)| now.duration_since(*start) < Duration::from_secs(60));
        if peers.len() >= state.limits.settings.tracked_peers() && !peers.contains_key(&peer) {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        let (_, used) = peers.entry(peer).or_insert((now, 0));
        if *used >= state.limits.settings.writes_per_minute() {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                [("retry-after", "60")],
                "Too many submissions. Wait a minute.",
            )
                .into_response();
        }
        *used += 1;
    }
    match tokio::time::timeout(state.limits.settings.handler_timeout(), next.run(request)).await {
        Ok(response) => response,
        Err(_) => (
            StatusCode::REQUEST_TIMEOUT,
            "Request timed out. Check the thread before retrying a post.",
        )
            .into_response(),
    }
}

fn headers(
    mut response: Response,
    state: &AppState,
    page: Option<bool>,
    posting: Option<&str>,
) -> Response {
    let interactive = page.is_some()
        && response.status().is_success()
        && response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("text/html"));
    let script = if interactive {
        let watcher = format!(
            "{}{} {}{}",
            state.origin,
            crate::ui_assets::WATCHER_SCRIPT_PATH,
            state.origin,
            crate::ui_assets::WATCHER_CORE_PATH
        );
        if page == Some(true) {
            format!(
                "{}{} {watcher}",
                state.origin,
                crate::ui_assets::CATALOG_SCRIPT_PATH
            )
        } else {
            watcher
        }
    } else {
        "'none'".into()
    };
    let script = if interactive {
        format!(
            "{script} {}{} {}{} {}{} {}{} {}{}",
            state.origin,
            crate::ui_assets::POST_TRACKING_PATH,
            state.origin,
            crate::ui_assets::NATIVE_SETTINGS_PATH,
            state.origin,
            crate::ui_assets::WATCHER_POSITION_PATH,
            state.origin,
            crate::ui_assets::NATIVE_FILTER_PATH,
            state.origin,
            crate::ui_assets::NATIVE_BACKLINKS_PATH
        )
    } else {
        script
    };
    let connect = if interactive {
        match posting {
            Some(posting) => format!("{}/_watch/ {posting}", state.origin),
            None => format!("{}/_watch/", state.origin),
        }
    } else {
        "'none'".into()
    };
    let worker = if interactive {
        format!("{}{}", state.origin, crate::ui_assets::NATIVE_FILTER_PATH)
    } else {
        "'none'".into()
    };
    let sound = if interactive {
        format!("{}{}", state.origin, crate::ui_assets::UPDATER_SOUND_PATH)
    } else {
        "'none'".into()
    };
    let script_resource = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.starts_with("text/javascript"));
    let headers = response.headers_mut();
    // Only fixed, release-owned UI images may load from the public origin.
    // Do not broaden this to 'self': uploaded content stays on the media origin.
    let mut images = format!(
        "{}/static/themes/fade.png {}/static/themes/fade-blue.png {}",
        state.origin,
        state.origin,
        crate::ui_assets::image_sources(&state.origin)
    );
    if let Some(media) = &state.media {
        images = format!("{} {images}", media.settings.origin.as_string());
    }
    let policy = if script_resource {
        // A script used as a worker receives this policy in its own execution
        // context. The initial module has no imports or network authority.
        "default-src 'none'; script-src 'none'; connect-src 'none'; worker-src 'none'; base-uri 'none'; frame-ancestors 'none'; object-src 'none'".into()
    } else {
        format!(
            "default-src 'none'; style-src 'self'; img-src {images}; media-src {sound}; script-src {script}; script-src-attr 'none'; connect-src {connect}; worker-src {worker}; form-action 'self'; base-uri 'none'; frame-ancestors 'none'; object-src 'none'"
        )
    };
    headers.insert(
        "content-security-policy",
        HeaderValue::from_str(&policy).expect("validated application origins"),
    );
    headers.insert(
        "x-content-type-options",
        HeaderValue::from_static("nosniff"),
    );
    headers.insert("x-frame-options", HeaderValue::from_static("DENY"));
    // no-referrer makes browser form navigations send Origin: null. Keep the
    // same-origin signal while withholding referrers from external destinations.
    headers.insert("referrer-policy", HeaderValue::from_static("same-origin"));
    headers.insert(
        "permissions-policy",
        HeaderValue::from_static("camera=(), microphone=(), geolocation=()"),
    );
    headers
        .entry("cache-control")
        .or_insert(HeaderValue::from_static("no-store"));
    if state.production {
        headers.insert(
            "strict-transport-security",
            HeaderValue::from_static("max-age=31536000"),
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, http::Request, middleware, routing::get};
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    use tower::ServiceExt;

    #[tokio::test]
    async fn backlink_page_code_has_no_worker_or_additional_network_authority() {
        for origin in ["http://127.0.0.1:3000", "https://board.example"] {
            let state = AppState {
                pool: sqlx::postgres::PgPoolOptions::new()
                    .connect_lazy("postgres://unused:unused@127.0.0.1:1/absent")
                    .unwrap(),
                origin: origin.into(),
                production: origin.starts_with("https:"),
                limits: Arc::new(Limits::new(board_config::PublicRequestLimits::default())),
                media: None,
                proxy_uid: None,
            };
            for page in [None, Some(false), Some(true)] {
                let response = headers(
                    axum::response::Html("owned").into_response(),
                    &state,
                    page,
                    None,
                );
                let csp = response.headers()["content-security-policy"]
                    .to_str()
                    .unwrap();
                let directive = |name: &str| {
                    csp.split(';')
                        .map(str::trim)
                        .find(|part| part.starts_with(&format!("{name} ")))
                        .unwrap()
                };
                let backlink = format!("{origin}/static/native-backlinks.v1.js");
                assert_eq!(
                    directive("script-src")
                        .split_whitespace()
                        .any(|value| value == backlink),
                    page.is_some()
                );
                assert!(
                    !directive("script-src")
                        .split_whitespace()
                        .any(|value| value == "'self'")
                );
                if page.is_some() {
                    assert_eq!(
                        directive("worker-src"),
                        format!("worker-src {origin}/static/native-filter.v1.js")
                    );
                    assert_eq!(
                        directive("connect-src"),
                        format!("connect-src {origin}/_watch/")
                    );
                } else {
                    assert_eq!(directive("script-src"), "script-src 'none'");
                    assert_eq!(directive("worker-src"), "worker-src 'none'");
                    assert_eq!(directive("connect-src"), "connect-src 'none'");
                }
                assert!(!directive("img-src").contains("native-backlinks"));
                assert!(!directive("media-src").contains("native-backlinks"));
            }
        }
    }

    #[tokio::test]
    async fn public_security_headers_fit_the_candidate_proxy_buffer() {
        for origin in [
            "https://boards.example.com".to_owned(),
            format!(
                "https://{}.{}.{}.{}",
                "a".repeat(63),
                "b".repeat(63),
                "c".repeat(63),
                "d".repeat(61)
            ),
        ] {
            let state = AppState {
                pool: sqlx::postgres::PgPoolOptions::new()
                    .connect_lazy("postgres://unused:unused@127.0.0.1:1/absent")
                    .unwrap(),
                origin,
                production: true,
                limits: Arc::new(Limits::new(board_config::PublicRequestLimits::default())),
                media: None,
                proxy_uid: None,
            };
            for page in [None, Some(false), Some(true)] {
                let response = headers(
                    axum::response::Html("owned").into_response(),
                    &state,
                    page,
                    Some(&format!("{}/test/imgboard.php", state.origin)),
                );
                let bytes = response
                    .headers()
                    .iter()
                    .map(|(name, value)| name.as_str().len() + value.as_bytes().len() + 4)
                    .sum::<usize>()
                    + 128;
                println!("Public response headers: {bytes} bytes");
                assert!(bytes < 65_536, "Candidate proxy response buffer exceeded");
            }
        }
    }

    #[tokio::test]
    async fn verified_addresses_share_canonical_limits_and_replace_forged_context() {
        use axum::extract::ConnectInfo;
        let settings = board_config::PublicRequestLimits::from_lookup(|key| match key {
            "PUBLIC_WRITES_PER_MINUTE" => Some("1".into()),
            _ => None,
        })
        .unwrap();
        let state = AppState {
            pool: sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://unused:unused@127.0.0.1:1/absent")
                .unwrap(),
            origin: "https://boards.example.com".into(),
            production: true,
            limits: Arc::new(Limits::new(settings)),
            media: None,
            proxy_uid: Some(33),
        };
        let app = Router::new()
            .route(
                "/identity",
                axum::routing::post(
                    |axum::Extension(peer): axum::Extension<RequestPeer>| async move {
                        peer.0.unwrap().to_string()
                    },
                ),
            )
            .layer(middleware::from_fn_with_state(state, protect));
        for (address, status) in [
            ("192.0.2.1", 200),
            ("::ffff:192.0.2.1", 429),
            ("192.0.2.2", 200),
        ] {
            let request = Request::post("/identity")
                .header("origin", "https://boards.example.com")
                .header("x-board-client-ip", address)
                .header("x-forwarded-for", "192.0.2.200")
                .extension(ConnectInfo(crate::proxy_peer::UnixPeer(Some(33))))
                .extension(RequestPeer(Some("192.0.2.200".parse().unwrap())))
                .body(Body::empty())
                .unwrap();
            let response = app.clone().oneshot(request).await.unwrap();
            assert_eq!(response.status(), status);
            if status == 200 {
                use http_body_util::BodyExt;
                let body = response.into_body().collect().await.unwrap().to_bytes();
                assert_eq!(&body[..], address.as_bytes());
            }
        }
    }

    #[tokio::test]
    async fn production_posting_without_transport_peer_rejects_forged_hints_before_database_access()
    {
        use http_body_util::BodyExt;
        let pool = sqlx::postgres::PgPoolOptions::new()
            .connect_lazy("postgres://unused:unused@127.0.0.1:1/absent")
            .unwrap();
        let app = crate::router(pool, "https://boards.example.com".into(), true);
        let request = Request::post("/test/imgboard.php")
            .extension(RequestPeer(Some("192.0.2.1".parse().unwrap())))
            .header("origin", "https://boards.example.com")
            .header("accept", "application/json")
            .header("content-type", "application/x-www-form-urlencoded")
            .header("x-forwarded-for", "192.0.2.1")
            .header("forwarded", "for=192.0.2.1")
            .body(Body::from("com=Owned+fixture&pwd=owned-secret"))
            .unwrap();
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), 503);
        let body = response.into_body().collect().await.unwrap().to_bytes();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&body).unwrap()["error"],
            "Posting transport identity is unavailable."
        );
    }

    #[tokio::test]
    async fn request_clock_precedes_body_collection_and_replaces_untrusted_hints() {
        use http_body_util::BodyExt;
        let state = AppState {
            pool: sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://unused:unused@127.0.0.1:1/absent")
                .unwrap(),
            origin: "http://127.0.0.1:3000".into(),
            production: false,
            limits: Arc::new(Limits::new(board_config::PublicRequestLimits::default())),
            media: None,
            proxy_uid: None,
        };
        let app =
            Router::new()
                .route(
                    "/clock",
                    axum::routing::post(
                        |axum::Extension(start): axum::Extension<RequestStart>,
                         body: bytes::Bytes| async move {
                            let collected: i64 =
                                std::str::from_utf8(&body).unwrap().parse().unwrap();
                            assert!(start.0.timestamp_millis() < collected);
                            start.0.timestamp_millis().to_string()
                        },
                    ),
                )
                .layer(middleware::from_fn_with_state(state, protect));
        let before = chrono::Utc::now().timestamp_millis();
        let body = Body::from_stream(futures_util::stream::once(async {
            tokio::time::sleep(Duration::from_millis(30)).await;
            Ok::<_, std::io::Error>(bytes::Bytes::from(
                chrono::Utc::now().timestamp_millis().to_string(),
            ))
        }));
        let mut request = Request::post("/clock")
            .header("origin", "http://127.0.0.1:3000")
            .header("x-request-start", "0")
            .header("date", "Thu, 01 Jan 1970 00:00:00 GMT")
            .body(body)
            .unwrap();
        request
            .extensions_mut()
            .insert(RequestStart(chrono::DateTime::UNIX_EPOCH));
        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), 200);
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        let start: i64 = std::str::from_utf8(&bytes).unwrap().parse().unwrap();
        assert!(start >= before);
        assert!(start <= chrono::Utc::now().timestamp_millis());
    }

    #[test]
    fn configured_hash_and_upload_semaphores_enforce_and_release_their_budgets() {
        let settings = board_config::PublicRequestLimits::from_lookup(|key| match key {
            "PUBLIC_MAX_HASH_OPERATIONS" => Some("2".into()),
            "PUBLIC_MAX_UPLOADS" => Some("3".into()),
            _ => None,
        })
        .unwrap();
        let limits = Limits::new(settings);
        for (semaphore, count) in [(&*limits.hashes, 2), (&limits.uploads, 3)] {
            let mut permits = Vec::new();
            for _ in 0..count {
                permits.push(semaphore.try_acquire().unwrap());
            }
            assert!(semaphore.try_acquire().is_err());
            drop(permits.pop());
            let recovered = semaphore.try_acquire().unwrap();
            assert!(semaphore.try_acquire().is_err());
            drop(recovered);
            drop(permits);
            assert_eq!(semaphore.available_permits(), count);
        }
    }

    #[tokio::test]
    async fn configured_timeout_drops_the_handler_and_releases_admission() {
        struct Dropped(Arc<AtomicBool>);
        impl Drop for Dropped {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let settings = board_config::PublicRequestLimits::from_lookup(|key| match key {
            "PUBLIC_MAX_ACTIVE_REQUESTS" => Some("1".into()),
            "PUBLIC_HANDLER_TIMEOUT_MS" => Some("20".into()),
            _ => None,
        })
        .unwrap();
        let limits = Arc::new(Limits::new(settings));
        let state = AppState {
            pool: sqlx::postgres::PgPoolOptions::new()
                .connect_lazy("postgres://unused:unused@127.0.0.1:1/absent")
                .unwrap(),
            origin: "http://127.0.0.1:3000".into(),
            production: false,
            limits: limits.clone(),
            media: None,
            proxy_uid: None,
        };
        let dropped = Arc::new(AtomicBool::new(false));
        let witness = dropped.clone();
        let app = Router::new()
            .route(
                "/pending",
                get(move || {
                    let dropped = witness.clone();
                    async move {
                        let _guard = Dropped(dropped);
                        std::future::pending::<()>().await;
                    }
                }),
            )
            .layer(middleware::from_fn_with_state(state, protect));
        let response = tokio::time::timeout(
            Duration::from_secs(2),
            app.oneshot(Request::get("/pending").body(Body::empty()).unwrap()),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(response.status(), StatusCode::REQUEST_TIMEOUT);
        assert!(dropped.load(Ordering::SeqCst));
        assert_eq!(limits.active.available_permits(), 0);
        drop(response);
        assert_eq!(limits.active.available_permits(), 1);
    }
}
