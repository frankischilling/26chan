#![cfg(all(unix, feature = "database-tests"))]

use std::{
    io::{Read, Write},
    net::{SocketAddr, TcpListener, TcpStream},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        // Own and reap this child even when an assertion fails.
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
        }
        let _ = self.0.wait();
    }
}

fn connection(address: SocketAddr) -> std::io::Result<TcpStream> {
    let stream = TcpStream::connect_timeout(&address, Duration::from_millis(200))?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    Ok(stream)
}

fn response(address: SocketAddr, request: &str) -> std::io::Result<String> {
    let mut stream = connection(address)?;
    stream.write_all(request.as_bytes())?;
    let mut output = String::new();
    stream.read_to_string(&mut output)?;
    Ok(output)
}

fn eventually(mut check: impl FnMut() -> bool, limit: Duration, reason: &str) {
    let deadline = Instant::now() + limit;
    loop {
        if check() {
            return;
        }
        assert!(Instant::now() < deadline, "{reason}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn drains_after(signal: &str) {
    let app_reservation = TcpListener::bind("127.0.0.1:0").unwrap();
    let metrics_reservation = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = app_reservation.local_addr().unwrap();
    let metrics_address = metrics_reservation.local_addr().unwrap();
    let origin = format!("http://localhost:{}", address.port());
    let token = "a".repeat(64); // Synthetic, per-process loopback test credential.
    let metrics_request = format!(
        "GET /metrics HTTP/1.1\r\nHost: localhost\r\nAuthorization: Bearer {token}\r\nConnection: close\r\n\r\n"
    );
    let mut command = Command::new(env!("CARGO_BIN_EXE_board-staff"));
    command
        .env_clear()
        .env("STAFF_MODE", "development")
        .env("STAFF_ORIGIN", &origin)
        .env("STAFF_BIND", address.to_string())
        .env("PUBLIC_ORIGIN", "http://127.0.0.1:3000")
        .env("MEDIA_ORIGIN", "http://127.0.0.1:3002")
        .env("METRICS_BIND_ADDR", metrics_address.to_string())
        .env("METRICS_TOKEN", token)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    for key in ["AUTH_DATABASE_URL", "STAFF_DATABASE_URL"] {
        command.env(
            key,
            std::env::var(key).expect("dedicated test login required"),
        );
    }
    drop(app_reservation);
    drop(metrics_reservation);
    let mut server = Server(command.spawn().unwrap());
    eventually(
        || {
            assert!(
                server.0.try_wait().unwrap().is_none(),
                "staff startup failed"
            );
            response(
                address,
                "GET /readyz HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n",
            )
            .is_ok_and(|value| value.starts_with("HTTP/1.1 200"))
        },
        Duration::from_secs(10),
        "real staff databases did not become ready",
    );
    assert!(
        response(metrics_address, &metrics_request)
            .unwrap()
            .starts_with("HTTP/1.1 200")
    );

    // An incomplete bounded body keeps the real login handler active without
    // holding database locks or adding a test-only route to the application.
    let mut active = connection(address).unwrap();
    let body = r#"{"username":""}"#;
    let headers = format!(
        "POST /login/start HTTP/1.1\r\nHost: localhost\r\nOrigin: {origin}\r\nSec-Fetch-Site: same-origin\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    active.write_all(headers.as_bytes()).unwrap();
    active.write_all(&body.as_bytes()[..1]).unwrap();
    eventually(
        || {
            response(metrics_address, &metrics_request)
                .unwrap()
                .lines()
                .any(|line| line == "board_http_handlers_inflight{listener=\"staff\"} 1")
        },
        Duration::from_secs(2),
        "partial request did not reach the real handler",
    );
    assert!(
        Command::new("/bin/kill")
            .args([signal, &server.0.id().to_string()])
            .status()
            .unwrap()
            .success()
    );
    eventually(
        || {
            assert!(
                server.0.try_wait().unwrap().is_none(),
                "signal terminated staff before draining its active request"
            );
            connection(address).is_err()
        },
        Duration::from_secs(2),
        "shutdown did not stop application admission",
    );
    assert!(
        response(metrics_address, &metrics_request)
            .unwrap()
            .starts_with("HTTP/1.1 200")
    );
    active.write_all(&body.as_bytes()[1..]).unwrap();
    let mut completed = String::new();
    active.read_to_string(&mut completed).unwrap();
    assert!(
        completed.starts_with("HTTP/1.1 401"),
        "held login request did not complete normally"
    );
    assert!(
        completed.ends_with("Authentication required"),
        "response body was truncated"
    );
    eventually(
        || match server.0.try_wait().unwrap() {
            None => false,
            Some(status) => {
                assert!(status.success(), "staff did not exit cleanly: {status}");
                true
            }
        },
        Duration::from_secs(3),
        "staff did not exit after its active request completed",
    );
    assert!(connection(address).is_err());
    assert!(connection(metrics_address).is_err());
}

#[test]
fn sigterm_drains_active_staff_request_and_closes_both_listeners() {
    drains_after("-TERM");
}

#[test]
fn sigint_drains_active_staff_request_and_closes_both_listeners() {
    drains_after("-INT");
}
