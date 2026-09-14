use axum::{
    extract::{ConnectInfo, Request},
    http::StatusCode,
};
use std::net::{IpAddr, SocketAddr};

/// Only the listener constructs this from kernel peer credentials.
#[derive(Clone, Copy)]
pub(crate) struct UnixPeer(pub Option<u32>);

#[cfg(target_os = "linux")]
impl
    axum::extract::connect_info::Connected<
        axum::serve::IncomingStream<'_, tokio::net::UnixListener>,
    > for UnixPeer
{
    fn connect_info(stream: axum::serve::IncomingStream<'_, tokio::net::UnixListener>) -> Self {
        Self(stream.io().peer_cred().ok().map(|cred| cred.uid()))
    }
}

pub(crate) fn resolve(
    request: &Request,
    proxy_uid: Option<u32>,
) -> Result<Option<IpAddr>, StatusCode> {
    let Some(expected) = proxy_uid else {
        return Ok(request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|peer| peer.0.ip().to_canonical()));
    };
    if request
        .extensions()
        .get::<ConnectInfo<UnixPeer>>()
        .and_then(|peer| peer.0.0)
        != Some(expected)
    {
        return Err(StatusCode::FORBIDDEN);
    }
    let mut values = request.headers().get_all("x-board-client-ip").iter();
    let value = values
        .next()
        .and_then(|value| value.to_str().ok())
        .ok_or(StatusCode::BAD_REQUEST)?;
    if values.next().is_some() || value.is_empty() || value.len() > 45 {
        return Err(StatusCode::BAD_REQUEST);
    }
    value
        .parse::<IpAddr>()
        .map(|peer| Some(peer.to_canonical()))
        .map_err(|_| StatusCode::BAD_REQUEST)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    #[test]
    fn only_a_verified_unix_peer_can_supply_one_canonical_client_address() {
        let mut request = Request::builder()
            .header("x-board-client-ip", "::ffff:192.0.2.10")
            .header("x-forwarded-for", "198.51.100.8")
            .body(Body::empty())
            .unwrap();
        request
            .extensions_mut()
            .insert(ConnectInfo("127.0.0.1:1234".parse::<SocketAddr>().unwrap()));
        assert_eq!(
            resolve(&request, None).unwrap(),
            Some("127.0.0.1".parse().unwrap())
        );
        assert_eq!(resolve(&request, Some(33)), Err(StatusCode::FORBIDDEN));
        request
            .extensions_mut()
            .insert(ConnectInfo(UnixPeer(Some(34))));
        assert_eq!(resolve(&request, Some(33)), Err(StatusCode::FORBIDDEN));
        request
            .extensions_mut()
            .insert(ConnectInfo(UnixPeer(Some(33))));
        assert_eq!(
            resolve(&request, Some(33)).unwrap(),
            Some("192.0.2.10".parse().unwrap())
        );
        for value in [
            "",
            "192.0.2.1,192.0.2.2",
            "192.0.2.1:80",
            "[::1]",
            "fe80::1%eth0",
            " 192.0.2.1",
            "unknown",
        ] {
            request
                .headers_mut()
                .insert("x-board-client-ip", value.parse().unwrap());
            assert_eq!(resolve(&request, Some(33)), Err(StatusCode::BAD_REQUEST));
        }
        request
            .headers_mut()
            .insert("x-board-client-ip", "192.0.2.1".parse().unwrap());
        request
            .headers_mut()
            .append("x-board-client-ip", "192.0.2.2".parse().unwrap());
        assert_eq!(resolve(&request, Some(33)), Err(StatusCode::BAD_REQUEST));
        request.headers_mut().remove("x-board-client-ip");
        assert_eq!(resolve(&request, Some(33)), Err(StatusCode::BAD_REQUEST));
    }
}
