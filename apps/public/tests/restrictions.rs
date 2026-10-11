#![cfg(feature = "database-tests")]
use axum::{
    Router,
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{HeaderMap, Request, StatusCode},
};
use board_domain::{anonymous_session::Capability, poster_id::PosterIdKey};
use sqlx::PgPool;
use std::{net::SocketAddr, sync::Arc};
use tower::ServiceExt;

const ORIGIN: &str = "https://boards.example.com";
async fn request(
    app: &Router,
    method: &str,
    path: &str,
    peer: &str,
    cookie: Option<&str>,
    body: &str,
    origin: &str,
) -> (StatusCode, HeaderMap, String) {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("origin", origin)
        .header("content-type", "application/x-www-form-urlencoded");
    if let Some(cookie) = cookie {
        builder = builder.header("cookie", cookie);
    }
    let mut req = builder.body(Body::from(body.to_owned())).unwrap();
    req.extensions_mut()
        .insert(ConnectInfo(peer.parse::<SocketAddr>().unwrap()));
    let response = app.clone().oneshot(req).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = String::from_utf8(
        to_bytes(response.into_body(), 2_000_000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    (status, headers, body)
}

#[tokio::test]
async fn status_is_private_escaped_inert_and_warning_viewing_is_identity_and_csrf_bound() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT 'wv'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Owned warning','Synthetic fixture',2000,100,100,100,10)").bind(&board).execute(&owner).await.unwrap();
    let target: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&owner)
        .await
        .unwrap();
    sqlx::query("INSERT INTO content.threads(id,board) VALUES($1,$2)")
        .bind(target)
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Synthetic name','Owned report target','Synthetic target text')")
        .bind(target).bind(&board).execute(&owner).await.unwrap();
    let secret = Capability::generate().unwrap().storage_hash();
    let key = Arc::new(
        PosterIdKey::parse(
            &secret
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>(),
        )
        .unwrap(),
    );
    let actor = key.public_posting_rate_identity("192.0.2.134".parse().unwrap());
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let id:i64=sqlx::query_scalar("INSERT INTO restrictions.bans(board,post_id,actor_hash,global,warning,starts_at,ends_at,public_reason,private_reason,account_id,charged_bytes) VALUES($1,1,$2,false,true,now(),now(),'<script>synthetic</script>','Private test reason',1,1024) RETURNING id").bind(&board).bind(actor.as_bytes().as_slice()).fetch_one(&mut *tx).await.unwrap();
    sqlx::query("UPDATE restrictions.capacity SET used_bytes=used_bytes+1024 WHERE singleton")
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let (app, _) = board_public::routers_with_options(
        public,
        board_public::PublicRouterOptions {
            origin: ORIGIN.into(),
            production: true,
            media: None,
            limits: board_config::PublicRequestLimits::default(),
            proxy_uid: None,
            tripcode_key: None,
            country_database: None,
            poster_id_key: Some(key),
        },
    );
    let path = format!("/banned?board={board}");
    let (code, headers, body) =
        request(&app, "GET", &path, "192.0.2.134:44134", None, "", ORIGIN).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert!(
        headers["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("script-src 'none'")
    );
    assert!(
        body.contains("&#60;script&#62;synthetic&#60;/script&#62;"),
        "{body}"
    );
    assert!(!body.contains("Private test reason"));
    let set_cookie = headers["set-cookie"].to_str().unwrap();
    for attr in ["HttpOnly", "SameSite=Strict", "Secure", "Path=/"] {
        assert!(set_cookie.contains(attr));
    }
    let cookie = set_cookie.split(';').next().unwrap();
    let token = body
        .split("name=\"csrf\" value=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    let form = format!("board={board}&id={id}&csrf={token}");
    let posting = "resto=0&name=Anonymous&sub=Owned&com=Synthetic+posting&pwd=&email=";
    let (refused, refusal_headers, refusal) = request(
        &app,
        "POST",
        &format!("/{board}/post"),
        "192.0.2.134:44134",
        None,
        posting,
        ORIGIN,
    )
    .await;
    assert_eq!(refused, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(
        refusal_headers["refresh"],
        format!("3; url=/banned?board={board}")
    );
    assert!(refusal.contains(&format!("href=\"/banned?board={board}\"")));
    assert!(refusal.contains("You must first"));
    // Original GET uses check_ban=false, POST uses true. The real transport
    // peer binds the reporting HMAC to the independent posting restriction HMAC.
    let report_path = format!("/{board}/imgboard.php?mode=report&no={target}");
    assert_eq!(
        request(
            &app,
            "GET",
            &report_path,
            "192.0.2.134:44134",
            None,
            "",
            ORIGIN
        )
        .await
        .0,
        StatusCode::OK
    );
    for (report_path, report_form) in [
        (report_path.clone(), "cat=&cat_id=1&revision=1".to_owned()),
        (
            format!("/{board}/report"),
            format!("no={target}&reason=Owned+report"),
        ),
    ] {
        let (code, headers, body) = request(
            &app,
            "POST",
            &report_path,
            "192.0.2.134:44134",
            None,
            &report_form,
            ORIGIN,
        )
        .await;
        assert_eq!(code, StatusCode::UNPROCESSABLE_ENTITY, "{body}");
        assert_eq!(headers["cache-control"], "private, no-store");
        assert!(!headers.contains_key("set-cookie"));
        assert!(
            body.contains(&format!("href=\"/banned?board={board}\"")),
            "{body}"
        );
        assert!(!body.contains("Private test reason"));
    }
    let reports: i64 = sqlx::query_scalar("SELECT count(*) FROM content.reports WHERE board=$1")
        .bind(&board)
        .fetch_one(&owner)
        .await
        .unwrap();
    assert_eq!(reports, 0);
    assert_eq!(
        request(
            &app,
            "POST",
            "/banned",
            "192.0.2.134:44134",
            None,
            &form,
            ORIGIN
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/banned",
            "192.0.2.134:44134",
            Some(cookie),
            &form,
            "https://other.example"
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let invalid = format!("board={board}&id={id}&csrf={}", "f".repeat(64));
    assert_eq!(
        request(
            &app,
            "POST",
            "/banned",
            "192.0.2.134:44134",
            Some(cookie),
            &invalid,
            ORIGIN
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/banned",
            "192.0.2.135:44134",
            Some(cookie),
            &form,
            ORIGIN
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert!(
        !request(&app, "GET", &path, "192.0.2.135:44134", None, "", ORIGIN)
            .await
            .2
            .contains("synthetic")
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/banned",
            "192.0.2.134:44134",
            Some(cookie),
            &form,
            ORIGIN
        )
        .await
        .0,
        StatusCode::SEE_OTHER
    );
    assert_eq!(
        request(
            &app,
            "POST",
            "/banned",
            "192.0.2.134:44134",
            Some(cookie),
            &form,
            ORIGIN
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert!(
        request(&app, "GET", &path, "192.0.2.134:44134", None, "", ORIGIN)
            .await
            .2
            .contains("No current ban or warning")
    );
    // Register an actual reporting session, then retain its credential across
    // an idle reset and a changed transport address. No UUID crosses HTTP.
    let (code, headers, body) = request(
        &app,
        "POST",
        &format!("/{board}/report"),
        "192.0.2.134:44134",
        None,
        &format!("no={target}&reason=Owned+registration"),
        ORIGIN,
    )
    .await;
    assert_eq!(code, StatusCode::OK, "{body}");
    let session_cookie = headers["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let session_cap =
        Capability::parse(session_cookie.strip_prefix("__Host-board-anon=").unwrap()).unwrap();
    let now = chrono::Utc::now().timestamp();
    sqlx::query("UPDATE post_secrets.anonymous_sessions SET created_at=$1,activity_at=0 WHERE token_hash=$2")
        .bind(now - 604800).bind(session_cap.storage_hash().as_slice()).execute(&owner).await.unwrap();
    let mut seed = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *seed)
        .await
        .unwrap();
    let retained: i64 = sqlx::query_scalar("INSERT INTO restrictions.bans(board,post_id,actor_hash,automatic_identity,global,warning,starts_at,ends_at,public_reason,private_reason,account_id,charged_bytes) SELECT $1,$2,$3,a.automatic_identity,false,true,now(),now(),'Retained synthetic warning','Private retained reason',1,1024 FROM post_secrets.lookup_automatic_identity($4,$5) a WHERE a.automatic_identity IS NOT NULL RETURNING id")
        .bind(&board).bind(target).bind(actor.as_bytes().as_slice())
        .bind(session_cap.storage_hash().as_slice()).bind(now).fetch_one(&mut *seed).await.unwrap();
    sqlx::query("UPDATE restrictions.capacity SET used_bytes=used_bytes+1024 WHERE singleton")
        .execute(&mut *seed)
        .await
        .unwrap();
    seed.commit().await.unwrap();
    assert!(
        !request(&app, "GET", &path, "192.0.2.240:44240", None, "", ORIGIN)
            .await
            .2
            .contains("Retained synthetic warning")
    );
    let (code, headers, body) = request(
        &app,
        "GET",
        &path,
        "192.0.2.240:44240",
        Some(&session_cookie),
        "",
        ORIGIN,
    )
    .await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(headers["cache-control"], "private, no-store");
    assert!(body.contains("Retained synthetic warning"), "{body}");
    assert!(!body.contains("Private retained reason"));
    let viewing_cookie = headers["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let csrf = body
        .split("name=\"csrf\" value=\"")
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap();
    let form = format!("board={board}&id={retained}&csrf={csrf}");
    assert_eq!(
        request(
            &app,
            "POST",
            "/banned",
            "192.0.2.240:44240",
            Some(viewing_cookie),
            &form,
            ORIGIN
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let cookies = format!("{viewing_cookie}; {session_cookie}");
    assert_eq!(
        request(
            &app,
            "POST",
            "/banned",
            "192.0.2.240:44240",
            Some(&cookies),
            &form,
            ORIGIN
        )
        .await
        .0,
        StatusCode::SEE_OTHER
    );
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    for ban in [id, retained] {
        let audit: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM restrictions.audit WHERE ban_id=$1 AND action='warning-viewed'",
        )
        .bind(ban)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        assert_eq!(audit, 1);
    }
    sqlx::query("DELETE FROM restrictions.audit WHERE ban_id=ANY($1)")
        .bind(vec![id, retained])
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM restrictions.bans WHERE id=ANY($1)")
        .bind(vec![id, retained])
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE restrictions.capacity SET used_bytes=used_bytes-2048 WHERE singleton")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("RESET ROLE").execute(&mut *tx).await.unwrap();
    sqlx::query("DELETE FROM content.reports WHERE board=$1")
        .bind(&board)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
        .bind(session_cap.storage_hash().as_slice())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.posts WHERE board=$1")
        .bind(&board)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.threads WHERE board=$1")
        .bind(&board)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.boards WHERE slug=$1")
        .bind(board)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}
