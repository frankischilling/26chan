#![cfg(feature = "database-tests")]
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use board_staff::{AppState, Config, Limits, auth, router};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use tower::ServiceExt;
use webauthn_rs::prelude::*;

async fn request(
    app: &Router,
    path: &str,
    form: Option<&str>,
    cookie: &str,
) -> (StatusCode, HeaderMap, String) {
    let mut request = Request::builder().uri(path).header("cookie", cookie);
    let body = if let Some(form) = form {
        request = request
            .method("POST")
            .header("origin", "http://localhost:3001")
            .header("sec-fetch-site", "same-origin")
            .header("content-type", "application/x-www-form-urlencoded");
        Body::from(form.to_owned())
    } else {
        Body::empty()
    };
    let response = app
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    assert_eq!(headers["cache-control"], "private, no-store");
    let text = String::from_utf8(
        to_bytes(response.into_body(), 1_000_000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    (status, headers, text)
}

#[tokio::test]
async fn staff_issue_and_unban_use_live_session_scope_recent_auth_csrf_and_private_target() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let auth_pool = PgPool::connect(&std::env::var("AUTH_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let staff = PgPool::connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT 'sb'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Owned restrictions','Synthetic fixture',2000,100,100,100,10)").bind(&board).execute(&owner).await.unwrap();
    let post: i64 =
        sqlx::query_scalar("INSERT INTO content.threads(board) VALUES($1) RETURNING id")
            .bind(&board)
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','Owned restriction','Synthetic text')").bind(post).bind(&board).execute(&owner).await.unwrap();
    sqlx::query("INSERT INTO post_secrets.posting_history(post_id,board,thread_id,actor_hash,request_at) VALUES($1,$2,$1,decode(repeat('73',32),'hex'),floor(extract(epoch FROM now()))::bigint)").bind(post).bind(&board).execute(&owner).await.unwrap();
    let account:i64=sqlx::query_scalar("INSERT INTO staff_identity.accounts(role,allow_boards,deny_boards) VALUES('moderator',ARRAY[$1],'{}') RETURNING id").bind(&board).fetch_one(&owner).await.unwrap();
    let token = auth::token();
    let csrf = auth::token();
    let credential = auth::hash(&token);
    sqlx::query(
        "INSERT INTO staff_identity.credentials(id,account_id,credential) VALUES($1,$2,'{}')",
    )
    .bind(&credential)
    .bind(account)
    .execute(&owner)
    .await
    .unwrap();
    sqlx::query("INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id) VALUES($1,$2,$3,$1)").bind(&credential).bind(auth::hash(&csrf)).bind(account).execute(&owner).await.unwrap();
    let app = router(Arc::new(AppState {
        config: Config {
            media: None,
            proxy: None,
            poster_id_key: None,
            country_database: None,
            origin: "http://localhost:3001".into(),
            public_origin: "http://localhost:3000".into(),
            media_origin: "http://127.0.0.1:3002".into(),
            bind: "127.0.0.1:3001".parse().unwrap(),
            production: false,
            auth_database: String::new(),
            staff_database: String::new(),
            idle_timeout: Duration::from_secs(60),
            tripcode_key: None,
        },
        auth: auth_pool,
        staff,
        webauthn: WebauthnBuilder::new("localhost", &Url::parse("http://localhost:3001").unwrap())
            .unwrap()
            .build()
            .unwrap(),
        limits: Limits::default(),
    }));
    let cookie = format!("staff={token}; staff-csrf={csrf}");
    let path = format!("/ban?board={board}&target={post}");
    assert_eq!(request(&app, &path, None, &cookie).await.0, StatusCode::OK);
    let form = format!(
        "board={board}&target={post}&csrf={csrf}&scope=local&kind=finite&seconds=3600&public_reason=Synthetic+public&private_reason=Synthetic+private"
    );
    assert_eq!(
        request(&app, "/ban", Some(&form), "").await.0,
        StatusCode::UNAUTHORIZED
    );
    let bad = form.replace(&format!("csrf={csrf}"), "csrf=untrusted");
    assert_eq!(
        request(&app, "/ban", Some(&bad), &cookie).await.0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("UPDATE staff_identity.accounts SET role='janitor' WHERE id=$1")
        .bind(account)
        .execute(&owner)
        .await
        .unwrap();
    assert_eq!(
        request(&app, "/ban", Some(&form), &cookie).await.0,
        StatusCode::FORBIDDEN
    );
    sqlx::query(
        "UPDATE staff_identity.accounts SET role='moderator',deny_boards=ARRAY[$2] WHERE id=$1",
    )
    .bind(account)
    .bind(&board)
    .execute(&owner)
    .await
    .unwrap();
    assert_eq!(
        request(&app, "/ban", Some(&form), &cookie).await.0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("UPDATE staff_identity.accounts SET deny_boards='{}' WHERE id=$1")
        .bind(account)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("UPDATE staff_identity.sessions SET authenticated_at=now()-interval '11 minutes' WHERE account_id=$1").bind(account).execute(&owner).await.unwrap();
    assert_eq!(
        request(&app, "/ban", Some(&form), &cookie).await.0,
        StatusCode::FORBIDDEN
    );
    sqlx::query("UPDATE staff_identity.sessions SET authenticated_at=now(),last_activity_at=now()-interval '61 seconds' WHERE account_id=$1").bind(account).execute(&owner).await.unwrap();
    assert_eq!(
        request(&app, "/ban", Some(&form), &cookie).await.0,
        StatusCode::UNAUTHORIZED
    );
    sqlx::query("UPDATE staff_identity.sessions SET last_activity_at=now() WHERE account_id=$1")
        .bind(account)
        .execute(&owner)
        .await
        .unwrap();
    let (code, headers, _) = request(&app, "/ban", Some(&form), &cookie).await;
    assert_eq!(code, StatusCode::SEE_OTHER);
    let id: i64 = headers["location"]
        .to_str()
        .unwrap()
        .split("restriction=")
        .nth(1)
        .unwrap()
        .parse()
        .unwrap();
    let unban = format!("board={board}&target={post}&csrf={csrf}&id={id}");
    sqlx::query("UPDATE staff_identity.accounts SET revoked_at=now() WHERE id=$1")
        .bind(account)
        .execute(&owner)
        .await
        .unwrap();
    assert_eq!(
        request(&app, "/ban/unban", Some(&unban), &cookie).await.0,
        StatusCode::UNAUTHORIZED
    );
    sqlx::query("UPDATE staff_identity.accounts SET revoked_at=NULL WHERE id=$1")
        .bind(account)
        .execute(&owner)
        .await
        .unwrap();
    assert_eq!(
        request(&app, "/ban/unban", Some(&unban), &cookie).await.0,
        StatusCode::SEE_OTHER
    );
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let actual:(bool,String,i64)=sqlx::query_as("SELECT active,private_reason,(SELECT count(*) FROM restrictions.audit WHERE ban_id=$1) FROM restrictions.bans WHERE id=$1").bind(id).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(actual, (false, "Synthetic private".into(), 2));
    sqlx::query("UPDATE restrictions.capacity SET used_bytes=used_bytes-(SELECT charged_bytes FROM restrictions.bans WHERE id=$1) WHERE singleton").bind(id).execute(&mut *tx).await.unwrap();
    sqlx::query("DELETE FROM restrictions.audit WHERE ban_id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM restrictions.bans WHERE id=$1")
        .bind(id)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("RESET ROLE").execute(&mut *tx).await.unwrap();
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
    for sql in [
        "DELETE FROM staff_identity.sessions WHERE account_id=$1",
        "DELETE FROM staff_identity.credentials WHERE account_id=$1",
        "DELETE FROM staff_identity.accounts WHERE id=$1",
    ] {
        sqlx::query(sql)
            .bind(account)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();
}
