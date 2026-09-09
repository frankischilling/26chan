#![forbid(unsafe_code)]
use board_config::Settings;
use std::net::SocketAddr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .json()
        .with_target(false)
        .with_max_level(tracing::Level::INFO)
        .init();
    let settings = Settings::from_env()?;
    // Acquire every configured listener before connecting or serving. A failed
    // API bind must not leave a partially started public application.
    let listener = tokio::net::TcpListener::bind(settings.bind).await?;
    let api_listener = match &settings.api {
        Some(api) => Some(tokio::net::TcpListener::bind(api.bind).await?),
        None => None,
    };
    let pool = board_store::connect_public(&settings.database_url).await?;
    let (app, api_app) = board_public::routers(
        pool.clone(),
        settings.public_origin.as_string(),
        settings.production,
    );
    tracing::info!(bind = %settings.bind, media_enabled = false, "public server started");
    if let Some(api) = &settings.api {
        tracing::info!(bind = %api.bind, origin = %api.origin.as_string(), "JSON API listener started");
    }
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let serving = serve_pair(listener, app, api_listener, api_app, stopped);
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

async fn serve(
    listener: tokio::net::TcpListener,
    app: axum::Router,
    mut stopped: tokio::sync::watch::Receiver<bool>,
) -> std::io::Result<()> {
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        if !*stopped.borrow_and_update() {
            let _ = stopped.changed().await;
        }
    })
    .await
}

async fn serve_pair(
    listener: tokio::net::TcpListener,
    app: axum::Router,
    api_listener: Option<tokio::net::TcpListener>,
    api_app: axum::Router,
    stopped: tokio::sync::watch::Receiver<bool>,
) -> std::io::Result<()> {
    let public = serve(listener, app, stopped.clone());
    if let Some(api_listener) = api_listener {
        tokio::try_join!(public, serve(api_listener, api_app, stopped))?;
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
            public_listener,
            app.clone(),
            Some(api_listener),
            app,
            stopped,
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
