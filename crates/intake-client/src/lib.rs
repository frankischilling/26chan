//! Fixed-endpoint authenticated transport. Never decodes media or follows URLs.
use axum::{
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use serde::Deserialize;
use std::{net::SocketAddr, time::Duration};

/// An authenticated numeric endpoint. No Debug implementation exposes its token.
#[derive(Clone)]
pub struct IntakeClient {
    intake: SocketAddr,
    token: String,
}

/// Deliberately excludes upstream response bodies and transport diagnostics.
#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum IntakeError {
    #[error("Media intake is unavailable.")]
    Unavailable,
    #[error("Upload is unavailable or expired.")]
    NotFound,
    #[error("Upload is not available in its current state.")]
    Conflict,
    #[error("The file exceeds the 8 MiB upload limit.")]
    PayloadTooLarge,
    #[error("The upload was rejected.")]
    Rejected,
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
impl IntakeClient {
    /// Endpoint allowlists belong to the caller's configuration policy. This
    /// transport accepts numeric addresses only and never resolves hostnames.
    pub fn new(intake: SocketAddr, token: String) -> Result<Self, IntakeError> {
        if intake.port() == 0 || !valid_hex(&token, 64) {
            return Err(IntakeError::Unavailable);
        }
        Ok(Self { intake, token })
    }

    async fn request(
        &self,
        method: Method,
        path: &str,
        capability: Option<&str>,
        body: Body,
    ) -> Result<Vec<u8>, IntakeError> {
        let mut request = Request::builder()
            .method(method.clone())
            .uri(path)
            .header("host", self.intake.to_string())
            .header("authorization", format!("Bearer {}", self.token));
        if method == Method::POST {
            request = request.header("content-type", "application/json");
        }
        if method == Method::PUT {
            request = request.header("content-type", "application/octet-stream");
        }
        if let Some(capability) = capability {
            request = request.header("upload-capability", capability);
        }
        let request = request.body(body).map_err(|_| IntakeError::Unavailable)?;
        let exchange = async {
            let socket = tokio::time::timeout(
                Duration::from_secs(2),
                tokio::net::TcpStream::connect(self.intake),
            )
            .await
            .map_err(|_| IntakeError::Unavailable)?
            .map_err(|_| IntakeError::Unavailable)?;
            let (mut sender, connection) = hyper::client::conn::http1::Builder::new()
                .max_buf_size(16_384)
                .handshake(hyper_util::rt::TokioIo::new(socket))
                .await
                .map_err(|_| IntakeError::Unavailable)?;
            let response = async {
                let response = sender
                    .send_request(request)
                    .await
                    .map_err(|_| IntakeError::Unavailable)?;
                let status = response.status();
                if !status.is_success() {
                    return Err(match status {
                        StatusCode::NOT_FOUND => IntakeError::NotFound,
                        StatusCode::CONFLICT => IntakeError::Conflict,
                        StatusCode::PAYLOAD_TOO_LARGE => IntakeError::PayloadTooLarge,
                        StatusCode::UNPROCESSABLE_ENTITY | StatusCode::BAD_REQUEST => {
                            IntakeError::Rejected
                        }
                        _ => IntakeError::Unavailable,
                    });
                }
                if response
                    .headers()
                    .get("content-type")
                    .is_none_or(|h| h != "application/json")
                {
                    return Err(IntakeError::Unavailable);
                }
                let body = to_bytes(Body::new(response.into_body()), 4096)
                    .await
                    .map_err(|_| IntakeError::Unavailable)?;
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
            .map_err(|_| IntakeError::Unavailable)?
    }

    pub async fn ready(&self) -> Result<(), IntakeError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Ready {
            status: String,
        }
        let body = self
            .request(Method::GET, "/readyz", None, Body::empty())
            .await?;
        let ready: Ready = serde_json::from_slice(&body).map_err(|_| IntakeError::Unavailable)?;
        if ready.status != "ok" {
            return Err(IntakeError::Unavailable);
        }
        Ok(())
    }

    pub async fn reserve(&self, filename: &str) -> Result<Reservation, IntakeError> {
        let body = serde_json::to_vec(&serde_json::json!({"filename":filename}))
            .map_err(|_| IntakeError::Unavailable)?;
        let body = self
            .request(Method::POST, "/v1/reservations", None, Body::from(body))
            .await?;
        let value: Reservation =
            serde_json::from_slice(&body).map_err(|_| IntakeError::Unavailable)?;
        if !valid_hex(&value.id, 32)
            || !valid_hex(&value.capability, 64)
            || value.state != "receiving"
        {
            return Err(IntakeError::Unavailable);
        }
        Ok(value)
    }

    pub async fn upload(&self, id: &str, capability: &str, body: Body) -> Result<(), IntakeError> {
        if !valid_hex(id, 32) || !valid_hex(capability, 64) {
            return Err(IntakeError::NotFound);
        }
        let body = self
            .request(
                Method::PUT,
                &format!("/v1/uploads/{id}"),
                Some(capability),
                body,
            )
            .await?;
        let status: Status = serde_json::from_slice(&body).map_err(|_| IntakeError::Unavailable)?;
        if status.id != id
            || status.state != "queued"
            || status
                .input_bytes
                .is_none_or(|b| !(1..=8_388_608).contains(&b))
            || status.output_id.is_some()
        {
            return Err(IntakeError::Unavailable);
        }
        Ok(())
    }

    /// Inactive v2 endpoint contract; production intake does not register it yet.
    pub async fn reserve_pair(&self, filename: &str) -> Result<Reservation, IntakeError> {
        let body = serde_json::to_vec(&serde_json::json!({"filename":filename}))
            .map_err(|_| IntakeError::Unavailable)?;
        let body = self
            .request(Method::POST, "/v2/reservations", None, Body::from(body))
            .await?;
        let value: Reservation =
            serde_json::from_slice(&body).map_err(|_| IntakeError::Unavailable)?;
        if !valid_hex(&value.id, 32)
            || !valid_hex(&value.capability, 64)
            || value.state != "receiving"
        {
            return Err(IntakeError::Unavailable);
        }
        Ok(value)
    }

    pub async fn upload_pair(
        &self,
        id: &str,
        capability: &str,
        body: Body,
    ) -> Result<(), IntakeError> {
        if !valid_hex(id, 32) || !valid_hex(capability, 64) {
            return Err(IntakeError::NotFound);
        }
        let body = self
            .request(
                Method::PUT,
                &format!("/v2/uploads/{id}"),
                Some(capability),
                body,
            )
            .await?;
        let value: Status = serde_json::from_slice(&body).map_err(|_| IntakeError::Unavailable)?;
        if value.id != id
            || value.state != "queued"
            || value.output_id.is_some()
            || value
                .input_bytes
                .is_none_or(|b| !(57..=16_777_272).contains(&b))
        {
            return Err(IntakeError::Unavailable);
        }
        Ok(())
    }

    pub async fn status(&self, id: &str, capability: &str) -> Result<Status, IntakeError> {
        if !valid_hex(id, 32) || !valid_hex(capability, 64) {
            return Err(IntakeError::NotFound);
        }
        let body = self
            .request(
                Method::GET,
                &format!("/v1/uploads/{id}"),
                Some(capability),
                Body::empty(),
            )
            .await?;
        let value: Status = serde_json::from_slice(&body).map_err(|_| IntakeError::Unavailable)?;
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
            return Err(IntakeError::Unavailable);
        }
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn fixture(
        response: Vec<u8>,
        method: &'static str,
        path: String,
        capability: Option<String>,
    ) -> (IntakeClient, tokio::task::JoinHandle<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = IntakeClient::new(listener.local_addr().unwrap(), "a".repeat(64)).unwrap();
        let server = tokio::spawn(async move {
            let work = async {
                let (mut socket, _) = listener.accept().await.unwrap();
                let mut headers = Vec::new();
                while !headers.ends_with(b"\r\n\r\n") {
                    headers.push(socket.read_u8().await.unwrap());
                    assert!(headers.len() < 4096);
                }
                let headers = String::from_utf8(headers).unwrap();
                assert!(headers.starts_with(&format!("{method} {path} HTTP/1.1\r\n")));
                assert!(headers.contains(&format!("authorization: Bearer {}\r\n", "a".repeat(64))));
                if let Some(capability) = capability {
                    assert!(headers.contains(&format!("upload-capability: {capability}\r\n")));
                }
                if let Some(length) = headers.lines().find_map(|line| {
                    line.strip_prefix("content-length: ")
                        .map(|value| value.parse::<usize>().unwrap())
                }) {
                    assert!(length <= 4096);
                    socket.read_exact(&mut vec![0; length]).await.unwrap();
                }
                // The client may close early when it rejects headers or status.
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

    #[test]
    fn construction_rejects_invalid_credentials_and_zero_port() {
        for token in [
            "".into(),
            "A".repeat(64),
            "a".repeat(63),
            "a".repeat(65),
            "\r\n".repeat(32),
        ] {
            assert!(matches!(
                IntakeClient::new("127.0.0.1:4000".parse().unwrap(), token),
                Err(IntakeError::Unavailable)
            ));
        }
        assert!(matches!(
            IntakeClient::new("127.0.0.1:0".parse().unwrap(), "a".repeat(64)),
            Err(IntakeError::Unavailable)
        ));
        // The shared transport leaves environment-specific address policy to callers.
        assert!(IntakeClient::new("192.0.2.1:4000".parse().unwrap(), "a".repeat(64)).is_ok());
    }

    #[tokio::test]
    async fn stalled_exchange_times_out_and_closes_its_socket() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = IntakeClient::new(listener.local_addr().unwrap(), "a".repeat(64)).unwrap();
        let request = tokio::spawn(async move { client.ready().await });
        let (mut socket, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
            .await
            .unwrap()
            .unwrap();
        let mut headers = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), async {
            while !headers.ends_with(b"\r\n\r\n") {
                headers.push(socket.read_u8().await.unwrap());
                assert!(headers.len() < 4096);
            }
        })
        .await
        .unwrap();
        tokio::time::pause();
        tokio::time::advance(Duration::from_secs(19)).await;
        assert_eq!(request.await.unwrap(), Err(IntakeError::Unavailable));
        tokio::time::resume();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(5), socket.read(&mut [0; 1]))
                .await
                .unwrap()
                .unwrap(),
            0
        );
    }

    #[tokio::test]
    async fn readiness_requires_bounded_json_and_never_follows_redirects() {
        let (client, server) = fixture(
            response("200 OK", "application/json", r#"{"status":"ok"}"#),
            "GET",
            "/readyz".into(),
            None,
        )
        .await;
        client.ready().await.unwrap();
        server.await.unwrap();
        for body in [
            response("200 OK", "text/html", r#"{"status":"ok"}"#),
            response("200 OK", "application/json", "invalid JSON"),
            response("200 OK", "application/json", r#"{"status":"wrong"}"#),
            response("200 OK", "application/json", r#"{"status":"ok","unexpected":true}"#),
            response("200 OK", "application/json", &" ".repeat(4097)),
            b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/credential-leak\r\nContent-Length: 0\r\n\r\n".to_vec(),
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 500\r\n\r\n{}".to_vec(),
            format!("HTTP/1.1 200 OK\r\nX-Excessive: {}\r\n\r\n", "x".repeat(20_000)).into_bytes(),
            Vec::new(),
        ] {
            let (client, server) = fixture(body, "GET", "/readyz".into(), None).await;
            assert_eq!(client.ready().await, Err(IntakeError::Unavailable));
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn error_responses_are_bounded_categories_without_upstream_details() {
        for (status, error) in [
            ("404 Not Found", IntakeError::NotFound),
            ("409 Conflict", IntakeError::Conflict),
            ("413 Payload Too Large", IntakeError::PayloadTooLarge),
            ("400 Bad Request", IntakeError::Rejected),
            ("422 Unprocessable Entity", IntakeError::Rejected),
            ("401 Unauthorized", IntakeError::Unavailable),
            ("503 Service Unavailable", IntakeError::Unavailable),
            ("302 Found", IntakeError::Unavailable),
        ] {
            let (client, server) = fixture(
                response(status, "application/json", "private upstream diagnostics"),
                "GET",
                "/readyz".into(),
                None,
            )
            .await;
            assert_eq!(client.ready().await, Err(error));
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn reservations_require_exact_schema_and_capability_shape() {
        let id = "b".repeat(32);
        let capability = "c".repeat(64);
        let good = serde_json::json!({"id":id,"capability":capability,"state":"receiving"});
        let mut cases = vec![(good.clone(), true)];
        for (key, value) in [
            ("id", serde_json::json!("../readyz")),
            ("capability", serde_json::json!("C".repeat(64))),
            ("state", serde_json::json!("queued")),
            ("unexpected", serde_json::json!(true)),
        ] {
            let mut invalid = good.clone();
            invalid[key] = value;
            cases.push((invalid, false));
        }
        for (body, valid) in cases {
            let (client, server) = fixture(
                response("201 Created", "application/json", &body.to_string()),
                "POST",
                "/v1/reservations".into(),
                None,
            )
            .await;
            let result = client.reserve("sample.png").await;
            if valid {
                let reservation = result.unwrap();
                assert_eq!(reservation.id, id);
                assert_eq!(reservation.capability, capability);
            } else {
                assert!(matches!(result, Err(IntakeError::Unavailable)));
            }
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn upload_and_status_validate_receipts() {
        let id = "b".repeat(32);
        let capability = "c".repeat(64);
        let good = serde_json::json!({"id":id,"state":"queued","input_bytes":1,"output_id":null});
        let mut cases = vec![(good.clone(), true)];
        for (key, value) in [
            ("id", serde_json::json!("d".repeat(32))),
            ("state", serde_json::json!("published")),
            ("input_bytes", serde_json::json!(0)),
            ("input_bytes", serde_json::json!(8_388_609)),
            ("input_bytes", serde_json::Value::Null),
            ("output_id", serde_json::json!("d".repeat(32))),
            ("unexpected", serde_json::json!(true)),
        ] {
            let mut invalid = good.clone();
            invalid[key] = value;
            cases.push((invalid, false));
        }
        for (body, valid) in cases {
            let (client, server) = fixture(
                response("200 OK", "application/json", &body.to_string()),
                "PUT",
                format!("/v1/uploads/{id}"),
                Some(capability.clone()),
            )
            .await;
            let result = client.upload(&id, &capability, Body::from("x")).await;
            assert_eq!(
                result,
                if valid {
                    Ok(())
                } else {
                    Err(IntakeError::Unavailable)
                }
            );
            server.await.unwrap();
        }
        for (state, output, valid) in [
            ("receiving", None, true),
            ("uploading", None, true),
            ("queued", None, true),
            ("processing", None, true),
            ("published", Some("d".repeat(32)), true),
            ("failed", None, true),
            ("unknown", None, false),
            ("published", Some("invalid".into()), false),
        ] {
            let body =
                serde_json::json!({"id":id,"state":state,"input_bytes":1,"output_id":output});
            let (client, server) = fixture(
                response("200 OK", "application/json", &body.to_string()),
                "GET",
                format!("/v1/uploads/{id}"),
                Some(capability.clone()),
            )
            .await;
            let result = client.status(&id, &capability).await;
            if valid {
                assert_eq!(result.unwrap().state, state);
            } else {
                assert!(matches!(result, Err(IntakeError::Unavailable)));
            }
            server.await.unwrap();
        }
    }

    #[tokio::test]
    async fn paired_reservation_uses_only_the_explicit_v2_endpoint() {
        let id = "1".repeat(32);
        let capability = "2".repeat(64);
        let body = serde_json::json!({"id":id,"capability":capability,"state":"receiving"});
        let (client, server) = fixture(
            response("201 Created", "application/json", &body.to_string()),
            "POST",
            "/v2/reservations".into(),
            None,
        )
        .await;
        let reservation = client.reserve_pair("tegaki.png").await.unwrap();
        assert_eq!(reservation.id, id);
        assert_eq!(reservation.capability, capability);
        server.await.unwrap();
    }

    #[tokio::test]
    async fn paired_upload_acknowledgement_has_its_own_exact_aggregate_bound() {
        let id = "1".repeat(32);
        let capability = "2".repeat(64);
        for (bytes, valid) in [
            (56, false),
            (57, true),
            (8_388_664, true),
            (16_777_272, true),
            (16_777_273, false),
        ] {
            let body = serde_json::json!({"id":id,"state":"queued","input_bytes":bytes});
            let (client, server) = fixture(
                response("202 Accepted", "application/json", &body.to_string()),
                "PUT",
                format!("/v2/uploads/{id}"),
                Some(capability.clone()),
            )
            .await;
            assert_eq!(
                client
                    .upload_pair(&id, &capability, Body::from("candidate"))
                    .await
                    .is_ok(),
                valid
            );
            server.await.unwrap();
        }
    }
}
