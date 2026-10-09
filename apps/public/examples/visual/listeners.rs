//! Transport contract for synthetic browser fixtures, never production listeners.
use std::{ffi::OsStr, future::Future, io, net::Ipv4Addr, net::Ipv6Addr, time::Duration};

use axum::Router;
use tokio::{net::TcpListener, sync::watch, task::JoinSet};

pub const PROFILE_ENV: &str = "VISUAL_FIXTURE_MEDIA_PROFILE";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MediaProfile {
    DualLoopback,
    Ipv4Only,
}

impl MediaProfile {
    pub fn parse(value: Option<&OsStr>) -> io::Result<Self> {
        match value {
            None => Ok(Self::DualLoopback),
            Some(value) if value == "dual-loopback" => Ok(Self::DualLoopback),
            Some(value) if value == "ipv4-only" => Ok(Self::Ipv4Only),
            Some(_) => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{PROFILE_ENV} must be dual-loopback or ipv4-only"),
            )),
        }
    }
}

pub async fn bind_media(profile: MediaProfile, port: u16) -> io::Result<Vec<TcpListener>> {
    let ipv4 = TcpListener::bind((Ipv4Addr::LOCALHOST, port)).await?;
    // Port zero is useful to transport tests; both families still share one port.
    let port = ipv4.local_addr()?.port();
    let mut listeners = vec![ipv4];
    if profile == MediaProfile::DualLoopback {
        // localhost intentionally separates media cookies from 127.0.0.1.
        // Cover both explicit loopbacks; never expose a wildcard or silently
        // fall back if the host cannot satisfy this fixture contract.
        let ipv6 = TcpListener::bind((Ipv6Addr::LOCALHOST, port))
            .await
            .map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("dual-loopback fixture requires [::1]:{port}: {error}"),
                )
            })?;
        listeners.push(ipv6);
    }
    Ok(listeners)
}

async fn serve_listener(
    listener: TcpListener,
    app: Router,
    mut stopped: watch::Receiver<bool>,
) -> io::Result<()> {
    let mut connections = JoinSet::new();
    let result = loop {
        tokio::select! {
            _ = async { let _ = stopped.wait_for(|stop| *stop).await; } => break Ok(()),
            accepted = listener.accept() => {
                let (stream, _) = match accepted {
                    Ok(accepted) => accepted,
                    Err(error) => break Err(error),
                };
                let app = app.clone();
                let mut stopped = stopped.clone();
                connections.spawn(async move {
                    // Axum's enabled protocol is HTTP/1. Own the connection
                    // future directly: axum::serve detaches connection tasks,
                    // which cannot be force-cancelled for a stuck handler.
                    let connection = hyper::server::conn::http1::Builder::new()
                        .serve_connection(
                            hyper_util::rt::TokioIo::new(stream),
                            hyper_util::service::TowerToHyperService::new(app),
                        );
                    tokio::pin!(connection);
                    tokio::select! {
                        result = &mut connection => { let _ = result; }
                        _ = async { let _ = stopped.wait_for(|stop| *stop).await; } => {
                            connection.as_mut().graceful_shutdown();
                            let _ = connection.await;
                        }
                    }
                });
            }
            joined = connections.join_next(), if !connections.is_empty() => {
                if let Some(Err(error)) = joined {
                    break Err(io::Error::other(error));
                }
            }
        }
    };
    drop(listener);
    let drained = tokio::time::timeout(Duration::from_secs(5), async {
        while let Some(connection) = connections.join_next().await {
            connection.map_err(io::Error::other)?;
        }
        Ok::<_, io::Error>(())
    })
    .await;
    // Abort and join every remaining connection, including pending handlers
    // and streaming bodies, before returning even if the runtime stays alive.
    connections.shutdown().await;
    result?;
    drained.map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "fixture shutdown timed out"))?
}

pub async fn serve(
    endpoints: Vec<(TcpListener, Router)>,
    shutdown: impl Future<Output = io::Result<()>>,
) -> io::Result<()> {
    let (stop, _) = watch::channel(false);
    let mut servers = JoinSet::new();
    for (listener, app) in endpoints {
        servers.spawn(serve_listener(listener, app, stop.subscribe()));
    }
    let result = tokio::select! {
        result = shutdown => result,
        result = servers.join_next() => Err(io::Error::other(
            format!("fixture listener stopped unexpectedly: {result:?}"),
        )),
    };
    // Sender drop also wakes every connection on supervisor unwind. Both
    // JoinSet levels abort their owned tasks if their enclosing future drops.
    let _ = stop.send(true);
    let mut drain_error = None;
    while let Some(server) = servers.join_next().await {
        let result = server.map_err(io::Error::other).and_then(|result| result);
        if let Err(error) = result {
            drain_error.get_or_insert(error);
        }
    }
    result?;
    match drain_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}
