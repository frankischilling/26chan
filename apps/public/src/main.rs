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
    let pool = board_store::connect_public(&settings.database_url).await?;
    let app = board_public::router(
        pool.clone(),
        settings.public_origin.as_string(),
        settings.production,
    );
    let listener = tokio::net::TcpListener::bind(settings.bind).await?;
    tracing::info!(bind = %settings.bind, media_enabled = false, "public server started");
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown())
    .await?;
    pool.close().await;
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
