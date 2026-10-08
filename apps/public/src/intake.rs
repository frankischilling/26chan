//! Public configuration and response policy around the shared bounded transport.
use crate::handlers::AppError;
use axum::{body::Body, http::StatusCode};
use board_config::PublicMediaSettings;
use board_intake_client::IntakeError;
pub use board_intake_client::{Reservation, Status};

#[derive(Clone)]
pub struct IntakeClient {
    pub settings: PublicMediaSettings,
}

pub fn unavailable() -> AppError {
    map_error(IntakeError::Unavailable)
}

fn map_error(error: IntakeError) -> AppError {
    match error {
        IntakeError::Unavailable => AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Media intake is unavailable. Try again later.",
        ),
        IntakeError::NotFound => {
            AppError(StatusCode::NOT_FOUND, "Upload is unavailable or expired.")
        }
        IntakeError::Conflict => AppError(
            StatusCode::CONFLICT,
            "Upload is not available in its current state.",
        ),
        IntakeError::PayloadTooLarge => AppError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "The file exceeds the 8 MiB upload limit.",
        ),
        IntakeError::Rejected => {
            AppError(StatusCode::UNPROCESSABLE_ENTITY, "The upload was rejected.")
        }
    }
}

impl IntakeClient {
    fn transport(&self) -> Result<board_intake_client::IntakeClient, AppError> {
        board_intake_client::IntakeClient::new(self.settings.intake, self.settings.token.clone())
            .map_err(map_error)
    }

    pub async fn ready(&self) -> Result<(), AppError> {
        self.transport()?.ready().await.map_err(map_error)
    }

    pub async fn reserve(&self, filename: &str) -> Result<Reservation, AppError> {
        self.transport()?.reserve(filename).await.map_err(map_error)
    }

    pub async fn upload(&self, id: &str, capability: &str, body: Body) -> Result<(), AppError> {
        self.transport()?
            .upload(id, capability, body)
            .await
            .map_err(map_error)
    }

    pub async fn status(&self, id: &str, capability: &str) -> Result<Status, AppError> {
        self.transport()?
            .status(id, capability)
            .await
            .map_err(map_error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
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
