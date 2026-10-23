#![cfg(feature = "database-tests")]
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{Request, StatusCode, header},
};
use board_domain::{
    challenge_secrets::{ReceiptToken, TokenKind, VerificationKey, VerificationToken},
    poster_id::PosterIdKey,
    verification_mail::MailKey,
};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tower::ServiceExt;
#[path = "event_likes/browser.rs"]
mod browser;
const ORIGIN: &str = "http://localhost:3000";
fn request(path: &str, fields: Option<&str>, cookie: Option<&str>) -> Request<Body> {
    let mut builder = if fields.is_some() {
        Request::post(path)
    } else {
        Request::get(path)
    };
    builder = builder
        .header("origin", ORIGIN)
        .header("content-type", "application/x-www-form-urlencoded")
        .extension(ConnectInfo(
            "192.0.2.42:1234".parse::<SocketAddr>().unwrap(),
        ));
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    builder
        .body(Body::from(fields.unwrap_or("").to_owned()))
        .unwrap()
}
fn field(body: &str, name: &str) -> String {
    body.split(&format!("name=\"{name}\" value=\""))
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .into()
}
async fn body(response: axum::response::Response) -> String {
    String::from_utf8(
        to_bytes(response.into_body(), 262144)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}
#[tokio::test]
#[ignore = "Requires isolated verifier credentials for the owned native score form."]
async fn native_like_form_uses_real_issuer_and_atomic_event_authority() {
    let owner = PgPoolOptions::new()
        .max_connections(5)
        .connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
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
    let material = ReceiptToken::generate().unwrap().encode();
    let key = VerificationKey::parse(&material[3..]).unwrap();
    let board = format!("lh{}", &material[3..11]);
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,captcha_enabled) VALUES($1,'Owned score forms','Synthetic',1000,100,100,100,10,false)")
        .bind(&board).execute(&owner).await.unwrap();
    let op: i64 = sqlx::query_scalar("INSERT INTO content.threads(board) VALUES($1) RETURNING id")
        .bind(&board)
        .fetch_one(&owner)
        .await
        .unwrap();
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','Owned event','Synthetic')").bind(op).bind(&board).execute(&owner).await.unwrap();
    let post:i64=sqlx::query_scalar("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,$2,'Anonymous','','Owned like target') RETURNING id").bind(&board).bind(op).fetch_one(&owner).await.unwrap();
    sqlx::query("INSERT INTO post_secrets.posting_history(post_id,board,thread_id,actor_hash,request_at) VALUES($1,$2,$3,$4,1)").bind(post).bind(&board).bind(op).bind([55u8;32].as_slice()).execute(&owner).await.unwrap();
    let run = post;
    sqlx::query("INSERT INTO event_private.runs(id,kind,enabled,starts_at,ends_at) VALUES($1,3,true,1,253402300799)").bind(run).execute(&owner).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = listener.local_addr().unwrap();
    let service = board_verification::router(
        board_verification::AppState::new(
            issuer.clone(),
            VerificationKey::parse(&material[3..]).unwrap(),
            MailKey::parse(&material[3..]).unwrap(),
            &"11".repeat(32),
        )
        .unwrap(),
    );
    let server = tokio::spawn(async move {
        axum::serve(listener, service).await.unwrap();
    });
    let settings = board_config::PublicVerificationSettings::parse(
        Some(&endpoint.to_string()),
        Some(&"11".repeat(32)),
    )
    .unwrap();
    let web = board_public::routers_with_events(
        public.clone(),
        board_public::PublicRouterOptions {
            origin: ORIGIN.into(),
            production: false,
            media: None,
            limits: Default::default(),
            proxy_uid: None,
            poster_id_key: Some(Arc::new(PosterIdKey::parse(&"44".repeat(32)).unwrap())),
            tripcode_key: None,
            country_database: None,
        },
        settings,
        Default::default(),
        true,
    )
    .0;
    let sessions = Arc::new(Mutex::new(Vec::new()));
    let peer_hash = key.network_identity("192.0.2.42".parse().unwrap());
    let work = {
        let owner = owner.clone();
        let public = public.clone();
        let board = board.clone();
        let sessions = sessions.clone();
        let browser_key = VerificationKey::parse(&material[3..]).unwrap();
        tokio::spawn(async move {
            exercise(
                web,
                owner.clone(),
                key,
                board.clone(),
                run,
                post,
                sessions.clone(),
            )
            .await;
            if std::env::var("RUN_SCORE_BROWSER").as_deref() == Ok("1") {
                browser::run(browser::Fixture {
                    owner: &owner,
                    public: &public,
                    endpoint,
                    key: browser_key,
                    board: &board,
                    op,
                    run,
                    sessions,
                })
                .await;
            }
        })
    };
    let outcome = work.await;
    server.abort();
    let _ = server.await;
    let cleanup = sessions.lock().unwrap().clone();
    for token in cleanup {
        sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
            .bind(token.as_slice())
            .execute(&owner)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM verification_private.peers WHERE peer_hash=$1")
        .bind(peer_hash.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM event_private.runs WHERE id=$1")
        .bind(run)
        .execute(&owner)
        .await
        .unwrap();
    for query in [
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
    sqlx::query("SELECT pg_advisory_unlock(2240251)")
        .execute(&mut *lock)
        .await
        .unwrap();
    drop(lock);
    issuer.close().await;
    public.close().await;
    owner.close().await;
    outcome.unwrap();
}
async fn exercise(
    web: Router,
    owner: PgPool,
    key: VerificationKey,
    board: String,
    run: i64,
    post: i64,
    sessions: Arc<Mutex<Vec<[u8; 32]>>>,
) {
    let overview = web
        .clone()
        .oneshot(request("/event-perks.html", None, None))
        .await
        .unwrap();
    assert_eq!(overview.status(), StatusCode::OK);
    assert!(!overview.headers().contains_key(header::SET_COOKIE));
    let overview = body(overview).await;
    for name in board_domain::source_events::PERKS {
        assert!(overview.contains(&format!("<strong>{name}</strong>")));
    }
    let redirect = web
        .clone()
        .oneshot(request(
            &format!("/event-like.html?board={board}&post={post}"),
            None,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(redirect.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        redirect.headers()[header::LOCATION],
        format!("/event-like/{board}/{post}")
    );
    assert!(!redirect.headers().contains_key(header::SET_COOKIE));
    for query in [
        "board=g&post=0",
        "board=g&post=001",
        "board=g&post=1&post=2",
        "board=../../&post=1",
        "board=g&post=1&extra=x",
    ] {
        assert_eq!(
            web.clone()
                .oneshot(request(&format!("/event-like.html?{query}"), None, None))
                .await
                .unwrap()
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    let path = format!("/event-like/{board}/{post}");
    let response = web
        .clone()
        .oneshot(request(&path, None, None))
        .await
        .unwrap();
    if response.status() != StatusCode::OK {
        let status = response.status();
        panic!("Like form returned {status}: {}", body(response).await);
    }
    assert!(
        response.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap()
            .contains("data:")
    );
    let cookie = response
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().split(';').next().unwrap())
        .collect::<Vec<_>>()
        .join("; ");
    let anonymous = cookie
        .split("; ")
        .find_map(|v| v.strip_prefix("board-anon="))
        .unwrap();
    let capability = board_domain::anonymous_session::Capability::parse(anonymous).unwrap();
    sessions
        .lock()
        .unwrap()
        .push(capability.fingerprints(None, *b"XX").token);
    let page = body(response).await;
    assert!(page.contains("This post has 0 likes."));
    let csrf = field(&page, "csrf");
    let challenge = field(&page, "challenge");
    let token = VerificationToken::parse(TokenKind::Challenge, &challenge).unwrap();
    let suspicious: bool = sqlx::query_scalar(
        "SELECT suspicious FROM verification_private.challenges WHERE token_hash=$1",
    )
    .bind(key.token_hash(&token).as_slice())
    .fetch_one(&owner)
    .await
    .unwrap();
    let answer = key
        .challenge_answer(&token, if suspicious { 8 } else { 6 })
        .unwrap();
    let fields = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("csrf", &csrf)
        .append_pair("run", &run.to_string())
        .append_pair("challenge", &challenge)
        .append_pair("answer", answer.as_str())
        .finish();
    let forged = fields.replace(&csrf, &"00".repeat(32));
    assert_eq!(
        web.clone()
            .oneshot(request(&path, Some(&forged), Some(&cookie)))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM event_private.score_votes WHERE run_id=$1"
        )
        .bind(run)
        .fetch_one(&owner)
        .await
        .unwrap(),
        0
    );
    let mut cross = request(&path, Some(&fields), Some(&cookie));
    cross
        .headers_mut()
        .insert(header::ORIGIN, "https://other.invalid".parse().unwrap());
    assert_eq!(
        web.clone().oneshot(cross).await.unwrap().status(),
        StatusCode::FORBIDDEN
    );
    let response = web
        .clone()
        .oneshot(request(&path, Some(&fields), Some(&cookie)))
        .await
        .unwrap();
    let status = response.status();
    let location = response.headers().get(header::LOCATION).cloned();
    let detail = body(response).await;
    assert_eq!(status, StatusCode::SEE_OTHER, "{detail}");
    assert_eq!(location.unwrap(), format!("{path}?view=1"));
    let response = web
        .clone()
        .oneshot(request(&format!("{path}?view=1"), None, None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!response.headers().contains_key(header::SET_COOKIE));
    let page = body(response).await;
    assert!(page.contains("This post has 1 like."));
    assert!(!page.contains("name=\"challenge\""));
    assert!(
        web.clone()
            .oneshot(request(&path, Some(&fields), Some(&cookie)))
            .await
            .unwrap()
            .status()
            .is_client_error()
    );
    assert_eq!(
        sqlx::query_scalar::<_, i32>("SELECT vote_count FROM event_private.runs WHERE id=$1")
            .bind(run)
            .fetch_one(&owner)
            .await
            .unwrap(),
        1
    );
    let op: i64 = sqlx::query_scalar("SELECT thread_id FROM content.posts WHERE id=$1")
        .bind(post)
        .fetch_one(&owner)
        .await
        .unwrap();
    let next:i64=sqlx::query_scalar("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,$2,'Anonymous','','Owned known-address target') RETURNING id")
        .bind(&board).bind(op).fetch_one(&owner).await.unwrap();
    sqlx::query("INSERT INTO post_secrets.posting_history(post_id,board,thread_id,actor_hash,request_at) VALUES($1,$2,$3,$4,1)")
        .bind(next).bind(&board).bind(op).bind([55u8;32].as_slice()).execute(&owner).await.unwrap();
    let next_path = format!("/event-like/{board}/{next}");
    // A fresh browser inherits only the source's address history, not a proof.
    let known = web
        .clone()
        .oneshot(request(&next_path, None, None))
        .await
        .unwrap();
    assert_eq!(known.status(), StatusCode::OK);
    let known_cookie = known
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().split(';').next().unwrap())
        .collect::<Vec<_>>()
        .join("; ");
    let token = known_cookie
        .split("; ")
        .find_map(|v| v.strip_prefix("board-anon="))
        .unwrap();
    sessions.lock().unwrap().push(
        board_domain::anonymous_session::Capability::parse(token)
            .unwrap()
            .fingerprints(None, *b"XX")
            .token,
    );
    let known_page = body(known).await;
    assert!(known_page.contains("No CAPTCHA is required for this address."));
    assert!(!known_page.contains("name=\"challenge\""));
    let known_fields = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("csrf", &field(&known_page, "csrf"))
        .append_pair("run", &run.to_string())
        .finish();
    let cooldown = web
        .clone()
        .oneshot(request(
            &next_path,
            Some(&known_fields),
            Some(&known_cookie),
        ))
        .await
        .unwrap();
    assert!(cooldown.status().is_client_error());
    assert!(
        body(cooldown)
            .await
            .contains("You have to wait a while before liking this post.")
    );
    sqlx::query("UPDATE event_private.score_accounts SET last_vote_at=0 WHERE run_id=$1")
        .bind(run)
        .execute(&owner)
        .await
        .unwrap();
    let forced_cookie = format!("{known_cookie}; 4chan_auser=");
    assert!(
        web.clone()
            .oneshot(request(
                &next_path,
                Some(&known_fields),
                Some(&forced_cookie)
            ))
            .await
            .unwrap()
            .status()
            .is_client_error()
    );
    let peer = key.network_identity("192.0.2.42".parse().unwrap());
    sqlx::query("UPDATE verification_private.peers SET refresh_after=clock_timestamp()-interval '1 second' WHERE peer_hash=$1").bind(peer.as_slice()).execute(&owner).await.unwrap();
    let forced = web
        .clone()
        .oneshot(request(&next_path, None, Some(&forced_cookie)))
        .await
        .unwrap();
    assert_eq!(forced.status(), StatusCode::OK);
    assert!(body(forced).await.contains("name=\"challenge\""));
    assert_eq!(
        web.clone()
            .oneshot(request(
                &next_path,
                Some(&known_fields),
                Some(&known_cookie)
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::SEE_OTHER
    );
    assert_eq!(
        sqlx::query_scalar::<_, i32>("SELECT vote_count FROM event_private.runs WHERE id=$1")
            .bind(run)
            .fetch_one(&owner)
            .await
            .unwrap(),
        2
    );
    sqlx::query("UPDATE event_private.runs SET enabled=false WHERE id=$1")
        .bind(run)
        .execute(&owner)
        .await
        .unwrap();
    let closed = web
        .clone()
        .oneshot(request("/event-perks.html", None, None))
        .await
        .unwrap();
    assert_eq!(closed.status(), StatusCode::OK);
    assert!(!closed.headers().contains_key(header::SET_COOKIE));
    assert!(body(closed).await.contains("This event is closed."));
    assert_eq!(
        web.clone()
            .oneshot(request(
                &format!("/event-like.html?board={board}&post={post}"),
                None,
                None
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        web.oneshot(request(&path, None, Some(&cookie)))
            .await
            .unwrap()
            .status(),
        StatusCode::NOT_FOUND
    );
}
