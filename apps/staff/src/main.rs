#![forbid(unsafe_code)]
#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("Staff startup failed: {error}");
        std::process::exit(1);
    }
}
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = board_staff::Config::from_env()?;
    let bind = config.bind;
    let metrics_config = board_observe::Config::from_env()?;
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|_| "Staff listener unavailable")?;
    let metrics_endpoint = board_observe::Endpoint::bind(metrics_config).await?;
    let state = board_staff::AppState::connect(config)
        .await
        .map_err(|_| "Staff database or WebAuthn setup unavailable")?;
    let (metrics, app) = board_staff::observed_router(state);
    let serving = async {
        axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = tokio::signal::ctrl_c().await;
            })
            .await
    };
    metrics_endpoint.serve(metrics, serving).await?;
    Ok(())
}
