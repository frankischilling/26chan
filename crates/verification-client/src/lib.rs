//! Bounded authenticated transport to one numeric loopback verifier endpoint.
//! No URL resolution, redirects, detached tasks, or automatic write retries.
pub mod protocol;
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde::{Serialize, de::DeserializeOwned};
use std::{net::SocketAddr, time::Duration};

#[derive(Clone)]
pub struct VerificationClient {
    endpoint: SocketAddr,
    token: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    #[error("Verification is unavailable. Try again.")]
    Unavailable,
    #[error("The verification request was rejected.")]
    Rejected,
}

pub fn valid_token(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl VerificationClient {
    pub fn new(endpoint: SocketAddr, token: String) -> Result<Self, Error> {
        if endpoint.port() == 0 || !endpoint.ip().is_loopback() || !valid_token(&token) {
            return Err(Error::Unavailable);
        }
        Ok(Self { endpoint, token })
    }

    async fn exchange<I: Serialize, O: DeserializeOwned>(
        &self,
        path: &'static str,
        input: &I,
    ) -> Result<O, Error> {
        let encoded = serde_json::to_vec(input).map_err(|_| Error::Rejected)?;
        if encoded.len() > 8192 {
            return Err(Error::Rejected);
        }
        let request = Request::builder()
            .method("POST")
            .uri(path)
            .header("host", self.endpoint.to_string())
            .header("content-type", "application/json")
            .header("authorization", format!("Bearer {}", self.token))
            .body(Body::from(encoded))
            .map_err(|_| Error::Unavailable)?;
        let exchange = async {
            let socket = tokio::time::timeout(
                Duration::from_secs(2),
                tokio::net::TcpStream::connect(self.endpoint),
            )
            .await
            .map_err(|_| Error::Unavailable)?
            .map_err(|_| Error::Unavailable)?;
            let (mut sender, connection) = hyper::client::conn::http1::Builder::new()
                .max_buf_size(16_384)
                .handshake(hyper_util::rt::TokioIo::new(socket))
                .await
                .map_err(|_| Error::Unavailable)?;
            let response = async {
                let response = sender
                    .send_request(request)
                    .await
                    .map_err(|_| Error::Unavailable)?;
                if response.status() != StatusCode::OK {
                    return Err(if response.status() == StatusCode::BAD_REQUEST {
                        Error::Rejected
                    } else {
                        Error::Unavailable
                    });
                }
                if response
                    .headers()
                    .get("content-type")
                    .is_none_or(|v| v != "application/json")
                {
                    return Err(Error::Unavailable);
                }
                let bytes = to_bytes(Body::new(response.into_body()), 262_144)
                    .await
                    .map_err(|_| Error::Unavailable)?;
                serde_json::from_slice(&bytes).map_err(|_| Error::Unavailable)
            };
            tokio::pin!(connection);
            tokio::pin!(response);
            tokio::select! {biased; result=&mut response=>result, _=&mut connection=>response.await}
        };
        tokio::time::timeout(Duration::from_secs(8), exchange)
            .await
            .map_err(|_| Error::Unavailable)?
    }

    pub async fn ready(&self) -> Result<(), Error> {
        let response: protocol::Readiness =
            self.exchange("/readyz", &serde_json::json!({})).await?;
        if response.ready {
            Ok(())
        } else {
            Err(Error::Unavailable)
        }
    }

    pub async fn ensure_session(
        &self,
        input: &protocol::Session,
    ) -> Result<protocol::SessionResult, Error> {
        self.exchange("/v1/session", input).await
    }
    pub async fn challenge(
        &self,
        input: &protocol::ChallengeContext,
    ) -> Result<protocol::Challenge, Error> {
        self.exchange("/v1/challenge", input).await
    }
    pub async fn posting_challenge(
        &self,
        input: &protocol::PostingChallengeRequest,
    ) -> Result<protocol::PostingChallengeResult, Error> {
        self.exchange("/v1/posting/challenge", input).await
    }
    pub async fn credit(
        &self,
        input: &protocol::ChallengeContext,
    ) -> Result<protocol::AnswerResult, Error> {
        self.exchange("/v1/credit", input).await
    }
    pub async fn check(&self, input: &protocol::Answer) -> Result<protocol::AnswerResult, Error> {
        self.exchange("/v1/check", input).await
    }
    pub async fn request_email(
        &self,
        input: &protocol::EmailRequest,
    ) -> Result<protocol::EmailResult, Error> {
        self.exchange("/v1/email/request", input).await
    }
    pub async fn email_state(
        &self,
        input: &protocol::EmailToken,
    ) -> Result<protocol::EmailResult, Error> {
        self.exchange("/v1/email/state", input).await
    }
    pub async fn verify_email(
        &self,
        input: &protocol::EmailVerification,
    ) -> Result<protocol::EmailResult, Error> {
        self.exchange("/v1/email/verify", input).await
    }
    pub async fn signout(
        &self,
        input: &protocol::Session,
    ) -> Result<protocol::SessionResult, Error> {
        self.exchange("/v1/signout", input).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn endpoint_and_credentials_are_explicit_and_local() {
        for address in ["127.0.0.1:9012", "[::1]:9012"] {
            assert!(VerificationClient::new(address.parse().unwrap(), "ab".repeat(32)).is_ok());
        }
        for address in ["0.0.0.0:9012", "192.0.2.1:9012", "127.0.0.1:0"] {
            assert!(VerificationClient::new(address.parse().unwrap(), "ab".repeat(32)).is_err());
        }
        for token in ["".into(), "AB".repeat(32), "ab".repeat(31), "x".repeat(64)] {
            assert!(VerificationClient::new("127.0.0.1:9012".parse().unwrap(), token).is_err());
        }
    }
    #[test]
    fn response_schemas_reject_unexpected_authority_fields() {
        assert!(
            serde_json::from_str::<protocol::SessionResult>(r#"{"changed":true,"staff":true}"#)
                .is_err()
        );
        assert!(
            serde_json::from_str::<protocol::AnswerResult>(
                r#"{"state":"rejected","receipt":"unexpected"}"#
            )
            .is_err()
        );
        assert!(
            serde_json::from_str::<protocol::EmailResult>(
                r#"{"state":"admin","cooldown_seconds":0}"#
            )
            .is_err()
        );
    }
}
