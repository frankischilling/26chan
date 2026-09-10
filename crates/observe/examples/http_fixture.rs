//! Synthetic loopback fixture used by tests/monitoring/qualify.py.
//! METRICS_BIND_ADDR, METRICS_TOKEN and FIXTURE_BIND_ADDR must be configured.
use std::{io, net::SocketAddr};

use axum::{Router, http::StatusCode, routing::get};
use board_observe::{Config, Endpoint, Listener, Metrics};

#[tokio::main]
async fn main() -> io::Result<()> {
    let config = Config::from_env()
        .map_err(io::Error::other)?
        .ok_or_else(|| io::Error::other("fixture metrics configuration is required"))?;
    let endpoint = Endpoint::bind(Some(config)).await?;
    let bind: SocketAddr = std::env::var("FIXTURE_BIND_ADDR")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|bind: &SocketAddr| bind.ip().is_loopback() && bind.port() != 0)
        .ok_or_else(|| io::Error::other("fixture requires a loopback address and nonzero port"))?;
    let listener = tokio::net::TcpListener::bind(bind).await?;
    let metrics = Metrics::new();
    let app = metrics.layer(
        Router::new()
            .route("/health", get(|| async { StatusCode::OK }))
            .route(
                "/failure",
                get(|| async { StatusCode::INTERNAL_SERVER_ERROR }),
            ),
        Listener::Public,
    );
    endpoint
        .serve(metrics, async { axum::serve(listener, app).await })
        .await
}
