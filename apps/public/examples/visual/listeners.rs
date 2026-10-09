//! Transport contract for synthetic browser fixtures, never production listeners.
use std::{ffi::OsStr, future::Future, io, net::Ipv4Addr, net::Ipv6Addr, time::Duration};

use axum::Router;
use tokio::{net::TcpListener, sync::watch, task::JoinSet};

#[path = "lifecycle.rs"]
mod lifecycle;

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

async fn serve_connection(
    accepted: (tokio::net::TcpStream, lifecycle::Connection),
    app: Router,
    mut stopped: watch::Receiver<bool>,
) {
    let owned = accepted.1;
    let result = {
        // Axum's enabled protocol is HTTP/1. Own this future directly so a
        // stuck handler can be cancelled with the containing connection task.
        let connection = hyper::server::conn::http1::Builder::new().serve_connection(
            hyper_util::rt::TokioIo::new(accepted.0),
            hyper_util::service::TowerToHyperService::new(app),
        );
        tokio::pin!(connection);
        tokio::select! {
            result = &mut connection => result,
            _ = async { let _ = stopped.wait_for(|stop| *stop).await; } => {
                connection.as_mut().graceful_shutdown();
                connection.await
            }
        }
    }; // Release the connection and its stream before ending ownership.
    owned.finish(result);
}

async fn serve_listener(
    listener: TcpListener,
    app: Router,
    mut stopped: watch::Receiver<bool>,
    lifecycle: lifecycle::Lifecycle,
) -> io::Result<()> {
    let mut connections = JoinSet::new();
    let result = loop {
        tokio::select! {
            _ = async { let _ = stopped.wait_for(|stop| *stop).await; } => break Ok(()),
            accepted = listener.accept() => {
                let (stream, _) = match accepted {
                    Ok(accepted) => accepted,
                    Err(error) => { lifecycle.accept_error(); break Err(error); },
                };
                // Tuple fields drop in order if the task is cancelled before
                // its first poll: release the stream before its counter guard.
                connections.spawn(serve_connection(
                    (stream, lifecycle.accepted()), app.clone(), stopped.clone(),
                ));
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
    let lifecycle = lifecycle::Lifecycle::from_env();
    let (stop, _) = watch::channel(false);
    let mut servers = JoinSet::new();
    for (listener, app) in endpoints {
        servers.spawn(serve_listener(
            listener,
            app,
            stop.subscribe(),
            lifecycle.clone(),
        ));
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
    lifecycle.shutdown();
    result?;
    match drain_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use tokio::io::AsyncReadExt;

    #[tokio::test]
    async fn cancelling_never_polled_connection_releases_stream_and_ownership() {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await.unwrap();
        let mut client = tokio::net::TcpStream::connect(listener.local_addr().unwrap())
            .await
            .unwrap();
        let (stream, _) = listener.accept().await.unwrap();
        let lifecycle = lifecycle::Lifecycle::for_test();
        let (_stop, stopped) = watch::channel(false);
        // Construct and drop the future without ever polling or spawning it.
        let connection = serve_connection((stream, lifecycle.accepted()), Router::new(), stopped);
        assert_eq!(lifecycle.test_counts(), (1, 1, 0));
        drop(connection);
        assert_eq!(lifecycle.test_counts(), (1, 0, 1));
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(3), client.read(&mut [0]))
                .await
                .unwrap()
                .unwrap(),
            0
        );
    }
}
