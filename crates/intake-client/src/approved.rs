//! Cookie-free reads of already-approved representations from one numeric endpoint.
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use std::{net::SocketAddr, time::Duration};

#[derive(Clone)]
pub struct ApprovedMediaClient {
    endpoint: SocketAddr,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
#[error("Approved media is unavailable.")]
pub struct ReadError;

/// Metadata comes from the scoped database plan, never a submitted URL.
pub struct ApprovedRepresentation<'a> {
    pub board: &'a str,
    pub tim: i64,
    pub suffix: &'a str,
    pub content_type: &'a str,
    pub bytes: usize,
}
impl ApprovedRepresentation<'_> {
    fn path(&self) -> Result<String, ReadError> {
        if self.board.is_empty()
            || self.board.len() > 10
            || !self
                .board
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            || self.tim <= 0
        {
            return Err(ReadError);
        }
        let limit = match (self.suffix, self.content_type) {
            (".png", "image/png") | ("s.jpg", "image/png" | "image/jpeg") => 5_242_880,
            (".jpg", "image/jpeg") => 10_485_760,
            (".gif", "image/gif") => 20_971_520,
            (".webm", "video/webm") | (".mp4", "video/mp4") | (".pdf", "application/pdf") => {
                8_388_608
            }
            _ => return Err(ReadError),
        };
        if self.bytes == 0 || self.bytes > limit {
            return Err(ReadError);
        }
        Ok(format!("/{}/{}{}", self.board, self.tim, self.suffix))
    }
}
impl ApprovedMediaClient {
    /// Isolated development transport. Production requires a separate reviewed
    /// endpoint policy; this client cannot resolve names or contact remote hosts.
    pub fn new(endpoint: SocketAddr) -> Result<Self, ReadError> {
        if !endpoint.ip().is_loopback() || endpoint.port() == 0 {
            return Err(ReadError);
        }
        Ok(Self { endpoint })
    }
    pub async fn read(&self, expected: &ApprovedRepresentation<'_>) -> Result<Vec<u8>, ReadError> {
        let path = expected.path()?;
        let request = Request::builder()
            .uri(path)
            .header("host", self.endpoint.to_string())
            .header("accept", expected.content_type)
            .header("accept-encoding", "identity")
            .body(Body::empty())
            .map_err(|_| ReadError)?;
        let exchange = async {
            let socket = tokio::time::timeout(
                Duration::from_secs(2),
                tokio::net::TcpStream::connect(self.endpoint),
            )
            .await
            .map_err(|_| ReadError)?
            .map_err(|_| ReadError)?;
            let (mut sender, connection) = hyper::client::conn::http1::Builder::new()
                .max_buf_size(16_384)
                .handshake(hyper_util::rt::TokioIo::new(socket))
                .await
                .map_err(|_| ReadError)?;
            let response = async {
                let response = sender.send_request(request).await.map_err(|_| ReadError)?;
                if response.status() != StatusCode::OK
                    || response
                        .headers()
                        .get("content-type")
                        .is_none_or(|v| v != expected.content_type)
                    || response.headers().contains_key("content-encoding")
                    || response.headers().get("content-length").is_some_and(|v| {
                        v.to_str().ok().and_then(|s| s.parse::<usize>().ok())
                            != Some(expected.bytes)
                    })
                {
                    return Err(ReadError);
                }
                let bytes = to_bytes(Body::new(response.into_body()), expected.bytes)
                    .await
                    .map_err(|_| ReadError)?;
                if bytes.len() != expected.bytes {
                    return Err(ReadError);
                }
                Ok(bytes.to_vec())
            };
            // No detached connection tasks survive cancellation or timeout.
            tokio::pin!(connection);
            tokio::pin!(response);
            tokio::select! { biased; result=&mut response=>result, _=&mut connection=>response.await }
        };
        tokio::time::timeout(Duration::from_secs(8), exchange)
            .await
            .map_err(|_| ReadError)?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    fn image() -> ApprovedRepresentation<'static> {
        ApprovedRepresentation {
            board: "b",
            tim: 123,
            suffix: ".png",
            content_type: "image/png",
            bytes: 4,
        }
    }
    #[test]
    fn canonical_descriptors_and_local_endpoints() {
        assert_eq!(image().path().unwrap(), "/b/123.png");
        for board in ["", "B", "../a", "a?b", "abcdefghijkl"] {
            assert!(ApprovedRepresentation { board, ..image() }.path().is_err());
        }
        for suffix in [".svg", ".png?x", ".png/../x", ".html"] {
            assert!(ApprovedRepresentation { suffix, ..image() }.path().is_err());
        }
        assert!(
            ApprovedRepresentation {
                bytes: 5_242_881,
                ..image()
            }
            .path()
            .is_err()
        );
        assert!(
            ApprovedRepresentation {
                bytes: 0,
                ..image()
            }
            .path()
            .is_err()
        );
        assert!(ApprovedRepresentation { tim: 0, ..image() }.path().is_err());
        assert!(
            ApprovedRepresentation {
                content_type: "text/html",
                ..image()
            }
            .path()
            .is_err()
        );
        for endpoint in ["192.0.2.1:80", "0.0.0.0:80", "127.0.0.1:0"] {
            assert!(ApprovedMediaClient::new(endpoint.parse().unwrap()).is_err());
        }
    }
    async fn exchange(raw: &'static [u8]) -> Result<Vec<u8>, ReadError> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = ApprovedMediaClient::new(listener.local_addr().unwrap()).unwrap();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                socket.read_exact(&mut byte).await.unwrap();
                request.push(byte[0]);
                assert!(request.len() < 4096);
            }
            let request = String::from_utf8(request).unwrap().to_lowercase();
            assert!(request.starts_with("get /b/123.png http/1.1\r\n"));
            for forbidden in ["authorization:", "cookie:", "referer:"] {
                assert!(!request.contains(forbidden));
            }
            socket.write_all(raw).await.unwrap();
        });
        let result = client.read(&image()).await;
        server.await.unwrap();
        result
    }
    #[tokio::test]
    async fn reads_only_complete_expected_representation() {
        assert_eq!(
            exchange(
                b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: 4\r\n\r\ntest"
            )
            .await
            .unwrap(),
            b"test"
        );
        for response in [
            &b"HTTP/1.1 302 Found\r\nLocation: http://192.0.2.1/secret\r\nContent-Length: 0\r\n\r\n"[..],
            &b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: 4\r\n\r\ntest"[..],
            &b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: 4\r\nContent-Encoding: gzip\r\n\r\ntest"[..],
            &b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: 5\r\n\r\ntests"[..],
            &b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: 4\r\n\r\ntes"[..],
            &b"HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nTransfer-Encoding: chunked\r\n\r\n5\r\ntests\r\n0\r\n\r\n"[..],
        ] { assert!(exchange(response).await.is_err()); }
    }
    #[tokio::test]
    async fn cancellation_closes_the_only_connection() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = ApprovedMediaClient::new(listener.local_addr().unwrap()).unwrap();
        let (sent, received) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                let mut byte = [0];
                socket.read_exact(&mut byte).await.unwrap();
                request.push(byte[0]);
                assert!(request.len() < 4096);
            }
            sent.send(()).unwrap();
            let mut byte = [0];
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(2), socket.read(&mut byte))
                    .await
                    .unwrap()
                    .unwrap(),
                0
            );
        });
        let read = tokio::spawn(async move { client.read(&image()).await });
        received.await.unwrap();
        read.abort();
        assert!(read.await.unwrap_err().is_cancelled());
        server.await.unwrap();
    }
}
