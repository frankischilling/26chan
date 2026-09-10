#![forbid(unsafe_code)]

use board_maintenance_monitor::{SampleState, config, journal, sample_loop};
use board_observe::{Config, Endpoint, Metrics};
use std::{process::ExitCode, time::Duration};

fn main() -> ExitCode {
    if !cfg!(target_os = "linux") || std::env::args_os().len() != 1 {
        eprintln!("maintenance observer requires Linux and environment configuration");
        return ExitCode::FAILURE;
    }
    let Ok(targets) = config::from_env() else {
        eprintln!("maintenance observer configuration rejected");
        return ExitCode::FAILURE;
    };
    let Ok(Some(metrics)) = Config::from_env() else {
        eprintln!("maintenance observer private endpoint configuration rejected");
        return ExitCode::FAILURE;
    };
    let Ok(runtime) = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(1)
        .enable_all()
        .build()
    else {
        eprintln!("maintenance observer runtime unavailable");
        return ExitCode::FAILURE;
    };
    tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::INFO)
        .init();
    let result = runtime.block_on(run(targets, metrics));
    runtime.shutdown_timeout(Duration::from_secs(1));
    if result.is_err() {
        tracing::error!("maintenance observer stopped with an error");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

async fn run(targets: config::Targets, config: Config) -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = Endpoint::bind(Some(config)).await?;
    let state = SampleState::new(&targets);
    let mut metrics = Metrics::new();
    let observed = state.clone();
    metrics.register_maintenance(move || observed.snapshot())?;
    let readiness = state.clone();
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let application = async {
        let sampler = sample_loop(state, move || journal::collect(&targets), stopped);
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
    endpoint
        .serve_with_health(metrics, application, move || readiness.ready())
        .await?;
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
