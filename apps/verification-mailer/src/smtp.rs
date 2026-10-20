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
    transmit(address, domain, payload.recipient(), &message).await
}

async fn transmit(
    address: SocketAddr,
    domain: &str,
    recipient: &str,
    message: &str,
) -> Result<(), ()> {
    if !address.ip().is_loopback() || address.port() == 0 {
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
        command(&mut stream, &format!("RCPT TO:<{}>\r\n", recipient), b"250").await?;
        command(&mut stream, "DATA\r\n", b"354").await?;
        command(&mut stream, message, b"250").await?;
        // The final 250 acknowledges delivery. A lost QUIT reply must not turn
        // acknowledged mail into a false failure or trigger a second delivery.
        let _ = stream.get_mut().try_write(b"QUIT\r\n");
        Ok(())
    })
    .await
    .map_err(|_| ())?
}

fn pass_message(
    origin: &str,
    id: &str,
    payload: &board_domain::pass_mail::Payload,
) -> Result<(String, String, String), ()> {
    use board_domain::pass_mail::{Notice, Payload};
    if !request_id(id) {
        return Err(());
    }
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
    if let Payload::Recovery { recipient, token } = payload {
        let mut link = origin.join("pass").map_err(|_| ())?;
        link.query_pairs_mut().append_pair("reset", &token.encode());
        let message = format!(
            "From: Pass <noreply@{domain}>\r\nTo: {}\r\nSubject: Reset your Pass PIN\r\nMessage-ID: <pass-{id}@{domain}>\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=UTF-8\r\n\r\nOpen this link to choose a new Pass PIN: {link}\r\n\r\nThis link expires in 30 minutes and can be used once. If you did not request it, ignore this email.\r\n.\r\n",
            recipient.address()
        );
        if message.len() > 2048 {
            return Err(());
        }
        return Ok((domain.to_owned(), recipient.address().to_owned(), message));
    }
    let Payload::Notice {
        contact,
        kind,
        expires,
    } = payload
    else {
        return Err(());
    };
    if !(0..=253_402_300_799).contains(expires) {
        return Err(());
    }
    let expiry = chrono::DateTime::<chrono::Utc>::from_timestamp(*expires, 0)
        .ok_or(())?
        .format("%m/%d/%y")
        .to_string();
    let mut renewal = origin.join("pass").map_err(|_| ())?;
    renewal
        .query_pairs_mut()
        .append_pair("renew", contact.renewal());
    let (subject, introduction, extension) = match kind {
        Notice::Reminder => (
            "Your 4chan Pass is about to expire",
            format!(
                "Your 4chan Pass (Token: {}) is due to expire in less than 7 days, on {expiry}.",
                contact.identifier()
            ),
            "Renewing your Pass will add 12 additional months to your current expiration date.",
        ),
        Notice::Expired => (
            "Your 4chan Pass has expired!",
            format!(
                "Your 4chan Pass (Token: {}) has expired.",
                contact.identifier()
            ),
            "Renewing your Pass will add 12 additional months from the date of your renewal payment.",
        ),
    };
    let message = format!(
        "From: Pass <noreply@{domain}>\r\nTo: {}\r\nSubject: {subject}\r\nMessage-ID: <pass-{id}@{domain}>\r\nMIME-Version: 1.0\r\nContent-Type: text/plain; charset=UTF-8\r\n\r\n{introduction}\r\n\r\n{extension}\r\n\r\nYou can renew your Pass by visiting the following link: {renewal}\r\n\r\nThanks for your support!\r\n.\r\n",
        contact.recipient()
    );
    if message.len() > 2048 {
        return Err(());
    }
    Ok((domain.to_owned(), contact.recipient().to_owned(), message))
}

pub(crate) async fn deliver_pass(
    address: SocketAddr,
    origin: &str,
    id: &str,
    payload: &board_domain::pass_mail::Payload,
) -> Result<(), ()> {
    let (domain, recipient, message) = pass_message(origin, id, payload)?;
    transmit(address, &domain, &recipient, &message).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::{io::AsyncBufReadExt, net::TcpListener};
    #[test]
    fn pass_notices_use_source_subjects_and_only_configured_renewal_origin() {
        use board_domain::pass_mail::{Contact, Notice, Payload, Recipient};
        let contact = || {
            Contact::new(
                Recipient::parse("owner+pass@example.org").unwrap(),
                "ABCDEFGHIJ",
                "renewal_123",
            )
            .unwrap()
        };
        let id = "12345678-1234-1234-1234-123456789abc";
        for (kind, subject) in [
            (Notice::Reminder, "Your 4chan Pass is about to expire"),
            (Notice::Expired, "Your 4chan Pass has expired!"),
        ] {
            let payload = Payload::notice(contact(), kind, 1_700_000_000).unwrap();
            let (_, recipient, text) = pass_message("https://board.example", id, &payload).unwrap();
            assert_eq!(recipient, "owner+pass@example.org");
            assert!(text.contains(&format!("Subject: {subject}\r\n")));
            assert!(text.contains("Token: ABCDEFGHIJ"));
            assert!(text.contains("https://board.example/pass?renew=renewal_123"));
            assert!(
                text.contains(
                    "Message-ID: <pass-12345678-1234-1234-1234-123456789abc@board.example>"
                )
            );
            for origin in [
                "https://board.example/path",
                "https://user@board.example",
                "https://board.example?x=1",
                "file:///tmp",
            ] {
                assert!(pass_message(origin, id, &payload).is_err());
            }
            assert!(
                pass_message("https://board.example", "bad\r\nHeader: value", &payload).is_err()
            );
        }
        assert!(pass_message("https://board.example", id, &Payload::Contact(contact())).is_err());
    }

    #[test]
    fn recovery_mail_uses_a_bounded_native_link_without_revealing_a_pin() {
        use board_domain::{
            challenge_secrets::PassRecoveryToken,
            pass_mail::{Payload, Recipient},
        };
        let token = PassRecoveryToken::generate().unwrap();
        let encoded = token.encode();
        let payload = Payload::Recovery {
            recipient: Recipient::parse("owned@example.org").unwrap(),
            token,
        };
        let (_, _, message) = pass_message(
            "https://board.example",
            "12345678-1234-1234-1234-123456789abc",
            &payload,
        )
        .unwrap();
        assert!(message.contains(&format!("https://board.example/pass?reset={encoded}")));
        assert!(message.contains("30 minutes and can be used once"));
        assert!(message.len() < 2048);
    }

    #[tokio::test]
    async fn transport_rejects_nonlocal_relay_before_connecting() {
        assert!(
            transmit(
                "192.0.2.1:25".parse().unwrap(),
                "example.org",
                "owner@example.org",
                "test"
            )
            .await
            .is_err()
        );
        assert!(
            transmit(
                "127.0.0.1:0".parse().unwrap(),
                "example.org",
                "owner@example.org",
                "test"
            )
            .await
            .is_err()
        );
    }

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
