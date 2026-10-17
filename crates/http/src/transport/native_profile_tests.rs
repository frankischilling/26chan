use super::*;
use crate::request_facts::native_profile::{self, Context, Mode};
use board_domain::content_admission::network::ProviderValue::{Available as A, Unavailable as U};

#[tokio::test]
async fn native_score_uses_actual_parser_facts_and_server_owned_context() {
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../domain/tests/fixtures/threat-score.json"
    ))
    .unwrap();
    for enabled in [false, true] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().route(
            "/{mode}",
            get(|request: axum::extract::Request| async move {
                let reporting = request.uri().path() == "/report";
                let timing = request.uri().path().starts_with("/time");
                let country = if request.uri().path() == "/post-country" {
                    b"JP"
                } else {
                    b"US"
                };
                let request_time = if request.uri().path() == "/time-control" {
                    2000
                } else {
                    1000
                };
                let captured =
                    crate::request_facts::capture(request.headers(), request.extensions().get());
                let borrowed = match &captured {
                    A(value) => A(value),
                    U => U,
                };
                let origin = board_config::Origin::parse("https://board.example").unwrap();
                let mode = if reporting {
                    Mode::Reporting
                } else {
                    Mode::Posting {
                        is_op: false,
                        country: A(country),
                    }
                };
                match native_profile::score(
                    borrowed,
                    &origin,
                    Context {
                        request_time: A(request_time),
                        challenge: timing.then_some(b"owned"),
                        response: timing.then_some(b"answer"),
                        mode,
                    },
                ) {
                    A(value) => value.value().to_string(),
                    U => "unavailable".into(),
                }
            }),
        );
        let budget = ConnectionBudget::new(board_config::PublicRequestLimits::default());
        let budget = if enabled {
            budget.with_request_header_order()
        } else {
            budget
        };
        let (stop, stopped) = watch::channel(false);
        let server = tokio::spawn(HttpListener::Tcp(listener).serve(app, stopped, budget));
        for (name, mode, control) in [
            ("country-US-0", "post", Some("post-country")),
            ("navigate-1-0", "report", None),
            ("time-1010", "time", Some("time-control")),
        ] {
            let case = fixture["cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|case| case["name"] == name)
                .unwrap();
            let mut fields = String::new();
            for entry in case["server"].as_array().unwrap() {
                let name = entry[0]
                    .as_str()
                    .unwrap()
                    .strip_prefix("HTTP_")
                    .unwrap()
                    .replace('_', "-");
                let value = if name == "REFERER" {
                    "https://board.example/g/"
                } else if name == "COOKIE" && mode == "time" {
                    // Supply the fixture's parsed timing cookie on the actual
                    // wire too; both raw cookies satisfy the same presence rule.
                    "_tcs=x.UTC.999.33"
                } else {
                    entry[1].as_str().unwrap()
                };
                fields.push_str(&format!("{name}: {value}\r\n"));
            }
            // Header names cannot supply the profile's trusted time or geography.
            fields.push_str("X-Geo-Country: JP\r\nX-Request-Time: 2000\r\n");
            for (extra, valid) in [
                ("", true),
                ("User-Agent: duplicate\r\n", false),
                ("Cookie: _tcs=x%2eUTC\r\n", false),
            ] {
                let mut socket = TcpStream::connect(address).await.unwrap();
                socket.write_all(format!("GET /{mode} HTTP/1.1\r\nHost: localhost\r\n{fields}{extra}Connection: close\r\n\r\n").as_bytes()).await.unwrap();
                let mut response = String::new();
                tokio::time::timeout(OUTER, socket.read_to_string(&mut response))
                    .await
                    .unwrap()
                    .unwrap();
                assert!(response.starts_with("HTTP/1.1 200 OK"));
                let body = response.split_once("\r\n\r\n").unwrap().1;
                if enabled && valid {
                    assert_eq!(
                        body.parse::<f64>().unwrap(),
                        case["score"].as_f64().unwrap(),
                        "{name}"
                    );
                } else {
                    assert_eq!(body, "unavailable", "{name}");
                }
            }
            if enabled && let Some(control) = control {
                let mut socket = TcpStream::connect(address).await.unwrap();
                socket.write_all(format!("GET /{control} HTTP/1.1\r\nHost: localhost\r\n{fields}Connection: close\r\n\r\n").as_bytes()).await.unwrap();
                let mut response = String::new();
                tokio::time::timeout(OUTER, socket.read_to_string(&mut response))
                    .await
                    .unwrap()
                    .unwrap();
                let value = response
                    .split_once("\r\n\r\n")
                    .unwrap()
                    .1
                    .parse::<f64>()
                    .unwrap();
                assert_ne!(
                    value,
                    case["score"].as_f64().unwrap(),
                    "{name}: changing actual trusted context must affect this vector"
                );
            }
        }
        stop.send(true).unwrap();
        finish(server).await;
    }
}
