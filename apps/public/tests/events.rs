#![cfg(feature = "database-tests")]
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{HeaderMap, Request, StatusCode, header},
};
use board_domain::{
    poster_id::PosterIdKey,
    source_events::{STOCKS, emotes},
};
use board_store::events::admin;
use serde_json::{Value, json};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tower::ServiceExt;
const ORIGIN: &str = "http://127.0.0.1:3000";
async fn login(name: &str, role: &str) -> PgPool {
    let pool = PgPoolOptions::new()
        .max_connections(6)
        .connect(&std::env::var(name).unwrap())
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT current_user::text")
            .fetch_one(&pool)
            .await
            .unwrap(),
        role
    );
    pool
}
fn router(pool: PgPool, enabled: bool) -> Router {
    router_at(pool, enabled, ORIGIN)
}
fn router_at(pool: PgPool, enabled: bool, origin: &str) -> Router {
    board_public::routers_with_events(
        pool,
        board_public::PublicRouterOptions {
            origin: origin.into(),
            production: false,
            media: None,
            limits: Default::default(),
            proxy_uid: None,
            poster_id_key: Some(Arc::new(PosterIdKey::parse(&"4".repeat(64)).unwrap())),
            tripcode_key: None,
            country_database: None,
        },
        None,
        Default::default(),
        enabled,
    )
    .0
}
fn request(path: &str, fields: &str, cookie: Option<&str>, multipart: bool) -> Request<Body> {
    let (content_type, body) = if multipart {
        let mut body = String::new();
        for (name, value) in url::form_urlencoded::parse(fields.as_bytes()) {
            body.push_str(&format!("--owned-event\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"));
        }
        body.push_str("--owned-event--\r\n");
        ("multipart/form-data; boundary=owned-event", body)
    } else {
        ("application/x-www-form-urlencoded", fields.to_owned())
    };
    let mut request = Request::post(path)
        .header("origin", ORIGIN)
        .header("content-type", content_type)
        .extension(ConnectInfo(
            "192.0.2.42:1234".parse::<SocketAddr>().unwrap(),
        ))
        .body(Body::from(body))
        .unwrap();
    if let Some(cookie) = cookie {
        request
            .headers_mut()
            .insert(header::COOKIE, cookie.parse().unwrap());
    }
    request
}
async fn response(web: &Router, request: Request<Body>, status: StatusCode) -> (HeaderMap, Value) {
    let response = web.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), status);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    assert_eq!(response.headers()[header::CONTENT_TYPE], "application/json");
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), 32768).await.unwrap();
    (headers, serde_json::from_slice(&body).unwrap())
}
fn credential(headers: &HeaderMap, name: &str) -> String {
    let raw = headers
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| value.to_str().unwrap())
        .find(|value| value.starts_with(&format!("{name}=")))
        .unwrap();
    assert!(raw.contains("; HttpOnly; SameSite=Strict"));
    assert!(raw.contains("; Path=/; Max-Age="));
    assert!(!raw.contains("Domain="));
    raw.split(';').next().unwrap().into()
}

#[tokio::test]
async fn optional_event_http_workflows_use_private_state_and_preserve_closed_defaults() {
    let owner = login("MIGRATION_DATABASE_URL", "board_migrator").await;
    let public = login("TEST_PUBLIC_DATABASE_URL", "board_public").await;
    let mut lock = owner.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(2240251)")
        .execute(&mut *lock)
        .await
        .unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM event_private.runs WHERE enabled")
            .fetch_one(&owner)
            .await
            .unwrap(),
        0
    );
    let ids = Arc::new(Mutex::new(Vec::new()));
    let result = tokio::spawn(check_http(owner.clone(), public.clone(), ids.clone())).await;
    let owned = ids.lock().unwrap().clone();
    sqlx::query("DELETE FROM event_private.runs WHERE id=ANY($1)")
        .bind(&owned)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("SELECT pg_advisory_unlock(2240251)")
        .execute(&mut *lock)
        .await
        .unwrap();
    result.unwrap();
}
async fn check_http(owner: PgPool, public: PgPool, ids: Arc<Mutex<Vec<i64>>>) {
    let now = chrono::Utc::now().timestamp();
    let mut connection = owner.acquire().await.unwrap();
    let prices: std::collections::BTreeMap<_, _> =
        STOCKS.into_iter().map(|stock| (stock, 100)).collect();
    let profile = json!({"version":1,"kind":"market","starts_at":now-3600,"ends_at":now+3600,"account_capacity":3,"prices":prices});
    let market = admin::start(
        &mut connection,
        &admin::read_profile(profile.to_string().as_bytes()).unwrap(),
    )
    .await
    .unwrap();
    ids.lock().unwrap().push(market);
    let emote_profile = json!({"version":1,"kind":"emotes","starts_at":now-3600,"ends_at":now+3600,"account_capacity":3});
    let emote = admin::start(
        &mut connection,
        &admin::read_profile(emote_profile.to_string().as_bytes()).unwrap(),
    )
    .await
    .unwrap();
    ids.lock().unwrap().push(emote);
    let disabled = router(public.clone(), false);
    let web = router(public.clone(), true);
    let (headers, closed) = response(
        &disabled,
        request("/xa24tb.php", "create=1", None, false),
        StatusCode::OK,
    )
    .await;
    assert_eq!(closed, json!({"error":"The market is closed"}));
    assert!(!headers.contains_key(header::SET_COOKIE));
    assert_eq!(
        disabled
            .oneshot(request("/emotes_xa22.php", "action=start", None, false))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM event_private.accounts WHERE run_id=ANY($1)"
        )
        .bind([market, emote].as_slice())
        .fetch_one(&owner)
        .await
        .unwrap(),
        0
    );
    // Native forms work without a client script. GET never creates accounts.
    for path in ["/event-market.html", "/event-emotes.html"] {
        let page = web
            .clone()
            .oneshot(Request::get(path).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(page.status(), StatusCode::OK);
        assert!(!page.headers().contains_key(header::SET_COOKIE));
        assert_eq!(page.headers()[header::CACHE_CONTROL], "no-store");
        let body = to_bytes(page.into_body(), 262144).await.unwrap();
        let html = std::str::from_utf8(&body).unwrap();
        assert!(html.contains("method=\"post\""));
        assert!(!html.contains("<script"));
        if path.contains("emotes") {
            assert!(html.contains("😀"));
            assert!(html.contains("AngryWojak"));
        } else {
            for symbol in STOCKS {
                assert!(html.contains(symbol));
            }
        }
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM event_private.accounts WHERE run_id=ANY($1)"
        )
        .bind([market, emote].as_slice())
        .fetch_one(&owner)
        .await
        .unwrap(),
        0
    );
    let native = web
        .clone()
        .oneshot(request("/event-market.html", "create=1", None, false))
        .await
        .unwrap();
    assert_eq!(native.status(), StatusCode::SEE_OTHER);
    assert_eq!(native.headers()[header::LOCATION], "/event-market.html");
    let native_cookie = credential(native.headers(), "board-event-market");
    let page = web
        .clone()
        .oneshot(
            Request::get("/event-market.html")
                .header(header::COOKIE, &native_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = to_bytes(page.into_body(), 262144).await.unwrap();
    let html = std::str::from_utf8(&body).unwrap();
    assert!(html.contains("Balance: 1000 points"));
    assert!(!html.contains(&native_cookie));
    let (headers, created) = response(
        &web,
        request("/xa24tb.php", "create=1", None, false),
        StatusCode::OK,
    )
    .await;
    assert_eq!(created, json!({"balance":1000}));
    let market_cookie = credential(&headers, "board-event-market");
    let (_, missing) = response(
        &web,
        request(
            "/xa24tb.php",
            "mode=buy&stock=PEPE&amount=1&price=100",
            None,
            false,
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(missing["error"], "Account not found");
    let (_, stale) = response(
        &web,
        request(
            "/xa24tb.php",
            "mode=buy&stock=PEPE&amount=1&price=101",
            None,
            false,
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(stale["error"], "The price has changed.");
    let (_, cooldown) = response(
        &web,
        request(
            "/xa24tb.php",
            "mode=buy&stock=PEPE&amount=1&price=100",
            Some(&market_cookie),
            false,
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        cooldown["error"],
        "You can only make an order once every 10 seconds"
    );
    sqlx::query("UPDATE event_private.accounts SET last_action_at=$2 WHERE run_id=$1")
        .bind(market)
        .bind(now - 100)
        .execute(&owner)
        .await
        .unwrap();
    let (headers, bought) = response(
        &web,
        request(
            "/xa24tb.php",
            "mode=buy&stock=PEPE&amount=1&price=100",
            Some(&market_cookie),
            false,
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(bought, json!({"balance":900,"PEPE":1}));
    assert!(!headers.contains_key(header::SET_COOKIE));
    sqlx::query("UPDATE event_private.accounts SET last_action_at=$2 WHERE run_id=$1")
        .bind(market)
        .bind(now - 100)
        .execute(&owner)
        .await
        .unwrap();
    let (_, sold) = response(
        &web,
        request(
            "/xa24tb.php",
            "mode=sell&stock=PEPE&amount=1&price=100",
            Some(&market_cookie),
            true,
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(sold, json!({"balance":1000}));
    let forged = market_cookie.replace("=ev1.", "=ev2.");
    let (_, refused) = response(
        &web,
        request(
            "/xa24tb.php",
            "mode=buy&stock=PEPE&amount=1&price=100",
            Some(&forged),
            false,
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(refused["error"], "Account not found");
    let (_, bad) = response(
        &web,
        request(
            "/emotes_xa22.php",
            "action=start&established=true",
            None,
            false,
        ),
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert_eq!(bad["status"], "error");
    let (headers, started) = response(
        &web,
        request("/emotes_xa22.php", "action=start", None, true),
        StatusCode::OK,
    )
    .await;
    assert_eq!(started["status"], "success");
    assert_eq!(started["data"]["balance"], 0);
    assert_eq!(started["data"]["owned"], json!([]));
    let emote_cookie = credential(&headers, "board-event-emotes");
    let (headers, recovered) = response(
        &web,
        request(
            "/emotes_xa22.php",
            "action=start",
            Some(&emote_cookie),
            false,
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(recovered, started);
    assert!(!headers.contains_key(header::SET_COOKIE));
    let (_, points) = response(
        &web,
        request(
            "/emotes_xa22.php",
            "action=buy&eid=happy",
            Some(&emote_cookie),
            false,
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        points,
        json!({"status":"error","msg":"Not enough points","pts":0})
    );
    sqlx::query("UPDATE event_private.accounts SET created_at=$2 WHERE run_id=$1")
        .bind(emote)
        .bind(now - 900)
        .execute(&owner)
        .await
        .unwrap();
    let (_, bought) = response(
        &web,
        request(
            "/emotes_xa22.php",
            "action=buy&eid=happy",
            Some(&emote_cookie),
            false,
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(
        bought,
        json!({"status":"success","data":{"obtained":["happy",1,"&#x1F600;"],"balance":-50}})
    );
    let (_, duplicate) = response(
        &web,
        request(
            "/emotes_xa22.php",
            "action=buy&eid=happy",
            Some(&emote_cookie),
            false,
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(duplicate["msg"], "You already have that emote");
    sqlx::query("UPDATE event_private.accounts SET balance=100,created_at=$2 WHERE run_id=$1")
        .bind(emote)
        .bind(now)
        .execute(&owner)
        .await
        .unwrap();
    let (_, rolled) = response(
        &web,
        request(
            "/emotes_xa22.php",
            "action=roll10",
            Some(&emote_cookie),
            false,
        ),
        StatusCode::OK,
    )
    .await;
    assert_eq!(rolled["status"], "success");
    let drawn = rolled["data"]["obtained"].as_array().unwrap();
    assert_eq!(drawn.len(), 10);
    let drawn_ids: Vec<&str> = drawn
        .iter()
        .map(|row| {
            if row[1] == 1 {
                row[0].as_str().unwrap()
            } else {
                row[2].as_str().unwrap()
            }
        })
        .collect();
    let model = emotes::Account::from_stored(now, 100, &["happy".into()])
        .unwrap()
        .apply_roll(&drawn_ids, now)
        .unwrap();
    assert_eq!(rolled["data"]["balance"], model.account.balance());
    let (_, duplicate_fields) = response(
        &web,
        request(
            "/emotes_xa22.php",
            "action=start&action=start",
            Some(&emote_cookie),
            false,
        ),
        StatusCode::UNPROCESSABLE_ENTITY,
    )
    .await;
    assert_eq!(duplicate_fields["status"], "error");
    let (_, oversized) = response(
        &web,
        request(
            "/emotes_xa22.php",
            &format!("action={}", "x".repeat(8193)),
            Some(&emote_cookie),
            false,
        ),
        StatusCode::PAYLOAD_TOO_LARGE,
    )
    .await;
    assert_eq!(oversized["status"], "error");
    let mut cross = request("/xa24tb.php", "create=1", None, false);
    cross
        .headers_mut()
        .insert(header::ORIGIN, "http://other.invalid".parse().unwrap());
    assert_eq!(
        web.clone().oneshot(cross).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let duplicate_cookie = format!("{market_cookie}; {market_cookie}");
    response(
        &web,
        request("/xa24tb.php", "create=1", Some(&duplicate_cookie), false),
        StatusCode::BAD_REQUEST,
    )
    .await;
    let mut missing_peer = request("/xa24tb.php", "create=1", None, false);
    missing_peer
        .extensions_mut()
        .remove::<ConnectInfo<SocketAddr>>();
    assert_eq!(
        web.clone().oneshot(missing_peer).await.unwrap().status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    if std::env::var("RUN_EVENT_BROWSER").as_deref() == Ok("1") {
        check_browser(&owner, &public, market, emote).await;
    }
    assert!(admin::close(&mut connection, market).await.unwrap());
    assert!(admin::close(&mut connection, emote).await.unwrap());
    let (_, closed) = response(
        &web,
        request("/xa24tb.php", "create=1", Some(&market_cookie), false),
        StatusCode::OK,
    )
    .await;
    assert_eq!(closed, json!({"error":"The market is closed"}));
    assert_eq!(
        web.oneshot(request(
            "/emotes_xa22.php",
            "action=start",
            Some(&emote_cookie),
            false
        ))
        .await
        .unwrap()
        .status(),
        StatusCode::NOT_FOUND
    );
}

async fn check_browser(owner: &PgPool, public: &PgPool, market: i64, emote: i64) {
    use board_domain::source_events::credentials::Kind;
    let key = PosterIdKey::parse(&"4".repeat(64)).unwrap().event_key();
    let now = chrono::Utc::now().timestamp();
    for (kind, id) in [(Kind::Market, market), (Kind::Emotes, emote)] {
        let run = board_store::events::active_run(public, kind)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(run.id(), id);
        let session = key
            .issue_for_peer(run, "127.0.0.1".parse().unwrap(), now)
            .unwrap();
        board_store::events::open_account(public, run, &session, false)
            .await
            .unwrap()
            .unwrap();
        // Owned fixture points and elapsed cooldown; the browser cannot write them.
        sqlx::query("UPDATE event_private.accounts SET balance=1000,last_action_at=0 WHERE run_id=$1 AND actor_hash=$2")
            .bind(id).bind(session.storage_hash().as_slice()).execute(owner).await.unwrap();
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let app = router_at(public.clone(), true, &origin);
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let mut command = tokio::process::Command::new("node");
    command.env_clear();
    for name in [
        "PATH",
        "HOME",
        "PLAYWRIGHT_BROWSERS_PATH",
        "LANG",
        "SystemRoot",
        "TEMP",
        "TMP",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(180),
        command
            .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
            .arg("tests/browser/events-native.mjs")
            .arg(origin)
            .kill_on_drop(true)
            .output(),
    )
    .await;
    server.abort();
    let _ = server.await;
    let output = output.expect("event browser timeout").unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
