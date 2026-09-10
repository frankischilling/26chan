#![forbid(unsafe_code)]

use board_observe::{Config, Endpoint, Metrics};
use board_resource_monitor::{SampleState, collect, config, sample_loop};
use std::{process::ExitCode, time::Duration};

fn main() -> ExitCode {
    if !cfg!(target_os = "linux") {
        eprintln!("resource observer requires Linux cgroup v2");
        return ExitCode::FAILURE;
    }
    if std::env::args_os().len() != 1 {
        eprintln!("resource observer startup rejected");
        return ExitCode::FAILURE;
    }
    let Ok(targets) = config::from_env() else {
        eprintln!("resource observer configuration rejected");
        return ExitCode::FAILURE;
    };
    let Ok(Some(metrics)) = Config::from_env() else {
        eprintln!("resource observer private endpoint configuration rejected");
        return ExitCode::FAILURE;
    };
    let Ok(runtime) = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .max_blocking_threads(1)
        .enable_all()
        .build()
    else {
        eprintln!("resource observer runtime unavailable");
        return ExitCode::FAILURE;
    };
    tracing_subscriber::fmt()
        .json()
        .with_max_level(tracing::Level::INFO)
        .init();
    let result = runtime.block_on(run(targets, metrics));
    // A kernel filesystem call cannot be cancelled by dropping JoinHandle.
    // Bound runtime teardown too; the unit applies its external stop deadline.
    runtime.shutdown_timeout(Duration::from_secs(1));
    if result.is_err() {
        tracing::error!("resource observer stopped with an error");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

async fn run(targets: config::Targets, config: Config) -> Result<(), Box<dyn std::error::Error>> {
    let endpoint = Endpoint::bind(Some(config)).await?;
    let state = SampleState::default();
    let mut metrics = Metrics::new();
    let observed = state.clone();
    metrics.register_resources(move || observed.snapshot())?;
    let readiness = state.clone();
    let (stop, stopped) = tokio::sync::watch::channel(false);
    let application = async {
        let sampler = sample_loop(state, move || collect::collect(&targets), stopped);
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
        .serve_with_health(metrics, application, move || readiness.snapshot().available)
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
