//! Bounded delivery to an explicitly configured local SMTP relay or fixture.
use board_domain::verification_mail::MailPayload;
use std::{net::SocketAddr, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpStream,
};

async fn reply(stream: &mut BufReader<TcpStream>, expected: &[u8; 3]) -> Result<(), ()> {
    for _ in 0..32 {
        let mut line = Vec::with_capacity(128);
        loop {
            if line.len() >= 1024 {
                return Err(());
            }
            let byte = stream.read_u8().await.map_err(|_| ())?;
            line.push(byte);
            if byte == b'\n' {
                break;
            }
        }
        if line.len() < 6 || &line[..3] != expected || !line.ends_with(b"\r\n") {
            return Err(());
        }
        match line[3] {
            b' ' => return Ok(()),
            b'-' => {}
            _ => return Err(()),
        }
    }
    Err(())
}
async fn command(
    stream: &mut BufReader<TcpStream>,
    value: &str,
    expected: &[u8; 3],
) -> Result<(), ()> {
    stream
        .get_mut()
        .write_all(value.as_bytes())
        .await
        .map_err(|_| ())?;
    reply(stream, expected).await
}
fn request_id(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(&b)
            }
        })
}
pub(crate) async fn deliver(
    address: SocketAddr,
    origin: &str,
    id: &str,
    payload: &MailPayload,
) -> Result<(), ()> {
    if !address.ip().is_loopback() || address.port() == 0 || !request_id(id) {
        return Err(());
    }
    // Origin was parsed as an exact origin in configuration. It is checked
    // here too so callers cannot inject message headers through this API.
    let origin = url::Url::parse(origin).map_err(|_| ())?;
    if origin.origin().ascii_serialization() != origin.as_str().trim_end_matches('/')
        || !matches!(origin.scheme(), "http" | "https")
        || origin.query().is_some()
        || origin.fragment().is_some()
        || !origin.username().is_empty()
        || origin.password().is_some()
    {
        return Err(());
    }
    let domain = origin.host_str().ok_or(())?;
    let message = format!(
        "From: Paperboard <noreply@{domain}>\r\nTo: {}\r\nSubject: Email Verification\r\nMessage-ID: <verification-{id}@{domain}>\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=UTF-8\r\n\r\nOpen this link to verify your browser session:\r\n{}signin?action=verify&tkn={}\r\n\r\nThis link expires in 24 hours and can be used once. If you did not request it, ignore this email.\r\n.\r\n",
        payload.recipient(),
        origin.as_str(),
        payload.token()
    );
    if message.len() > 2048 {
        return Err(());
    }
    tokio::time::timeout(Duration::from_secs(5), async {
        let mut stream = BufReader::new(TcpStream::connect(address).await.map_err(|_| ())?);
        reply(&mut stream, b"220").await?;
        command(&mut stream, "EHLO verification.local\r\n", b"250").await?;
        command(
            &mut stream,
            &format!("MAIL FROM:<noreply@{domain}>\r\n"),
            b"250",
        )
        .await?;
        command(
            &mut stream,
            &format!("RCPT TO:<{}>\r\n", payload.recipient()),
            b"250",
        )
        .await?;
        command(&mut stream, "DATA\r\n", b"354").await?;
        command(&mut stream, &message, b"250").await?;
        // The final 250 acknowledges delivery. A lost QUIT reply must not turn
        // acknowledged mail into a false failure or trigger a second delivery.
        let _ = stream.get_mut().try_write(b"QUIT\r\n");
        Ok(())
    })
    .await
    .map_err(|_| ())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{io::AsyncBufReadExt, net::TcpListener};
    #[tokio::test]
    async fn fixture_delivery_has_stable_message_id_and_requires_final_ack() {
        for accepted in [true, false] {
            let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
            let addr = listener.local_addr().unwrap();
            let server = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut stream = BufReader::new(stream);
                stream
                    .get_mut()
                    .write_all(b"220 local fixture\r\n")
                    .await
                    .unwrap();
                let mut received = String::new();
                for (prefix, response) in [
                    ("EHLO ", "250-local\r\n250 OK\r\n"),
                    ("MAIL FROM:", "250 OK\r\n"),
                    ("RCPT TO:", "250 OK\r\n"),
                    ("DATA", "354 Send\r\n"),
                ] {
                    let mut line = String::new();
                    stream.read_line(&mut line).await.unwrap();
                    assert!(line.starts_with(prefix));
                    received.push_str(&line);
                    stream
                        .get_mut()
                        .write_all(response.as_bytes())
                        .await
                        .unwrap();
                }
                loop {
                    let mut line = String::new();
                    stream.read_line(&mut line).await.unwrap();
                    received.push_str(&line);
                    if line == ".\r\n" {
                        break;
                    }
                }
                stream
                    .get_mut()
                    .write_all(if accepted {
                        b"250 Accepted\r\n"
                    } else {
                        b"451 Retry later\r\n"
                    })
                    .await
                    .unwrap();
                received
            });
            let token = format!("e1_{}", "ab".repeat(32));
            let payload = MailPayload::new("owned.fixture@gmail.com", &token).unwrap();
            let result = deliver(
                addr,
                "http://127.0.0.1:3000",
                "12345678-1234-1234-1234-123456789abc",
                &payload,
            )
            .await;
            assert_eq!(result.is_ok(), accepted);
            let received = server.await.unwrap();
            assert!(received.contains(&format!(
                "http://127.0.0.1:3000/signin?action=verify&tkn={token}"
            )));
            assert!(received.contains(
                "Message-ID: <verification-12345678-1234-1234-1234-123456789abc@127.0.0.1>"
            ));
        }
    }
}
