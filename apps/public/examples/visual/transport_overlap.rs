//! Opt-in native diagnostic only. Events describe body polls, never network delivery.
//! At most twenty bodies and eighty scalar log records per fixture process.
use std::{
    convert::Infallible,
    ffi::OsStr,
    future::Future,
    io::Write,
    pin::Pin,
    sync::{Arc, Mutex},
    task::{Context, Poll},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use axum::{
    Router,
    body::{Body, Bytes},
    extract::{RawQuery, State},
    http::{Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use futures_util::Stream;
use serde::Serialize;
use tokio::time::Sleep;

pub const ENABLE_ENV: &str = "WINDOWS_VISUAL_RECEIVE_CONNECT_PROBE";
const POOLS: usize = 20;
const DELAY: Duration = Duration::from_millis(250);
const PREFIX: [u8; 1024] = [b'P'; 1024];
const SUFFIX: [u8; 3072] = [b'S'; 3072];

pub fn from_env(app: Router) -> Router {
    let enabled = enabled(
        cfg!(windows),
        std::env::var_os("WINDOWS_VISUAL_RESOURCE_DIAGNOSTICS").as_deref(),
        std::env::var_os(ENABLE_ENV).as_deref(),
    );
    mount(app, enabled)
}

fn enabled(windows: bool, diagnostics: Option<&OsStr>, probe: Option<&OsStr>) -> bool {
    windows && diagnostics == Some(OsStr::new("1")) && probe == Some(OsStr::new("1"))
}

fn mount(app: Router, enabled: bool) -> Router {
    if !enabled {
        return app;
    }
    app.merge(
        Router::new()
            .route("/__transport_overlap", get(response))
            .with_state(Arc::new(Mutex::new(Trace::new()))),
    )
}

struct Trace {
    origin: Instant,
    claimed: [bool; POOLS],
    sequence: u32,
    output_failed: bool,
    #[cfg(test)]
    events: Vec<Event>,
}

#[derive(Clone, Serialize)]
struct Event {
    schema_version: u8,
    scope: &'static str,
    sequence: u32,
    pool: u8,
    request_id: u8,
    lane: u8,
    event: &'static str,
    monotonic_ns: u64,
    unix_ms: u64,
    emitted_bytes: u16,
    output_failed: bool,
    emission_boundary: &'static str,
}

impl Trace {
    fn new() -> Self {
        Self {
            origin: Instant::now(),
            claimed: [false; POOLS],
            sequence: 0,
            output_failed: false,
            #[cfg(test)]
            events: Vec::new(),
        }
    }

    fn emit(&mut self, pool: u8, event: &'static str, emitted_bytes: u16) {
        // Each pool is claimed only once, each body has at most four events.
        self.sequence += 1;
        let record = Event {
            schema_version: 1,
            scope: "fixture-transport-overlap",
            sequence: self.sequence,
            pool,
            request_id: pool + 1,
            lane: 0,
            event,
            monotonic_ns: self.origin.elapsed().as_nanos().min(u64::MAX as u128) as u64,
            unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .min(u64::MAX as u128) as u64,
            emitted_bytes,
            output_failed: self.output_failed,
            emission_boundary: "http_body_poll",
        };
        #[cfg(test)]
        self.events.push(record.clone());
        let stdout = std::io::stdout();
        let mut output = stdout.lock();
        if write!(output, "[owned-fixture-overlap] ").is_err()
            || serde_json::to_writer(&mut output, &record).is_err()
            || writeln!(output).is_err()
            || output.flush().is_err()
        {
            self.output_failed = true;
        }
    }
}

type SharedTrace = Arc<Mutex<Trace>>;

fn pool(query: Option<&str>) -> Option<u8> {
    let value = query?.strip_prefix("pool=")?;
    let pool: u8 = value.parse().ok()?;
    // Exact canonical identity: no extra keys, escaping, signs or leading zeroes.
    (pool < POOLS as u8 && value == pool.to_string()).then_some(pool)
}

async fn response(
    State(trace): State<SharedTrace>,
    method: Method,
    RawQuery(query): RawQuery,
) -> Response {
    if method != Method::GET {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let Some(pool) = pool(query.as_deref()) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    let body = {
        let Ok(mut state) = trace.lock() else {
            return StatusCode::INTERNAL_SERVER_ERROR.into_response();
        };
        if state.claimed[usize::from(pool)] {
            return StatusCode::CONFLICT.into_response();
        }
        state.claimed[usize::from(pool)] = true;
        state.emit(pool, "accepted", 0);
        DelayedBody {
            trace: trace.clone(),
            pool,
            emitted: 0,
            timer: None,
        }
    };
    (
        [
            (header::CONTENT_TYPE, "application/octet-stream"),
            (header::CONTENT_LENGTH, "4096"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        Body::from_stream(body),
    )
        .into_response()
}

struct DelayedBody {
    trace: SharedTrace,
    pool: u8,
    emitted: u16,
    // Sleep is owned by the response, not a spawned producer. Dropping the body
    // cancels its timer synchronously, including an entirely unpolled response.
    timer: Option<Pin<Box<Sleep>>>,
}

impl DelayedBody {
    fn emit(&self, event: &'static str) {
        if let Ok(mut state) = self.trace.lock() {
            state.emit(self.pool, event, self.emitted);
        }
    }
}

impl Stream for DelayedBody {
    type Item = Result<Bytes, Infallible>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.emitted {
            0 => {
                self.emitted = 1024;
                self.emit("prefix_emitted");
                self.timer = Some(Box::pin(tokio::time::sleep(DELAY)));
                Poll::Ready(Some(Ok(Bytes::from_static(&PREFIX))))
            }
            1024 => {
                if self
                    .timer
                    .as_mut()
                    .expect("prefix owns delay")
                    .as_mut()
                    .poll(cx)
                    .is_pending()
                {
                    return Poll::Pending;
                }
                self.timer = None;
                self.emitted = 4096;
                self.emit("suffix_emitted");
                Poll::Ready(Some(Ok(Bytes::from_static(&SUFFIX))))
            }
            _ => Poll::Ready(None),
        }
    }
}

impl Drop for DelayedBody {
    fn drop(&mut self) {
        // Release timer before the terminal ownership checkpoint. Complete means
        // both frames were handed to HTTP, not acknowledged by the peer.
        self.timer = None;
        self.emit(if self.emitted == 4096 {
            "body_complete"
        } else {
            "body_dropped"
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;
    use futures_util::StreamExt;
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    fn body(trace: &SharedTrace) -> DelayedBody {
        let mut state = trace.lock().unwrap();
        assert!(!state.claimed[0]);
        state.claimed[0] = true;
        state.emit(0, "accepted", 0);
        DelayedBody {
            trace: trace.clone(),
            pool: 0,
            emitted: 0,
            timer: None,
        }
    }

    #[test]
    fn gate_and_identity_are_strictly_bounded() {
        let one = Some(OsStr::new("1"));
        assert!(enabled(true, one, one));
        assert!(!enabled(false, one, one));
        assert!(!enabled(true, None, one));
        assert!(!enabled(true, one, None));
        assert!(!enabled(true, one, Some(OsStr::new("true"))));
        for n in 0..20 {
            assert_eq!(pool(Some(&format!("pool={n}"))), Some(n));
        }
        for query in [
            "pool=20",
            "pool=255",
            "pool=00",
            "pool=-1",
            "pool=+1",
            "pool=1&x=2",
            "pool=%31",
            "pool=1&pool=2",
            "",
            "pool=",
        ] {
            assert_eq!(pool(Some(query)), None, "{query}");
        }
        assert_eq!(pool(None), None);
    }

    #[tokio::test(start_paused = true)]
    async fn exact_frames_delay_and_complete_owned_cleanup() {
        let trace = Arc::new(Mutex::new(Trace::new()));
        let mut stream = body(&trace);
        assert_eq!(stream.next().await.unwrap().unwrap().as_ref(), &PREFIX);
        assert!(futures_util::poll!(stream.next()).is_pending());
        tokio::time::advance(DELAY - Duration::from_millis(1)).await;
        assert!(futures_util::poll!(stream.next()).is_pending());
        tokio::time::advance(Duration::from_millis(2)).await;
        assert_eq!(stream.next().await.unwrap().unwrap().as_ref(), &SUFFIX);
        assert!(stream.next().await.is_none());
        assert!(stream.timer.is_none());
        drop(stream);
        let state = trace.lock().unwrap();
        assert_eq!(
            state
                .events
                .iter()
                .map(|event| event.event)
                .collect::<Vec<_>>(),
            [
                "accepted",
                "prefix_emitted",
                "suffix_emitted",
                "body_complete"
            ]
        );
        assert_eq!(state.events.last().unwrap().emitted_bytes, 4096);
        assert_eq!(Arc::strong_count(&trace), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn cancellation_drops_owned_timer_without_later_emission() {
        for poll_prefix in [false, true] {
            let trace = Arc::new(Mutex::new(Trace::new()));
            let mut stream = body(&trace);
            if poll_prefix {
                stream.next().await.unwrap().unwrap();
                assert!(futures_util::poll!(stream.next()).is_pending());
            }
            drop(stream);
            assert_eq!(
                Arc::strong_count(&trace),
                1,
                "no producer task retains ownership"
            );
            tokio::time::advance(Duration::from_secs(10)).await;
            let state = trace.lock().unwrap();
            assert_eq!(state.events.len(), if poll_prefix { 3 } else { 2 });
            assert_eq!(state.events.last().unwrap().event, "body_dropped");
            assert_eq!(
                state.events.last().unwrap().emitted_bytes,
                if poll_prefix { 1024 } else { 0 }
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn route_framing_gate_off_and_duplicates() {
        let base =
            || Router::new().route("/readyz", get(|| async { "synthetic fixture renderer" }));
        let off = mount(base(), false);
        assert_eq!(
            off.clone()
                .oneshot(
                    Request::builder()
                        .uri("/__transport_overlap?pool=0")
                        .body(Body::empty())
                        .unwrap()
                )
                .await
                .unwrap()
                .status(),
            StatusCode::NOT_FOUND
        );
        let ready = off
            .oneshot(
                Request::builder()
                    .uri("/readyz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            ready.into_body().collect().await.unwrap().to_bytes(),
            "synthetic fixture renderer"
        );
        let on = mount(base(), true);
        for n in 0..20 {
            let response = on
                .clone()
                .oneshot(
                    Request::builder()
                        .uri(format!("/__transport_overlap?pool={n}"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()[header::CONTENT_LENGTH], "4096");
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            assert_eq!(bytes.len(), 4096);
            assert_eq!(&bytes[..1024], &PREFIX);
            assert_eq!(&bytes[1024..], &SUFFIX);
        }
        for _ in 0..100 {
            assert_eq!(
                on.clone()
                    .oneshot(
                        Request::builder()
                            .uri("/__transport_overlap?pool=0")
                            .body(Body::empty())
                            .unwrap()
                    )
                    .await
                    .unwrap()
                    .status(),
                StatusCode::CONFLICT
            );
        }
    }

    #[tokio::test]
    async fn real_http_framing_and_shutdown_release_listener_and_body() {
        use tokio::{
            io::{AsyncReadExt, AsyncWriteExt},
            net::TcpStream,
            sync::oneshot,
        };
        let trace = Arc::new(Mutex::new(Trace::new()));
        let app = Router::new()
            .route("/__transport_overlap", get(response))
            .with_state(trace.clone());
        let profile = crate::listeners::MediaProfile::parse(Some(OsStr::new("ipv4-only"))).unwrap();
        let listener = crate::listeners::bind_media(profile, 0)
            .await
            .unwrap()
            .pop()
            .unwrap();
        let address = listener.local_addr().unwrap();
        let (stop, stopped) = oneshot::channel();
        let server = tokio::spawn(crate::listeners::serve(vec![(listener, app)], async {
            stopped.await.map_err(std::io::Error::other)
        }));
        tokio::time::timeout(Duration::from_secs(10), async {
            let mut client = TcpStream::connect(address).await.unwrap();
            client.write_all(b"GET /__transport_overlap?pool=0 HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").await.unwrap();
            let mut bytes = Vec::new();
            let split = loop {
                let mut chunk = [0; 8192];
                let read = client.read(&mut chunk).await.unwrap();
                assert_ne!(read, 0, "response ended before prefix");
                bytes.extend_from_slice(&chunk[..read]);
                if let Some(split) = bytes.windows(4).position(|window| window == b"\r\n\r\n")
                    && bytes.len() >= split + 4 + PREFIX.len()
                {
                    break split + 4;
                }
            };
            // Shutdown while a body may be delayed; the listener supervisor owns
            // and joins the connection. No body-level producer task is spawned.
            stop.send(()).unwrap();
            client.read_to_end(&mut bytes).await.unwrap();
            server.await.unwrap().unwrap();
            let headers = std::str::from_utf8(&bytes[..split]).unwrap().to_ascii_lowercase();
            assert!(headers.starts_with("http/1.1 200 ok\r\n"));
            assert!(headers.contains("content-length: 4096\r\n"));
            assert!(!headers.contains("transfer-encoding:"));
            assert_eq!(bytes.len() - split, 4096);
            assert_eq!(&bytes[split..split + 1024], &PREFIX);
            assert_eq!(&bytes[split + 1024..], &SUFFIX);
            assert!(TcpStream::connect(address).await.is_err());
        }).await.expect("bounded listener shutdown");
        assert_eq!(
            Arc::strong_count(&trace),
            1,
            "router and body owners released"
        );
        let state = trace.lock().unwrap();
        assert_eq!(state.events.len(), 4);
        assert_eq!(state.events.last().unwrap().event, "body_complete");
        assert!(state.events[2].monotonic_ns - state.events[1].monotonic_ns >= 250_000_000);
    }

    #[tokio::test(start_paused = true)]
    async fn malformed_head_and_repeated_requests_cannot_expand_event_budget() {
        let trace = Arc::new(Mutex::new(Trace::new()));
        for _ in 0..100 {
            let head = response(
                State(trace.clone()),
                Method::HEAD,
                RawQuery(Some("pool=0".into())),
            )
            .await;
            assert_eq!(head.status(), StatusCode::METHOD_NOT_ALLOWED);
            let invalid = response(
                State(trace.clone()),
                Method::GET,
                RawQuery(Some("pool=20".into())),
            )
            .await;
            assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
        }
        assert!(trace.lock().unwrap().events.is_empty());
        for pool in 0..20 {
            let result = response(
                State(trace.clone()),
                Method::GET,
                RawQuery(Some(format!("pool={pool}"))),
            )
            .await;
            assert_eq!(result.status(), StatusCode::OK);
            result.into_body().collect().await.unwrap();
        }
        for _ in 0..100 {
            let duplicate = response(
                State(trace.clone()),
                Method::GET,
                RawQuery(Some("pool=0".into())),
            )
            .await;
            assert_eq!(duplicate.status(), StatusCode::CONFLICT);
        }
        let state = trace.lock().unwrap();
        assert_eq!(state.events.len(), 80);
        assert_eq!(state.sequence, 80);
        for (index, event) in state.events.iter().enumerate() {
            assert_eq!(event.sequence as usize, index + 1);
            assert_eq!(event.pool as usize, index / 4);
            assert_eq!(event.request_id, event.pool + 1);
            assert_eq!(event.lane, 0);
            assert_eq!(event.emission_boundary, "http_body_poll");
        }
    }
}
