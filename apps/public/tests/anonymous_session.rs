#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode, header},
};
use board_domain::anonymous_session::Capability;
use sqlx::PgPool;
use tower::ServiceExt;

fn fixture_key() -> std::sync::Arc<board_domain::poster_id::PosterIdKey> {
    use rand_core::RngCore;
    let mut bytes = [0u8; 32];
    rand_core::OsRng.fill_bytes(&mut bytes);
    let encoded: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    std::sync::Arc::new(board_domain::poster_id::PosterIdKey::parse(&encoded).unwrap())
}

const ORIGIN: &str = "http://127.0.0.1:3000";

fn form(
    path: &str,
    fields: &[(&str, &str)],
    cookie: Option<&str>,
    multipart: bool,
) -> Request<Body> {
    let mut request = Request::post(path)
        .header("origin", ORIGIN)
        .header("accept", "application/json");
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    let body = if multipart {
        request = request.header("content-type", "multipart/form-data; boundary=owned-anon");
        let mut body = String::new();
        for (name, value) in fields {
            body.push_str(&format!(
                "--owned-anon\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            ));
        }
        body.push_str("--owned-anon--\r\n");
        body
    } else {
        request = request.header("content-type", "application/x-www-form-urlencoded");
        url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(fields.iter().copied())
            .finish()
    };
    request.body(Body::from(body)).unwrap()
}

fn anonymous_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| {
            let value = value.to_str().unwrap();
            if !value.starts_with("board-anon=") {
                return None;
            }
            assert!(value.contains("Path=/; Max-Age=31536000; HttpOnly; SameSite=Strict"));
            assert!(!value.contains("Domain=") && !value.contains("Secure"));
            assert_eq!(headers[header::CACHE_CONTROL], "no-store");
            Some(value.split(';').next().unwrap().to_owned())
        })
        .next()
}

async fn posted(
    app: &Router,
    path: &str,
    fields: &[(&str, &str)],
    cookie: Option<&str>,
    multipart: bool,
) -> (i64, String, HeaderMap) {
    let response = app
        .clone()
        .oneshot(form(path, fields, cookie, multipart))
        .await
        .unwrap();
    let (parts, body) = response.into_parts();
    let bytes = to_bytes(body, 65536).await.unwrap();
    assert_eq!(
        parts.status,
        StatusCode::OK,
        "{}",
        String::from_utf8_lossy(&bytes)
    );
    let result: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(result.as_object().unwrap().len(), 2);
    let cookie = anonymous_cookie(&parts.headers).unwrap();
    (result["pid"].as_i64().unwrap(), cookie, parts.headers)
}

async fn get(app: &Router, path: &str, cookie: Option<&str>) -> (HeaderMap, Vec<u8>) {
    let mut request = Request::get(path);
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!response.headers().contains_key(header::SET_COOKIE));
    let (parts, body) = response.into_parts();
    (
        parts.headers,
        to_bytes(body, 1_048_576).await.unwrap().to_vec(),
    )
}

async fn fixture() -> (
    PgPool,
    PgPool,
    String,
    std::sync::Arc<board_domain::poster_id::PosterIdKey>,
) {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT 'as'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,op_markup,deletion_known_min_seconds,deletion_unknown_min_seconds,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) VALUES($1,'Owned anonymous HTTP','Synthetic',2000,100,100,100,10,true,0,0,0,0,0)")
        .bind(&board).execute(&owner).await.unwrap();
    (owner, public, board, fixture_key())
}

async fn cleanup(owner: &PgPool, board: &str) {
    let tokens: Vec<Vec<u8>> = sqlx::query_scalar("SELECT a.token_hash FROM post_secrets.anonymous_posts a JOIN content.posts p ON p.id=a.post_id WHERE p.board=$1 UNION SELECT a.token_hash FROM post_secrets.anonymous_reports a JOIN content.reports r ON r.id=a.report_id WHERE r.board=$1")
        .bind(board).fetch_all(owner).await.unwrap();
    for query in [
        "DELETE FROM content.reports WHERE board=$1",
        "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(query).bind(board).execute(owner).await.unwrap();
    }
    for token in tokens {
        sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
            .bind(token)
            .execute(owner)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn automatic_cookie_owns_posts_across_forms_and_tabs_without_public_identity_leaks() {
    let (owner, public, board, key) = fixture().await;
    let (a, p, b) = (owner.clone(), public.clone(), board.clone());
    let outcome = tokio::spawn(async move {
        let (app, api) = fixture_routers(p.clone(), ORIGIN.into(), false, None, board_config::PublicRequestLimits::default(), key.clone());
        let (_, form_html) = get(&app, &format!("/{b}/"), None).await;
        let form_html = String::from_utf8(form_html).unwrap();
        assert!(form_html.contains("id=\"postPassword\" name=\"pwd\" type=\"hidden\""));
        assert!(!form_html.contains("id=\"password\"") && !form_html.contains("Save your deletion password"));
        let (op, cookie, headers) = posted(&app, &format!("/{b}/post"), &[("com", "Owned passwordless OP"), ("name", "Owned#private-trip"), ("email", "sage")], None, false).await;
        let capability = Capability::parse(cookie.strip_prefix("board-anon=").unwrap()).unwrap();
        let token = capability.storage_hash();
        for name in ["4chan_name=Owned;", "options=sage;"] {
            let preference = headers.get_all(header::SET_COOKIE).iter().find(|value| value.to_str().unwrap().starts_with(name)).unwrap().to_str().unwrap();
            assert!(preference.contains("Max-Age=604800") && !preference.contains("private-trip"));
        }
        let parent = op.to_string();
        let (reply, next_cookie, _) = posted(&app, &format!("/{b}/imgboard.php"), &[("mode", "regist"), ("resto", &parent), ("pwd", ""), ("com", "Owned multipart reply")], Some(&cookie), true).await;
        assert_eq!(next_cookie, cookie);
        let left_path = format!("/{b}/post");
        let right_path = format!("/{b}/imgboard.php");
        let left_fields = [("resto", parent.as_str()), ("com", "Owned first tab")];
        let right_fields = [("resto", parent.as_str()), ("com", "Owned second tab")];
        let left = posted(&app, &left_path, &left_fields, Some(&cookie), false);
        let right = posted(&app, &right_path, &right_fields, Some(&cookie), false);
        let ((_, left_cookie, _), (_, right_cookie, _)) = tokio::join!(left, right);
        assert_eq!(left_cookie, cookie);
        assert_eq!(right_cookie, cookie);
        let bindings: (i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM post_secrets.anonymous_sessions WHERE token_hash=$1),(SELECT count(*) FROM post_secrets.anonymous_posts WHERE token_hash=$1)")
            .bind(token.as_slice()).fetch_one(&a).await.unwrap();
        assert_eq!(bindings, (1, 4));
        let (plain_headers, plain) = get(&api, &format!("/{b}/thread/{op}.json"), None).await;
        let (cookie_headers, with_cookie) = get(&api, &format!("/{b}/thread/{op}.json"), Some(&cookie)).await;
        assert_eq!(plain, with_cookie);
        assert_eq!(plain_headers.get(header::ETAG), cookie_headers.get(header::ETAG));
        let json = String::from_utf8(plain).unwrap();
        for private in [cookie.as_str(), "token_hash", "verified_level", "network_hash", "address_hash", "change_score", "password_hash"] { assert!(!json.contains(private)); }
        let receipt = format!("4chan_awt={op}; board-posted-{op}={reply}.1");
        let deletion = format!("no={reply}");
        for forged in [None, Some(receipt.as_str()), Some("board-anon=a1_0000000000000000000000000000000000000000000000000000000000000000")] {
            let response = app.clone().oneshot(form(&format!("/{b}/delete"), &[("no", &reply.to_string())], forged, false)).await.unwrap();
            assert_eq!(response.status(), StatusCode::FORBIDDEN, "{deletion}");
            assert!(!response.headers().contains_key(header::SET_COOKIE));
        }
        let response = app.clone().oneshot(form(&format!("/{b}/imgboard.php"), &[("mode", "usrdel"), (&reply.to_string(), "delete"), ("pwd", "")], Some(&cookie), true)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert!(matches!(board_store::find_post(&p, &b, reply).await, Err(board_store::StoreError::NotFound)));
        assert!(board_store::find_post(&p, &b, op).await.is_ok());
    }).await;
    cleanup(&owner, &board).await;
    outcome.unwrap();
}

#[tokio::test]
async fn malformed_reset_and_expired_cookies_cannot_choose_or_recover_an_identity() {
    let (owner, public, board, key) = fixture().await;
    let (a, p, b) = (owner.clone(), public.clone(), board.clone());
    let outcome = tokio::spawn(async move {
        let app = fixture_routers(
            p.clone(),
            ORIGIN.into(),
            false,
            None,
            board_config::PublicRequestLimits::default(),
            key.clone(),
        )
        .0;
        let chosen = Capability::generate().unwrap();
        let unknown = format!("board-anon={}", chosen.credential());
        let (op, cookie, _) = posted(
            &app,
            &format!("/{b}/post"),
            &[("com", "Owned replacement identity")],
            Some(&unknown),
            false,
        )
        .await;
        assert_ne!(cookie, unknown);
        assert!(
            board_store::anonymous_session::snapshot(&p, &chosen.storage_hash())
                .await
                .unwrap()
                .is_none()
        );
        let token = Capability::parse(cookie.strip_prefix("board-anon=").unwrap())
            .unwrap()
            .storage_hash();
        let before: i64 = sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1")
            .bind(&b)
            .fetch_one(&a)
            .await
            .unwrap();
        for invalid in [
            format!("{cookie}; {cookie}"),
            format!("unrelated={}", "x".repeat(8193)),
        ] {
            let response = app
                .clone()
                .oneshot(form(
                    &format!("/{b}/post"),
                    &[("com", "Uncommitted ambiguous identity")],
                    Some(&invalid),
                    false,
                ))
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK);
            assert!(!response.headers().contains_key(header::SET_COOKIE));
            let value: serde_json::Value =
                serde_json::from_slice(&to_bytes(response.into_body(), 65536).await.unwrap())
                    .unwrap();
            assert_eq!(value.as_object().unwrap().len(), 1);
            assert!(matches!(
                value["error"].as_str(),
                Some("Ambiguous anonymous session cookie." | "Invalid cookie header.")
            ));
        }
        let response = app
            .clone()
            .oneshot(form(
                &format!("/{b}/post"),
                &[("com", "Forged verification"), ("verified_level", "1")],
                Some(&cookie),
                false,
            ))
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM content.posts WHERE board=$1")
                .bind(&b)
                .fetch_one(&a)
                .await
                .unwrap(),
            before
        );
        let (_, reset_cookie, _) = posted(
            &app,
            &format!("/{b}/post"),
            &[("com", "Owned cleared-cookie identity")],
            None,
            false,
        )
        .await;
        assert_ne!(reset_cookie, cookie);
        assert_eq!(
            app.clone()
                .oneshot(form(
                    &format!("/{b}/delete"),
                    &[("no", &op.to_string())],
                    Some(&reset_cookie),
                    false
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        sqlx::query("UPDATE post_secrets.anonymous_sessions SET expires_at=1 WHERE token_hash=$1")
            .bind(token.as_slice())
            .execute(&a)
            .await
            .unwrap();
        let (_, replacement, _) = posted(
            &app,
            &format!("/{b}/post"),
            &[("com", "Owned expired identity replacement")],
            Some(&cookie),
            false,
        )
        .await;
        assert_ne!(replacement, cookie);
        assert_eq!(
            app.oneshot(form(
                &format!("/{b}/delete"),
                &[("no", &op.to_string())],
                Some(&replacement),
                false
            ))
            .await
            .unwrap()
            .status(),
            StatusCode::FORBIDDEN
        );
    })
    .await;
    cleanup(&owner, &board).await;
    outcome.unwrap();
}

#[tokio::test]
async fn report_activity_is_private_and_only_successful_reports_receive_cookies() {
    let (owner, public, board, key) = fixture().await;
    let (a, p, b) = (owner.clone(), public.clone(), board.clone());
    let outcome = tokio::spawn(async move {
        let app = fixture_routers(p, ORIGIN.into(), false, None, board_config::PublicRequestLimits::default(), key.clone()).0;
        let (op, _, _) = posted(&app, &format!("/{b}/post"), &[("com", "Owned report target")], None, false).await;
        let response = app.clone().oneshot(form(&format!("/{b}/report"), &[("no", &op.to_string()), ("reason", "Owned anonymous report")], None, false)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let cookie = anonymous_cookie(response.headers()).unwrap();
        let token = Capability::parse(cookie.strip_prefix("board-anon=").unwrap()).unwrap().storage_hash();
        let stored: (i16, i64, i64) = sqlx::query_as("SELECT s.pending,(SELECT count(*) FROM post_secrets.anonymous_reports r WHERE r.token_hash=s.token_hash),(SELECT count(*) FROM post_secrets.anonymous_posts p WHERE p.token_hash=s.token_hash) FROM post_secrets.anonymous_sessions s WHERE s.token_hash=$1")
            .bind(token.as_slice()).fetch_one(&a).await.unwrap();
        assert_eq!(stored, (8, 1, 0));
        let op_text = op.to_string();
        for fields in [vec![("no", "0"), ("reason", "Owned missing target")], vec![("no", op_text.as_str()), ("reason", "")]] {
            let response = app.clone().oneshot(form(&format!("/{b}/report"), &fields, Some(&cookie), false)).await.unwrap();
            assert!(response.status().is_client_error());
            assert!(!response.headers().contains_key(header::SET_COOKIE));
        }
        let reports: i64 = sqlx::query_scalar("SELECT count(*) FROM content.reports WHERE board=$1").bind(&b).fetch_one(&a).await.unwrap();
        assert_eq!(reports, 1);
    }).await;
    cleanup(&owner, &board).await;
    outcome.unwrap();
}

#[tokio::test]
async fn public_deletion_resumes_the_cookie_with_the_current_transport_peer() {
    let (owner, public, board, key) = fixture().await;
    sqlx::query("UPDATE content.boards SET deletion_known_min_seconds=60,deletion_unknown_min_seconds=600 WHERE slug=$1")
        .bind(&board).execute(&owner).await.unwrap();
    for route in 0..3 {
        let app = fixture_routers(
            public.clone(),
            ORIGIN.into(),
            false,
            None,
            board_config::PublicRequestLimits::default(),
            key.clone(),
        )
        .0;
        let (post, cookie, _) = posted(
            &app,
            &format!("/{board}/post"),
            &[("com", "Owned current peer deletion test")],
            None,
            false,
        )
        .await;
        let capability = Capability::parse(cookie.strip_prefix("board-anon=").unwrap()).unwrap();
        let token = capability.storage_hash();
        sqlx::query("UPDATE content.posts SET created_at=clock_timestamp()-interval '61 seconds' WHERE id=$1").bind(post).execute(&owner).await.unwrap();
        sqlx::query("UPDATE post_secrets.anonymous_sessions SET created_at=extract(epoch FROM clock_timestamp())::bigint-1000,network_at=extract(epoch FROM clock_timestamp())::bigint-901,activity_at=extract(epoch FROM clock_timestamp())::bigint,verified_level=3,change_score=12 WHERE token_hash=$1")
            .bind(token.as_slice()).execute(&owner).await.unwrap();
        let id = post.to_string();
        let path = format!(
            "/{board}/{}",
            if route == 0 { "delete" } else { "imgboard.php" }
        );
        let fields = if route == 0 {
            vec![("no", id.as_str())]
        } else {
            vec![("mode", "usrdel"), (id.as_str(), "delete"), ("pwd", "")]
        };
        let mut changed = form(&path, &fields, Some(&cookie), route == 2);
        changed.extensions_mut().insert(axum::extract::ConnectInfo(
            "203.0.113.7:12345".parse::<std::net::SocketAddr>().unwrap(),
        ));
        let response = app.clone().oneshot(changed).await.unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = to_bytes(response.into_body(), 65536).await.unwrap();
        assert!(
            String::from_utf8_lossy(&body)
                .contains("Error: You must wait longer before deleting this post.")
        );
        assert!(board_store::find_post(&public, &board, post).await.is_ok());
        let mut unchanged = form(&path, &fields, Some(&cookie), route == 2);
        // Untrusted forwarding hints cannot change the server's peer identity.
        unchanged
            .headers_mut()
            .insert("x-forwarded-for", "203.0.113.7".parse().unwrap());
        let response = app.clone().oneshot(unchanged).await.unwrap();
        assert_eq!(
            response.status(),
            if route == 0 {
                StatusCode::SEE_OTHER
            } else {
                StatusCode::OK
            }
        );
        assert!(matches!(
            board_store::find_post(&public, &board, post).await,
            Err(board_store::StoreError::NotFound)
        ));
    }
    cleanup(&owner, &board).await;
}

// Fixture transport identities are isolated so unrelated authorization cases do
// not consume each other's shared public deletion quota.
fn fixture_peer() -> std::net::SocketAddr {
    static NEXT: std::sync::OnceLock<std::sync::atomic::AtomicU64> = std::sync::OnceLock::new();
    let nonce = NEXT
        .get_or_init(|| {
            std::sync::atomic::AtomicU64::new(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos() as u64,
            )
        })
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    std::net::SocketAddr::new(
        std::net::IpAddr::V6(std::net::Ipv6Addr::new(
            0x2001,
            0xdb8,
            4,
            0,
            (nonce >> 48) as u16,
            (nonce >> 32) as u16,
            (nonce >> 16) as u16,
            nonce as u16,
        )),
        12345,
    )
}

fn fixture_routers(
    pool: sqlx::PgPool,
    origin: String,
    production: bool,
    media: Option<board_config::PublicMediaSettings>,
    limits: board_config::PublicRequestLimits,
    key: std::sync::Arc<board_domain::poster_id::PosterIdKey>,
) -> (axum::Router, axum::Router) {
    let (web, api) = board_public::routers_with_options(
        pool,
        board_public::PublicRouterOptions {
            origin,
            production,
            media,
            limits,
            proxy_uid: None,
            poster_id_key: Some(key),
            tripcode_key: None,
            country_database: None,
        },
    );
    let peer = fixture_peer();
    let transport = axum::middleware::from_fn(
        move |mut request: axum::extract::Request, next: axum::middleware::Next| async move {
            if request
                .extensions()
                .get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
                .is_none()
            {
                request
                    .extensions_mut()
                    .insert(axum::extract::ConnectInfo(peer));
            }
            next.run(request).await
        },
    );
    (web.layer(transport.clone()), api.layer(transport))
}
