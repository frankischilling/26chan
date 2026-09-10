#![forbid(unsafe_code)]

use board_config::MonitorSettings;
use board_monitor::{SampleState, sample_loop};
use board_observe::{Config, Endpoint, Metrics};
use board_store::monitoring::MonitorReader;

fn main() -> std::process::ExitCode {
    if std::env::args_os().len() != 1 {
        eprintln!("queue observer startup rejected");
        return std::process::ExitCode::FAILURE;
    }
    let Ok(settings) = MonitorSettings::from_env() else {
        eprintln!("queue observer configuration rejected");
        return std::process::ExitCode::FAILURE;
    };
    let Ok(Some(metrics_config)) = Config::from_env() else {
        eprintln!("queue observer private endpoint configuration rejected");
        return std::process::ExitCode::FAILURE;
    };
    let Ok(runtime) = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(2)
        .enable_all()
        .build()
    else {
        eprintln!("queue observer runtime unavailable");
        return std::process::ExitCode::FAILURE;
    };
    tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::INFO)
        .init();
    if runtime.block_on(run(settings, metrics_config)).is_err() {
        tracing::error!("queue observer stopped with an error");
        std::process::ExitCode::FAILURE
    } else {
        std::process::ExitCode::SUCCESS
    }
}

async fn run(settings: MonitorSettings, config: Config) -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = Endpoint::bind(Some(config)).await?;
    let reader = MonitorReader::connect(&settings.database_url).await?;
    let state = SampleState::default();
    let mut metrics = Metrics::new();
    let sampled = state.clone();
    metrics.register_media_queue(move || sampled.snapshot())?;
    let readiness = state.clone();
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let application = async {
        let sampler = sample_loop(state, || reader.snapshot(), stopped);
        tokio::pin!(sampler);
        tokio::select! {
            _ = &mut sampler => (),
            _ = shutdown() => {
                let _ = stop.send(true);
                sampler.await;
            }
        }
        Ok(())
    };
    let result = endpoint
        .serve_with_health(metrics, application, move || readiness.snapshot().available)
        .await;
    // The paired application owns the sampler. Exporter failure drops an
    // in-progress sample before closing the connection pool.
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
