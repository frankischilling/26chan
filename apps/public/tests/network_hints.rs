#![cfg(feature = "database-tests")]
#[path = "support/posting.rs"]
mod posting;

use board_domain::content_admission::network::hints::BrowserKey;
use board_http::transport::{ConnectionBudget, HttpListener};
use sqlx::PgPool;
use std::{net::SocketAddr, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
    sync::watch,
};

// Independent selected-source vectors in fixtures/network-hints-reference.json.
const BROWSER: &str = "030393b9b";
const REQUEST: &str = "6dd64fdb";

async fn counts(owner: &PgPool, board: &str) -> (i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM content.posts WHERE board=$1),(SELECT count(*) FROM content.threads WHERE board=$1)")
        .bind(board).fetch_one(owner).await.unwrap()
}

async fn request(
    address: SocketAddr,
    board: &str,
    route: &str,
    ua: &str,
    reversed: bool,
    duplicate: bool,
) -> (u16, serde_json::Value) {
    let origin = format!("http://{address}");
    let body =
        "mode=regist&resto=0&name=Anonymous&sub=&com=Owned+request+hint+matrix&pwd=owned-password";
    let fetch = if reversed {
        "Sec-Fetch-Dest: empty\r\nSec-Fetch-Mode: cors\r\nSec-Fetch-Site: same-origin\r\n"
    } else {
        "Sec-Fetch-Site: same-origin\r\nSec-Fetch-Mode: cors\r\nSec-Fetch-Dest: empty\r\n"
    };
    let extra = if duplicate {
        "User_Agent: Different/1\r\n"
    } else {
        ""
    };
    let wire = format!(
        "POST /{board}/{route} HTTP/1.1\r\nHost: {address}\r\nOrigin: {origin}\r\nUser-Agent: {ua}\r\nAccept: application/json\r\nReferer: {origin}/{board}/\r\nAccept-Language: en\r\n{fetch}{extra}X-Board-Header-Order: sec-fetch-site,sec-fetch-mode,sec-fetch-dest\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut stream = TcpStream::connect(address).await.unwrap();
    stream.write_all(wire.as_bytes()).await.unwrap();
    let mut bytes = Vec::new();
    tokio::time::timeout(
        Duration::from_secs(15),
        (&mut stream).take(262145).read_to_end(&mut bytes),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(bytes.len() < 262144);
    let response = String::from_utf8(bytes).unwrap();
    let (head, body) = response
        .split_once("\r\n\r\n")
        .expect("Owned complete HTTP response");
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    let json = serde_json::from_str(body).expect("Owned JSON response body");
    (status, json)
}

async fn exercise(owner: PgPool, public: PgPool, board: String) {
    let rule: i64=sqlx::query_scalar("INSERT INTO admission.network_rules(boards,range_start,range_end,signatures) VALUES($1,2130706433,2130706433,$2) RETURNING id")
        .bind(&board).bind(BROWSER).fetch_one(&owner).await.unwrap();
    for (capture, key_present) in [(true, true), (true, false), (false, true)] {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let options = board_public::PublicRouterOptions {
            origin: format!("http://{address}"),
            production: false,
            media: None,
            limits: board_config::PublicRequestLimits::default(),
            proxy_uid: None,
            poster_id_key: Some(posting::key(&board)),
            tripcode_key: None,
            country_database: None,
        };
        let browser_key =
            key_present.then(|| Arc::new(BrowserKey::parse(&"11".repeat(32)).unwrap()));
        let app =
            board_public::routers_with_network(public.clone(), options, None, None, browser_key).0;
        let budget = ConnectionBudget::new(board_config::PublicRequestLimits::default());
        let budget = if capture {
            budget.with_request_header_order()
        } else {
            budget
        };
        let (stop, stopped) = watch::channel(false);
        let mut serving = tokio::spawn(HttpListener::Tcp(listener).serve(app, stopped, budget));
        // expected: 0 accepted, 1 a definite source match, 2 unavailable provider.
        let cases: Vec<(&str, &str, &str, bool, bool, u8)> = if capture && key_present {
            vec![
                (BROWSER, "post", "Firefox/123.4", false, false, 1),
                (BROWSER, "imgboard.php", "Different/1", false, false, 0),
                (REQUEST, "post", "Firefox/123.4", false, false, 1),
                (REQUEST, "imgboard.php", "Firefox/123.4", false, false, 1),
                (REQUEST, "post", "Firefox/123.4", true, false, 0),
                (REQUEST, "imgboard.php", "Firefox/123.4", false, true, 1),
                (BROWSER, "post", "Firefox/123.4", false, true, 2),
            ]
        } else if capture {
            vec![
                (REQUEST, "post", "Firefox/123.4", false, false, 1),
                (BROWSER, "imgboard.php", "Firefox/123.4", false, false, 2),
            ]
        } else {
            vec![
                (BROWSER, "post", "Firefox/123.4", false, false, 1),
                (REQUEST, "imgboard.php", "Firefox/123.4", false, false, 2),
            ]
        };
        let task_owner = owner.clone();
        let task_board = board.clone();
        let result = tokio::spawn(async move {
            for (signature, route, ua, reversed, duplicate, expected) in cases {
                sqlx::query("UPDATE admission.network_rules SET signatures=$2 WHERE id=$1")
                    .bind(rule)
                    .bind(signature)
                    .execute(&task_owner)
                    .await
                    .unwrap();
                let before = counts(&task_owner, &task_board).await;
                let (status, json) =
                    request(address, &task_board, route, ua, reversed, duplicate).await;
                if expected == 0 {
                    assert_eq!(status, 200);
                    assert!(
                        json.get("error").is_none(),
                        "Owned posting response unexpectedly rejected"
                    );
                    assert!(json["pid"].as_i64().unwrap() > 0);
                    assert_eq!(
                        counts(&task_owner, &task_board).await,
                        (before.0 + 1, before.1 + 1)
                    );
                } else {
                    assert_eq!(status, if expected == 1 { 200 } else { 503 });
                    let message = json["error"].as_str().unwrap();
                    if expected == 1 {
                        assert!(message.starts_with("Posting from your IP range"));
                    } else {
                        assert!(message.contains("unavailable"));
                    }
                    assert!(!message.contains(signature));
                    assert_eq!(counts(&task_owner, &task_board).await, before);
                }
            }
        })
        .await;
        stop.send(true).unwrap();
        match tokio::time::timeout(Duration::from_secs(5), &mut serving).await {
            Ok(result) => result.unwrap().unwrap(),
            Err(_) => {
                serving.abort();
                let _ = serving.await;
                panic!("Owned hint listener did not stop");
            }
        }
        result.unwrap();
    }
}

#[tokio::test]
async fn parser_owned_network_hints_drive_both_posting_routes_without_header_claims() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT 'h'||substr(replace(gen_random_uuid()::text,'-',''),1,9)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,captcha_enabled,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,user_thread_limit) VALUES($1,'Owned network hints','Synthetic',false,1000,100,100,100,10,0,0,0,100)").bind(&board).execute(&owner).await.unwrap();
    let result = tokio::spawn(exercise(owner.clone(), public.clone(), board.clone())).await;
    public.close().await;
    sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)").bind(&board).execute(&owner).await.unwrap();
    for query in [
        "DELETE FROM admission.network_rules WHERE boards=$1",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(query)
            .bind(&board)
            .execute(&owner)
            .await
            .unwrap();
    }
    owner.close().await;
    result.unwrap();
}

async fn report_request(address: SocketAddr, path: &str, body: Option<&str>) -> (u16, String) {
    let method = if body.is_some() { "POST" } else { "GET" };
    let body = body.unwrap_or("");
    let wire = format!(
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nOrigin: http://{address}\r\nUser-Agent: Firefox/123.4\r\nAccept: text/html\r\nReferer: http://{address}/\r\nAccept-Language: en\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let mut stream = TcpStream::connect(address).await.unwrap();
    stream.write_all(wire.as_bytes()).await.unwrap();
    let mut response = Vec::new();
    tokio::time::timeout(
        Duration::from_secs(15),
        (&mut stream).take(1_048_577).read_to_end(&mut response),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(response.len() < 1_048_576);
    let response = String::from_utf8(response).unwrap();
    let (head, body) = response.split_once("\r\n\r\n").unwrap();
    (
        head.split_whitespace().nth(1).unwrap().parse().unwrap(),
        body.into(),
    )
}

#[tokio::test]
async fn report_routes_keep_get_advice_and_apply_parser_hints_before_commit() {
    assert!(std::env::var("BOARD_TEST_CLUSTER").is_ok_and(|path| {
        path.strip_prefix("/tmp/board-postgres.")
            .is_some_and(|tag| tag.len() == 8 && tag.bytes().all(|b| b.is_ascii_alphanumeric()))
    }));
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT 'rh'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,captcha_enabled,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) VALUES($1,'Owned report hints','Synthetic',false,1000,100,100,100,10,0,0,0)").bind(&board).execute(&owner).await.unwrap();
    let post = posting::create_post(
        &public,
        &board,
        0,
        &board_store::NewPost {
            name: "Anonymous".into(),
            subject: "Owned report target".into(),
            comment: "Synthetic report target".into(),
            deletion_hash: "owned-report".into(),
            sage: false,
        },
    )
    .await
    .unwrap();
    let rule:i64=sqlx::query_scalar("INSERT INTO admission.network_rules(boards,range_start,range_end,reports_only,signatures) VALUES($1,2130706433,2130706433,true,$2) RETURNING id").bind(&board).bind(BROWSER).fetch_one(&owner).await.unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = board_public::routers_with_network(
        public.clone(),
        board_public::PublicRouterOptions {
            origin: format!("http://{address}"),
            production: false,
            media: None,
            limits: board_config::PublicRequestLimits::default(),
            proxy_uid: None,
            poster_id_key: Some(posting::key(&board)),
            tripcode_key: None,
            country_database: None,
        },
        None,
        None,
        Some(Arc::new(BrowserKey::parse(&"11".repeat(32)).unwrap())),
    )
    .0;
    let (stop, stopped) = watch::channel(false);
    let mut serving = tokio::spawn(
        HttpListener::Tcp(listener).serve(
            app,
            stopped,
            ConnectionBudget::new(board_config::PublicRequestLimits::default())
                .with_request_header_order(),
        ),
    );
    let task_owner = owner.clone();
    let task_board = board.clone();
    let result = tokio::spawn(async move {
        let path = format!("/{task_board}/imgboard.php?mode=report&no={post}");
        let (status, html) = report_request(address, &path, None).await;
        assert_eq!(status, 200);
        assert!(!html.contains("Reporting from this IP range"));
        let direct_body=format!("no={post}&reason=Owned+report");
        let (status,html)=report_request(address,&format!("/{task_board}/report"),Some(&direct_body)).await;
        assert_eq!(status,422,"{html}");
        assert!(html.contains("Reporting from this IP range has been blocked due to abuse."));
        let catalog=serde_json::json!({"version":1,"categories":[{"id":7,"board":"","op_only":false,"reply_only":false,"image_only":false,"exclude_boards":null,"title":"Owned report category","weight":1.25,"filtered":0}]});
        let revision:i64=sqlx::query_scalar("SELECT content.import_report_catalog($1::text::jsonb)").bind(catalog.to_string()).fetch_one(&task_owner).await.unwrap();
        sqlx::query("SELECT content.set_report_catalog_active($1)").bind(revision).execute(&task_owner).await.unwrap();
        let body=format!("cat_id=7&revision={revision}");
        let (status,html)=report_request(address,&path,None).await;
        assert_eq!(status,200);assert!(!html.contains("Reporting from this IP range"));
        let (status,html)=report_request(address,&path,Some(&body)).await;
        assert_eq!(status,422,"{html}");
        assert!(html.contains("Reporting from this IP range has been blocked due to abuse."));
        assert!(!html.contains(BROWSER));
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM content.reports WHERE board=$1").bind(&task_board).fetch_one(&task_owner).await.unwrap(),0);
        sqlx::query("UPDATE admission.network_rules SET active=false WHERE id=$1").bind(rule).execute(&task_owner).await.unwrap();
        let (status,html)=report_request(address,&path,Some(&body)).await;
        assert_eq!(status,200,"{html}");
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM content.reports WHERE board=$1")
                .bind(&task_board)
                .fetch_one(&task_owner)
                .await
                .unwrap(),
            1
        );
    })
    .await;
    stop.send(true).unwrap();
    if tokio::time::timeout(Duration::from_secs(5), &mut serving)
        .await
        .is_err()
    {
        serving.abort();
        let _ = serving.await;
    }
    sqlx::query("SELECT content.set_report_catalog_active(NULL)")
        .execute(&owner)
        .await
        .unwrap();
    let tokens:Vec<Vec<u8>>=sqlx::query_scalar("SELECT a.token_hash FROM post_secrets.anonymous_reports a JOIN content.reports r ON r.id=a.report_id WHERE r.board=$1").bind(&board).fetch_all(&owner).await.unwrap();
    posting::cleanup_posting(&owner, &board).await;
    for query in [
        "DELETE FROM admission.network_rules WHERE boards=$1",
        "DELETE FROM content.reports WHERE board=$1",
        "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(query)
            .bind(&board)
            .execute(&owner)
            .await
            .unwrap();
    }
    for token in tokens {
        sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
            .bind(token)
            .execute(&owner)
            .await
            .unwrap();
    }
    public.close().await;
    owner.close().await;
    result.unwrap();
}
