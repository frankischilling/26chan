//! Fixed-endpoint authenticated transport. Never decodes media or follows URLs.
use crate::handlers::AppError;
use axum::{
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use board_config::PublicMediaSettings;
use serde::Deserialize;
use std::time::Duration;

#[derive(Clone)]
pub struct IntakeClient {
    pub settings: PublicMediaSettings,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn fixture(response: Vec<u8>) -> (IntakeClient, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = IntakeClient {
            settings: PublicMediaSettings::development(
                &listener.local_addr().unwrap().to_string(),
                &"a".repeat(64),
                "http://localhost:3002",
            )
            .unwrap(),
        };
        let server = tokio::spawn(async move {
            let work = async {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut headers = Vec::new();
                while !headers.ends_with(b"\r\n\r\n") {
                    headers.push(socket.read_u8().await.unwrap());
                    assert!(headers.len() < 4096);
                }
                let headers = String::from_utf8(headers).unwrap();
                assert!(headers.starts_with("GET /readyz HTTP/1.1\r\n"));
                assert!(headers.contains(&format!("authorization: Bearer {}\r\n", "a".repeat(64))));
                // Peer errors are expected when the client rejects an oversized
                // header/body. No server task is detached after the assertion.
                let _ = socket.write_all(&response).await;
                let _ = socket.shutdown().await;
            };
            tokio::time::timeout(Duration::from_secs(5), work)
                .await
                .unwrap();
        });
        (client, server)
    }

    fn response(status: &str, content_type: &str, body: &str) -> Vec<u8> {
        format!("HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).into_bytes()
    }

    #[tokio::test]
    async fn authenticated_transport_requires_bounded_well_formed_success() {
        let (client, server) =
            fixture(response("200 OK", "application/json", r#"{"status":"ok"}"#)).await;
        client.ready().await.unwrap();
        server.await.unwrap();
        let cases = [
            response("200 OK", "text/html", r#"{"status":"ok"}"#),
            response("200 OK", "application/json", "invalid JSON"),
            response("200 OK", "application/json", r#"{"status":"wrong"}"#),
            response("200 OK", "application/json", r#"{"status":"ok","unexpected":true}"#),
            response("200 OK", "application/json", &" ".repeat(4097)),
            response("401 Unauthorized", "application/json", "private failure"),
            b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/credential-leak\r\nContent-Length: 0\r\n\r\n".to_vec(),
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 500\r\n\r\n{}".to_vec(),
            format!("HTTP/1.1 200 OK\r\nX-Excessive: {}\r\n\r\n", "x".repeat(20_000)).into_bytes(),
            Vec::new(),
        ];
        for response in cases {
            let (client, server) = fixture(response).await;
            let error = client.ready().await.unwrap_err();
            assert_eq!(error.0, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(error.1, unavailable().1);
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn invalid_capabilities_never_open_a_connection() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = IntakeClient {
            settings: PublicMediaSettings::development(
                &listener.local_addr().unwrap().to_string(),
                &"a".repeat(64),
                "http://localhost:3002",
            )
            .unwrap(),
        };
        for (id, capability) in [
            ("../readyz".into(), "a".repeat(64)),
            ("b".repeat(32), "\r\nother: header".into()),
        ] {
            assert_eq!(
                client
                    .upload(&id, &capability, Body::empty())
                    .await
                    .unwrap_err()
                    .0,
                StatusCode::NOT_FOUND
            );
            assert!(matches!(
                client.status(&id, &capability).await,
                Err(AppError(StatusCode::NOT_FOUND, _))
            ));
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reservation {
    pub id: String,
    pub capability: String,
    state: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Status {
    pub id: String,
    pub state: String,
    pub input_bytes: Option<u64>,
    pub output_id: Option<String>,
}

pub fn valid_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub fn unavailable() -> AppError {
    AppError(
        StatusCode::SERVICE_UNAVAILABLE,
        "Media intake is unavailable. Try again later.",
    )
}

impl IntakeClient {
    async fn request(
        &self,
        method: Method,
        path: &str,
        capability: Option<&str>,
        body: Body,
    ) -> Result<Vec<u8>, AppError> {
        let mut request = Request::builder()
            .method(method.clone())
            .uri(path)
            .header("host", self.settings.intake.to_string())
            .header("authorization", format!("Bearer {}", self.settings.token));
        if method == Method::POST {
            request = request.header("content-type", "application/json");
        }
        if method == Method::PUT {
            request = request.header("content-type", "application/octet-stream");
        }
        if let Some(capability) = capability {
            request = request.header("upload-capability", capability);
        }
        let request = request.body(body).map_err(|_| unavailable())?;
        let exchange = async {
            let socket = tokio::time::timeout(
                Duration::from_secs(2),
                tokio::net::TcpStream::connect(self.settings.intake),
            )
            .await
            .map_err(|_| unavailable())?
            .map_err(|_| unavailable())?;
            let (mut sender, connection) = hyper::client::conn::http1::Builder::new()
                .max_buf_size(16_384)
                .handshake(hyper_util::rt::TokioIo::new(socket))
                .await
                .map_err(|_| unavailable())?;
            let response = async {
                let response = sender
                    .send_request(request)
                    .await
                    .map_err(|_| unavailable())?;
                let status = response.status();
                if !status.is_success() {
                    return Err(match status {
                        StatusCode::NOT_FOUND => {
                            AppError(StatusCode::NOT_FOUND, "Upload is unavailable or expired.")
                        }
                        StatusCode::CONFLICT => AppError(
                            StatusCode::CONFLICT,
                            "Upload is not available in its current state.",
                        ),
                        StatusCode::PAYLOAD_TOO_LARGE => {
                            AppError(status, "The file exceeds the 8 MiB upload limit.")
                        }
                        StatusCode::UNPROCESSABLE_ENTITY | StatusCode::BAD_REQUEST => {
                            AppError(StatusCode::UNPROCESSABLE_ENTITY, "The upload was rejected.")
                        }
                        _ => unavailable(),
                    });
                }
                if response
                    .headers()
                    .get("content-type")
                    .is_none_or(|h| h != "application/json")
                {
                    return Err(unavailable());
                }
                let body = to_bytes(Body::new(response.into_body()), 4096)
                    .await
                    .map_err(|_| unavailable())?;
                Ok(body.to_vec())
            };
            // Drive the connection and bounded response together. Cancellation
            // drops the socket; there is no detached client task or retry.
            tokio::pin!(connection);
            tokio::pin!(response);
            tokio::select! {
                biased;
                result = &mut response => result,
                _ = &mut connection => response.await,
            }
        };
        tokio::time::timeout(Duration::from_secs(18), exchange)
            .await
            .map_err(|_| unavailable())?
    }

    pub async fn ready(&self) -> Result<(), AppError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Ready {
            status: String,
        }
        let body = self
            .request(Method::GET, "/readyz", None, Body::empty())
            .await?;
        let ready: Ready = serde_json::from_slice(&body).map_err(|_| unavailable())?;
        if ready.status != "ok" {
            return Err(unavailable());
        }
        Ok(())
    }

    pub async fn reserve(&self, filename: &str) -> Result<Reservation, AppError> {
        let body = serde_json::to_vec(&serde_json::json!({"filename":filename}))
            .map_err(|_| unavailable())?;
        let body = self
            .request(Method::POST, "/v1/reservations", None, Body::from(body))
            .await?;
        let value: Reservation = serde_json::from_slice(&body).map_err(|_| unavailable())?;
        if !valid_hex(&value.id, 32)
            || !valid_hex(&value.capability, 64)
            || value.state != "receiving"
        {
            return Err(unavailable());
        }
        Ok(value)
    }

    pub async fn upload(&self, id: &str, capability: &str, body: Body) -> Result<(), AppError> {
        if !valid_hex(id, 32) || !valid_hex(capability, 64) {
            return Err(AppError(
                StatusCode::NOT_FOUND,
                "Upload is unavailable or expired.",
            ));
        }
        let body = self
            .request(
                Method::PUT,
                &format!("/v1/uploads/{id}"),
                Some(capability),
                body,
            )
            .await?;
        let status: Status = serde_json::from_slice(&body).map_err(|_| unavailable())?;
        if status.id != id
            || status.state != "queued"
            || status
                .input_bytes
                .is_none_or(|b| !(1..=8_388_608).contains(&b))
            || status.output_id.is_some()
        {
            return Err(unavailable());
        }
        Ok(())
    }

    pub async fn status(&self, id: &str, capability: &str) -> Result<Status, AppError> {
        if !valid_hex(id, 32) || !valid_hex(capability, 64) {
            return Err(AppError(
                StatusCode::NOT_FOUND,
                "Upload is unavailable or expired.",
            ));
        }
        let body = self
            .request(
                Method::GET,
                &format!("/v1/uploads/{id}"),
                Some(capability),
                Body::empty(),
            )
            .await?;
        let value: Status = serde_json::from_slice(&body).map_err(|_| unavailable())?;
        if value.id != id
            || !matches!(
                value.state.as_str(),
                "receiving" | "uploading" | "queued" | "processing" | "published" | "failed"
            )
            || value
                .output_id
                .as_ref()
                .is_some_and(|id| !valid_hex(id, 32))
        {
            return Err(unavailable());
        }
        Ok(value)
    }
}
