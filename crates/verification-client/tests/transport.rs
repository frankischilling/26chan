use board_verification_client::{
    Error, VerificationClient,
    protocol::{EmailState, EmailToken},
};
use std::time::Duration;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

async fn exchange(
    status: &str,
    content_type: &str,
    body: &str,
) -> Result<board_verification_client::protocol::EmailResult, Error> {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let client = VerificationClient::new(address, "ab".repeat(32)).unwrap();
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\nLocation: http://192.0.2.1/never-follow\r\n\r\n{body}",
        body.len()
    );
    let serving = async {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut bytes = Vec::new();
        loop {
            let mut chunk = [0; 2048];
            let n = stream.read(&mut chunk).await.unwrap();
            assert!(n > 0);
            bytes.extend_from_slice(&chunk[..n]);
            assert!(bytes.len() <= 8192);
            if let Some(end) = bytes.windows(4).position(|v| v == b"\r\n\r\n") {
                let headers = std::str::from_utf8(&bytes[..end]).unwrap();
                let length: usize = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .map(|v| v.trim().parse().unwrap())
                    })
                    .unwrap();
                if bytes.len() >= end + 4 + length {
                    break;
                }
            }
        }
        let request = std::str::from_utf8(&bytes).unwrap();
        assert!(request.starts_with("POST /v1/email/state HTTP/1.1\r\n"));
        assert!(request.contains(&format!("authorization: Bearer {}\r\n", "ab".repeat(32))));
        assert!(request.ends_with(r#"{"token":"synthetic-token"}"#));
        stream.write_all(response.as_bytes()).await.unwrap();
        stream.shutdown().await.unwrap();
        // A rejected response must not cause another write to this endpoint.
        assert!(
            tokio::time::timeout(Duration::from_millis(100), listener.accept())
                .await
                .is_err()
        );
    };
    let input = EmailToken {
        token: "synthetic-token".into(),
    };
    let call = client.email_state(&input);
    let (_, result) = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::join!(serving, call)
    })
    .await
    .unwrap();
    result
}

#[tokio::test]
async fn bounded_json_response_and_fixed_authenticated_path() {
    let result = exchange(
        "200 OK",
        "application/json",
        r#"{"state":"pending","cooldown_seconds":0}"#,
    )
    .await
    .unwrap();
    assert!(matches!(result.state, EmailState::Pending));
    assert_eq!(result.cooldown_seconds, 0);
}

#[tokio::test]
async fn redirects_errors_wrong_types_and_unknown_fields_fail_without_retry() {
    for (status, kind, body) in [
        (
            "302 Found",
            "application/json",
            r#"{"state":"pending","cooldown_seconds":0}"#,
        ),
        (
            "500 Internal Server Error",
            "application/json",
            "private upstream details",
        ),
        (
            "200 OK",
            "text/html",
            "<html>private upstream details</html>",
        ),
        (
            "200 OK",
            "application/json",
            r#"{"state":"pending","cooldown_seconds":0,"secret":"unexpected"}"#,
        ),
    ] {
        assert!(matches!(
            exchange(status, kind, body).await,
            Err(Error::Unavailable)
        ));
    }
    assert!(matches!(
        exchange(
            "400 Bad Request",
            "application/json",
            "private validation details"
        )
        .await,
        Err(Error::Rejected)
    ));
}

#[tokio::test]
async fn oversized_request_is_rejected_before_a_connection() {
    let client = VerificationClient::new("127.0.0.1:1".parse().unwrap(), "ab".repeat(32)).unwrap();
    assert!(matches!(
        client
            .email_state(&EmailToken {
                token: "x".repeat(8192)
            })
            .await,
        Err(Error::Rejected)
    ));
}
