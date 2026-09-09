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
    let state = board_staff::AppState::connect(config)
        .await
        .map_err(|_| "Staff database or WebAuthn setup unavailable")?;
    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .map_err(|_| "Staff listener unavailable")?;
    axum::serve(listener, board_staff::router(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
