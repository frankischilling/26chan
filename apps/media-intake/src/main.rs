#![forbid(unsafe_code)]

use board_media::Quarantine;
use board_media_intake::{AppState, config::Settings, observed_router};
use board_store::media_intake::IntakeStore;

fn main() -> std::process::ExitCode {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    let check = args.len() == 1 && args[0] == "--check-config";
    if !args.is_empty() && !check {
        eprintln!("media intake arguments rejected");
        return std::process::ExitCode::FAILURE;
    }
    let (Ok(settings), Ok(metrics)) = (Settings::from_env(), board_observe::Config::from_env())
    else {
        eprintln!("media intake configuration rejected");
        return std::process::ExitCode::FAILURE;
    };
    if check {
        return std::process::ExitCode::SUCCESS;
    }
    let Ok(runtime) = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(4)
        .enable_all()
        .build()
    else {
        eprintln!("media intake runtime unavailable");
        return std::process::ExitCode::FAILURE;
    };
    tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::INFO)
        .init();
    if runtime.block_on(run(settings, metrics)).is_err() {
        tracing::error!("media intake stopped with an error");
        std::process::ExitCode::FAILURE
    } else {
        std::process::ExitCode::SUCCESS
    }
}

async fn run(
    settings: Settings,
    metrics_config: Option<board_observe::Config>,
) -> Result<(), Box<dyn std::error::Error>> {
    let quarantine = Quarantine::new(settings.quarantine_dir)?;
    quarantine.ready()?;
    let store = IntakeStore::connect(&settings.database_url).await?;
    let listener = tokio::net::TcpListener::bind(settings.bind).await?;
    let endpoint = board_observe::Endpoint::bind(metrics_config).await?;
    let (metrics, app) = observed_router(AppState::new(store.clone(), quarantine, settings.token)?);
    tracing::info!("media intake ready");
    let serving = async {
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown())
            .await
    };
    let result = endpoint.serve(metrics, serving).await;
    store.close().await?;
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
