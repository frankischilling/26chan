#![forbid(unsafe_code)]

use board_config::MediaHttpSettings;
use board_media::ApprovedFiles;
use board_media_http::{AppState, router};
use board_store::media_assets::MediaReader;

fn main() -> std::process::ExitCode {
    if std::env::args_os().len() != 1 {
        eprintln!("media HTTP startup rejected");
        return std::process::ExitCode::FAILURE;
    }
    // No unrelated credentials or unsafe mode reach the connection stage.
    let Ok(settings) = MediaHttpSettings::from_env() else {
        eprintln!("media HTTP configuration rejected");
        return std::process::ExitCode::FAILURE;
    };
    let Ok(runtime) = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(4)
        .enable_all()
        .build()
    else {
        eprintln!("media HTTP runtime unavailable");
        return std::process::ExitCode::FAILURE;
    };
    tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::INFO)
        .init();
    if runtime.block_on(run(settings)).is_err() {
        tracing::error!("media HTTP stopped with an error");
        std::process::ExitCode::FAILURE
    } else {
        std::process::ExitCode::SUCCESS
    }
}

async fn run(settings: MediaHttpSettings) -> Result<(), Box<dyn std::error::Error>> {
    let files = ApprovedFiles::open(settings.approved_dir)?;
    files.ready()?;
    let reader = MediaReader::connect(&settings.reader.database_url).await?;
    let listener = tokio::net::TcpListener::bind(settings.bind).await?;
    tracing::info!("media HTTP ready");
    let app = router(AppState::new(reader.clone(), files, &settings.origin));
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(shutdown())
        .await;
    reader.close().await;
    result?;
    Ok(())
}

async fn shutdown() {
    let interrupt = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    #[cfg(unix)]
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();
    tokio::select! { _ = interrupt => (), _ = terminate => () }
}
