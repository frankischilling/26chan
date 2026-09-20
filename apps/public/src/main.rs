#![forbid(unsafe_code)]
use board_config::Settings;
use board_public::transport::{ConnectionBudget, PublicListener};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .json()
        .with_target(false)
        .with_max_level(tracing::Level::INFO)
        .init();
    let settings = Settings::from_env()?;
    let metrics_config = board_observe::Config::from_env()?;
    // Acquire every configured listener before connecting or serving. A failed
    // API bind must not leave a partially started public application.
    let listener = PublicListener::bind(settings.bind, settings.public_proxy.as_ref()).await?;
    let api_listener = match &settings.api {
        Some(api) => Some(tokio::net::TcpListener::bind(api.bind).await?),
        None => None,
    };
    let metrics_endpoint = board_observe::Endpoint::bind(metrics_config).await?;
    let pool = board_store::connect_public(&settings.database_url).await?;
    if let Some(media) = &settings.media {
        board_public::media_ready(media).await?;
    }
    let (metrics, app, api_app) = board_public::observed_routers_with_proxy(
        pool.clone(),
        settings.public_origin.as_string(),
        settings.production,
        settings.api.is_some(),
        settings.media.clone(),
        settings.request_limits,
        settings.public_proxy.as_ref().map(|proxy| proxy.uid()),
    );
    if let Some(proxy) = &settings.public_proxy {
        tracing::info!(socket = %proxy.socket().display(), proxy_uid = proxy.uid(), "verified public proxy listener started");
    } else {
        tracing::info!(bind = %settings.bind, media_enabled = settings.media.is_some(), "public server started");
    }
    tracing::info!(request_limits = ?settings.request_limits, "public request budgets configured");
    if let Some(api) = &settings.api {
        tracing::info!(bind = %api.bind, origin = %api.origin.as_string(), "JSON API listener started");
    }
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let serving = metrics_endpoint.serve(
        metrics,
        serve_pair(
            listener,
            app,
            api_listener,
            api_app,
            stopped,
            settings.request_limits,
        ),
    );
    tokio::pin!(serving);
    let result = tokio::select! {
        result = &mut serving => result,
        () = shutdown() => {
            let _ = stop.send(true);
            serving.await
        }
    };
    pool.close().await;
    result?;
    Ok(())
}

async fn serve_pair(
    listener: PublicListener,
    app: axum::Router,
    api_listener: Option<tokio::net::TcpListener>,
    api_app: axum::Router,
    stopped: tokio::sync::watch::Receiver<bool>,
    limits: board_config::PublicRequestLimits,
) -> std::io::Result<()> {
    let budget = ConnectionBudget::new(limits);
    let public = listener.serve(app, stopped.clone(), budget.clone());
    if let Some(api_listener) = api_listener {
        tokio::try_join!(
            public,
            PublicListener::Tcp(api_listener).serve(api_app, stopped, budget)
        )?;
    } else {
        public.await?;
    }
    Ok(())
}

async fn shutdown() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut terminate = signal(SignalKind::terminate()).expect("signal handler");
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, extract::State, routing::get};
    use std::{
        io::{Read, Write},
        net::SocketAddr,
        sync::Arc,
        time::Duration,
    };
    use tokio::sync::{Notify, mpsc};

    #[derive(Clone)]
    struct TestState {
        started: mpsc::UnboundedSender<()>,
        release: Arc<Notify>,
    }

    async fn hold(State(state): State<TestState>) {
        state.started.send(()).unwrap();
        state.release.notified().await;
    }

    #[tokio::test]
    async fn paired_listeners_share_connection_admission_in_both_directions() {
        for public_first in [true, false] {
            let public_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let public_address = public_listener.local_addr().unwrap();
            let api_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let api_address = api_listener.local_addr().unwrap();
            let (started, mut active) = mpsc::unbounded_channel();
            let release = Arc::new(Notify::new());
            let app = Router::new()
                .route("/hold", get(hold))
                .with_state(TestState {
                    started,
                    release: release.clone(),
                });
            let limits = board_config::PublicRequestLimits::from_lookup(|name| {
                (name == "PUBLIC_MAX_CONNECTIONS").then(|| "1".into())
            })
            .unwrap();
            let (stop, stopped) = tokio::sync::watch::channel(false);
            let server = tokio::spawn(serve_pair(
                PublicListener::Tcp(public_listener),
                app.clone(),
                Some(api_listener),
                app,
                stopped,
                limits,
            ));
            let (first, second) = if public_first {
                (public_address, api_address)
            } else {
                (api_address, public_address)
            };
            let held = request_hold(first);
            tokio::time::timeout(Duration::from_secs(2), active.recv())
                .await
                .unwrap()
                .unwrap();
            let mut excess = request_hold(second);
            tokio::task::spawn_blocking(move || {
                let mut response = String::new();
                let result = excess.read_to_string(&mut response);
                assert!(response.is_empty(), "excess connection reached a handler");
                match result {
                    Ok(0) => {}
                    Err(error)
                        if matches!(
                            error.kind(),
                            std::io::ErrorKind::ConnectionReset
                                | std::io::ErrorKind::ConnectionAborted
                                | std::io::ErrorKind::BrokenPipe
                        ) => {}
                    other => panic!("excess connection did not close promptly: {other:?}"),
                }
            })
            .await
            .unwrap();
            assert!(active.try_recv().is_err());
            release.notify_one();
            assert!(
                tokio::task::spawn_blocking(move || read_response(held))
                    .await
                    .unwrap()
                    .starts_with("HTTP/1.1 200")
            );

            let recovered = request_hold(second);
            tokio::time::timeout(Duration::from_secs(2), active.recv())
                .await
                .unwrap()
                .unwrap();
            release.notify_one();
            assert!(
                tokio::task::spawn_blocking(move || read_response(recovered))
                    .await
                    .unwrap()
                    .starts_with("HTTP/1.1 200")
            );
            stop.send(true).unwrap();
            tokio::time::timeout(Duration::from_secs(2), server)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
        }
    }

    #[tokio::test]
    async fn paired_listener_shutdown_drains_requests_before_releasing_sockets() {
        let public_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let public_address = public_listener.local_addr().unwrap();
        let api_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let api_address = api_listener.local_addr().unwrap();
        let (started, mut active) = mpsc::unbounded_channel();
        let release = Arc::new(Notify::new());
        let app = Router::new()
            .route("/hold", get(hold))
            .with_state(TestState {
                started,
                release: release.clone(),
            });
        let (stop, stopped) = tokio::sync::watch::channel(false);
        let server = tokio::spawn(serve_pair(
            PublicListener::Tcp(public_listener),
            app.clone(),
            Some(api_listener),
            app,
            stopped,
            board_config::PublicRequestLimits::default(),
        ));
        let public_client = request_hold(public_address);
        let api_client = request_hold(api_address);
        for _ in 0..2 {
            tokio::time::timeout(Duration::from_secs(2), active.recv())
                .await
                .expect("listener accepted the active request")
                .expect("server keeps its active-request channel open");
        }
        stop.send(true).unwrap();
        tokio::task::yield_now().await;
        assert!(!server.is_finished());
        release.notify_one();
        release.notify_one();
        let public_response = tokio::task::spawn_blocking(move || read_response(public_client));
        let api_response = tokio::task::spawn_blocking(move || read_response(api_client));
        assert!(
            tokio::time::timeout(Duration::from_secs(2), public_response)
                .await
                .expect("public response completed")
                .unwrap()
                .starts_with("HTTP/1.1 200")
        );
        assert!(
            tokio::time::timeout(Duration::from_secs(2), api_response)
                .await
                .expect("API response completed")
                .unwrap()
                .starts_with("HTTP/1.1 200")
        );
        tokio::time::timeout(Duration::from_secs(2), server)
            .await
            .expect("paired server drained after request completion")
            .unwrap()
            .unwrap();
        assert!(tokio::net::TcpListener::bind(public_address).await.is_ok());
        assert!(tokio::net::TcpListener::bind(api_address).await.is_ok());
    }

    fn request_hold(address: SocketAddr) -> std::net::TcpStream {
        let mut client = std::net::TcpStream::connect(address).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        client
            .write_all(b"GET /hold HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
            .unwrap();
        client
    }

    fn read_response(mut client: std::net::TcpStream) -> String {
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        response
    }
}
