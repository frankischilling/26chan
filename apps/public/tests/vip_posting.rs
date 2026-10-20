#![cfg(feature = "database-tests")]
//! Actual private credential verification and public native/legacy/QR transport.
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use board_domain::{
    challenge_secrets::{ReceiptToken, VerificationKey},
    verification_mail::MailKey,
};
use sqlx::PgPool;
use std::sync::Arc;
use tower::ServiceExt;

#[tokio::test]
async fn vip_credentials_flow_through_public_forms_without_persistence() {
    qualify_public_vip(false).await;
}

#[cfg(feature = "browser-tests")]
#[tokio::test]
async fn verified_browser_forms_and_quick_reply_preserve_private_preferences() {
    qualify_public_vip(true).await;
}

async fn qualify_public_vip(browser: bool) {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let issuer = board_store::verification::VerificationStore::connect(
        &std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap(),
    )
    .await
    .unwrap();
    let material = ReceiptToken::generate().unwrap().encode();
    let key = VerificationKey::parse(&material[3..]).unwrap();
    let identifier = key.vip_identifier("owned-public-vip").unwrap();
    let board = format!("vp{}", &material[3..11]);
    let password = "owned-vip-secret";
    let hash = Argon2::default()
        .hash_password(
            password.as_bytes(),
            &SaltString::encode_b64(b"0123456789abcdef").unwrap(),
        )
        .unwrap()
        .to_string();
    let account: i64 =
        sqlx::query_scalar("SELECT content.vip_manage($1,'Owned Verified',$2,true,true)")
            .bind(identifier.as_slice())
            .bind(hash)
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,text_only,captcha_enabled,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,user_thread_limit,robot9000) VALUES($1,'Owned VIP','Synthetic',2000,100,100,100,10,true,false,0,0,0,100,true)")
        .bind(&board).execute(&owner).await.unwrap();
    sqlx::query("INSERT INTO admission.rules(board,pattern) VALUES($1,'ownedblockedvip')")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    let target: i64 =
        sqlx::query_scalar("INSERT INTO content.threads(board) VALUES($1) RETURNING id")
            .bind(&board)
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','Owned target','Owned seed')").bind(target).bind(&board).execute(&owner).await.unwrap();
    let private = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = private.local_addr().unwrap();
    let private_app = board_verification::router(
        board_verification::AppState::new(
            issuer.clone(),
            key,
            MailKey::parse(&"22".repeat(32)).unwrap(),
            &"11".repeat(32),
        )
        .unwrap(),
    );
    let server = tokio::spawn(async move {
        axum::serve(private, private_app).await.unwrap();
    });
    let web = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", web.local_addr().unwrap());
    let (app, api) = board_public::routers_with_verification(
        public.clone(),
        board_public::PublicRouterOptions {
            origin: origin.clone(),
            production: false,
            media: None,
            limits: board_config::PublicRequestLimits::default(),
            proxy_uid: None,
            poster_id_key: Some(Arc::new(
                board_domain::poster_id::PosterIdKey::parse(&material[3..]).unwrap(),
            )),
            tripcode_key: None,
            country_database: None,
        },
        board_config::PublicVerificationSettings::parse(
            Some(&address.to_string()),
            Some(&"11".repeat(32)),
        )
        .unwrap(),
    );
    let web_app = app.clone();
    let web_server = tokio::spawn(async move {
        axum::serve(
            web,
            web_app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let work = {
        let board = board.clone();
        let public = public.clone();
        async move {
            let mut cookie = String::new();
            let mut posted = Vec::new();
            for (number, multipart) in [false, true, false].into_iter().enumerate() {
                let target = target.to_string();
                let options = format!("capcode_!owned-public-vip!{password}");
                let fields = [
                    ("resto", target.as_str()),
                    ("name", "Untrusted name"),
                    (
                        "com",
                        "Same original comment is allowed only for verified badges",
                    ),
                    ("email", options.as_str()),
                    ("mode", "regist"),
                ];
                let path = if multipart {
                    format!("/{board}/imgboard.php")
                } else {
                    format!("/{board}/post")
                };
                let mut request = Request::post(path)
                    .header("origin", origin.as_str())
                    .header(
                        "accept",
                        if number == 2 {
                            "text/html"
                        } else {
                            "application/json"
                        },
                    );
                if !cookie.is_empty() {
                    request = request.header(header::COOKIE, &cookie);
                }
                let body = if multipart {
                    request =
                        request.header("content-type", "multipart/form-data; boundary=owned-vip");
                    let mut body = String::new();
                    for (name, value) in fields {
                        body.push_str(&format!("--owned-vip\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"));
                    }
                    body.push_str("--owned-vip--\r\n");
                    body
                } else {
                    request = request.header("content-type", "application/x-www-form-urlencoded");
                    url::form_urlencoded::Serializer::new(String::new())
                        .extend_pairs(fields)
                        .finish()
                };
                let mut request = request.body(Body::from(body)).unwrap();
                request.extensions_mut().insert(axum::extract::ConnectInfo(
                    "192.0.2.145:12345".parse::<std::net::SocketAddr>().unwrap(),
                ));
                let response = app.clone().oneshot(request).await.unwrap();
                let (parts, body) = response.into_parts();
                let bytes = to_bytes(body, 65536).await.unwrap();
                assert_eq!(
                    parts.status,
                    if number == 2 {
                        StatusCode::SEE_OTHER
                    } else {
                        StatusCode::OK
                    },
                    "case {number}: {}",
                    String::from_utf8_lossy(&bytes)
                );
                for value in parts.headers.get_all(header::SET_COOKIE) {
                    let value = value.to_str().unwrap();
                    assert!(!value.contains(password) && !value.contains("owned-public-vip"));
                    if value.starts_with("board-anon=") {
                        cookie = value.split(';').next().unwrap().into();
                    }
                }
                assert!(!cookie.is_empty());
                let id = if number == 2 {
                    let location = parts.headers[header::LOCATION].to_str().unwrap();
                    assert!(location.starts_with(&format!("/{board}/thread/{target}#p")));
                    location
                        .rsplit_once("#p")
                        .unwrap()
                        .1
                        .parse::<i64>()
                        .unwrap()
                } else {
                    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                    json["pid"].as_i64().unwrap()
                };
                posted.push(id);
                let row: (String, Option<String>, bool) = sqlx::query_as(
                    "SELECT name,capcode,staff_authorized_limits FROM content.posts WHERE id=$1",
                )
                .bind(id)
                .fetch_one(&public)
                .await
                .unwrap();
                assert_eq!(
                    row,
                    ("Owned Verified".into(), Some("verified".into()), false)
                );
            }
            let rejected_body = url::form_urlencoded::Serializer::new(String::new())
                .append_pair("resto", &target.to_string())
                .append_pair("com", "ownedblockedvip")
                .append_pair("email", &format!("capcode_!owned-public-vip!{password}"))
                .finish();
            let rejected = app
                .clone()
                .oneshot(
                    Request::post(format!("/{board}/post"))
                        .header("origin", origin.as_str())
                        .header("accept", "application/json")
                        .header("content-type", "application/x-www-form-urlencoded")
                        .header(
                            header::COOKIE,
                            format!("{cookie}; options=capcode_!owned-public-vip!{password}"),
                        )
                        .extension(axum::extract::ConnectInfo(
                            "192.0.2.145:12345".parse::<std::net::SocketAddr>().unwrap(),
                        ))
                        .body(Body::from(rejected_body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(rejected.status(), StatusCode::OK);
            assert!(
                rejected
                    .headers()
                    .get_all(header::SET_COOKIE)
                    .iter()
                    .any(|value| value
                        .to_str()
                        .unwrap()
                        .starts_with("options=; Path=/; Max-Age=0;"))
            );
            let bytes = to_bytes(rejected.into_body(), 65536).await.unwrap();
            let value: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert!(value["error"].as_str().unwrap().contains("spam"));
            assert!(!String::from_utf8_lossy(&bytes).contains(password));
            for (router, path) in [
                (&app, format!("/{board}/thread/{target}")),
                (&api, format!("/{board}/thread/{target}.json")),
            ] {
                let response = router
                    .clone()
                    .oneshot(Request::get(path).body(Body::empty()).unwrap())
                    .await
                    .unwrap();
                assert_eq!(response.status(), StatusCode::OK);
                let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
                let body = String::from_utf8(bytes.to_vec()).unwrap();
                assert!(body.contains("Owned Verified"));
                assert!(body.contains("verified") || body.contains("Verified"));
                assert!(!body.contains(password) && !body.contains("owned-public-vip"));
            }
            if browser {
                #[cfg(feature = "browser-tests")]
                {
                    let ids = qualify_browser(&origin, &board, target, password).await;
                    assert_eq!(ids.len(), 3);
                    for id in ids {
                        let row: (String, Option<String>, bool) = sqlx::query_as("SELECT name,capcode,staff_authorized_limits FROM content.posts WHERE board=$1 AND id=$2")
                            .bind(&board).bind(id).fetch_one(&public).await.unwrap();
                        assert_eq!(
                            row,
                            ("Owned Verified".into(), Some("verified".into()), false)
                        );
                        posted.push(id);
                    }
                }
                #[cfg(not(feature = "browser-tests"))]
                panic!("Browser qualification requires its explicit feature");
            }
            (posted, cookie)
        }
    };
    let result = tokio::spawn(work).await;
    server.abort();
    web_server.abort();
    let _ = server.await;
    let _ = web_server.await;
    // Retire only the synthetic fixture, also when a route assertion fails.
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(&board)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM content.threads WHERE board=$1 ORDER BY id FOR UPDATE")
        .bind(&board)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash IN (SELECT a.token_hash FROM post_secrets.anonymous_posts a JOIN content.posts p ON p.id=a.post_id WHERE p.board=$1)")
        .bind(&board).execute(&mut *tx).await.unwrap();
    sqlx::query("DELETE FROM admission.hits WHERE rule_id IN(SELECT id FROM admission.rules WHERE board=$1)").bind(&board).execute(&mut *tx).await.unwrap();
    sqlx::query("DELETE FROM admission.rules WHERE board=$1")
        .bind(&board)
        .execute(&mut *tx)
        .await
        .unwrap();
    for statement in [
        "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM post_secrets.posting_thread_actions WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(statement)
            .bind(&board)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.vip_accounts WHERE id=$1")
        .bind(account)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    public.close().await;
    issuer.close().await;
    owner.close().await;
    result.unwrap();
}

#[cfg(feature = "browser-tests")]
async fn qualify_browser(origin: &str, board: &str, target: i64, password: &str) -> Vec<i64> {
    use std::{process::Stdio, time::Duration};
    use tokio::io::AsyncWriteExt;
    let mut command = tokio::process::Command::new("node");
    command.env_clear();
    for name in [
        "PATH",
        "HOME",
        "SystemRoot",
        "WINDIR",
        "TEMP",
        "TMP",
        "USERPROFILE",
        "LOCALAPPDATA",
        "APPDATA",
        "PLAYWRIGHT_BROWSERS_PATH",
        "LANG",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    let mut child = command
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .arg("tests/browser/vip-posting.mjs")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    input.write_all(serde_json::to_string(&serde_json::json!({"origin":origin,"board":board,"target":target.to_string(),"credential":format!("capcode_!owned-public-vip!{password}")})).unwrap().as_bytes()).await.unwrap();
    input.shutdown().await.unwrap();
    drop(input);
    let output = tokio::time::timeout(Duration::from_secs(90), child.wait_with_output())
        .await
        .expect("Owned VIP browser exceeded its deadline")
        .unwrap();
    assert!(
        output.status.success(),
        "Owned VIP browser failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}
