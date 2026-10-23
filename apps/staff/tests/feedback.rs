#![cfg(feature = "database-tests")]
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use board_staff::{AppState, Config, Limits, auth, router};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use tower::ServiceExt;
use webauthn_rs::prelude::*;
#[path = "feedback/browser.rs"]
mod browser;
fn request(path: &str, cookie: &str, fields: Option<&str>) -> Request<Body> {
    Request::builder()
        .method(if fields.is_some() { "POST" } else { "GET" })
        .uri(path)
        .header("cookie", cookie)
        .header("origin", "http://localhost:3001")
        .header("sec-fetch-site", "same-origin")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(fields.unwrap_or("").to_owned()))
        .unwrap()
}
async fn body(response: axum::response::Response) -> String {
    String::from_utf8(
        to_bytes(response.into_body(), 4_194_304)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}
#[tokio::test]
async fn feedback_pages_keep_source_controls_escaped_private_and_authorized() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let auth_pool = PgPool::connect(&std::env::var("AUTH_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let staff_pool = PgPool::connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut lock = owner.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(2250258)")
        .execute(&mut *lock)
        .await
        .unwrap();
    let origin = Url::parse("http://localhost:3001").unwrap();
    let state = Arc::new(AppState {
        config: Config {
            asn_database: None,
            browser_hint_key: None,
            media: None,
            proxy: None,
            poster_id_key: Some(Arc::new(
                board_domain::poster_id::PosterIdKey::parse(&"44".repeat(32)).unwrap(),
            )),
            country_database: None,
            origin: origin.to_string().trim_end_matches('/').into(),
            public_origin: "http://localhost:3000".into(),
            media_origin: "http://127.0.0.1:3002".into(),
            bind: "127.0.0.1:3001".parse().unwrap(),
            production: false,
            auth_database: String::new(),
            staff_database: String::new(),
            idle_timeout: Duration::from_secs(600),
            tripcode_key: None,
        },
        auth: auth_pool.clone(),
        staff: staff_pool.clone(),
        webauthn: WebauthnBuilder::new("localhost", &origin)
            .unwrap()
            .build()
            .unwrap(),
        limits: Limits::default(),
    });
    let profile: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&owner)
        .await
        .unwrap();
    sqlx::query("INSERT INTO submission_private.feedback_profiles(id,subject_limit,message_limit,capacity) VALUES($1,150,2000,100)").bind(profile).execute(&owner).await.unwrap();
    sqlx::query("INSERT INTO submission_private.feedback_categories VALUES($1,1,0,'Owned category','Owned')").bind(profile).execute(&owner).await.unwrap();
    let mut ids = Vec::new();
    for status in [0i16, 3] {
        let id:i64=sqlx::query_scalar("INSERT INTO submission_private.feedback(profile_id,category,subject,message,state,actor_hash,peer,user_agent,language,created_at,updated_at,reply,reply_capcode) VALUES($1,1,'<script>subject</script>','Owned literal <body>',$2,decode(repeat('ab',32),'hex'),'192.0.2.225','<Owned agent>','en',100,100,$3,$4) RETURNING id")
            .bind(profile).bind(status).bind((status==3).then_some("Owned reply")).bind((status==3).then_some("mod")).fetch_one(&owner).await.unwrap();
        ids.push(id);
    }
    let token = auth::token();
    let csrf = auth::token();
    let credential = uuid::Uuid::new_v4().as_bytes().to_vec();
    let account: i64 = sqlx::query_scalar(
        "INSERT INTO staff_identity.accounts(role) VALUES('moderator') RETURNING id",
    )
    .fetch_one(&owner)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO staff_identity.credentials(id,account_id,credential) VALUES($1,$2,'{}')",
    )
    .bind(&credential)
    .bind(account)
    .execute(&owner)
    .await
    .unwrap();
    sqlx::query("INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id) VALUES($1,$2,$3,$4)").bind(auth::hash(&token)).bind(auth::hash(&csrf)).bind(account).bind(&credential).execute(&owner).await.unwrap();
    let result=tokio::spawn({let owner=owner.clone();async move {
        let app=router(state);let cookie=format!("staff={token}; staff-csrf={csrf}");let path=format!("/feedback?profile={profile}");
        assert_eq!(app.clone().oneshot(request(&path,"",None)).await.unwrap().status(),StatusCode::UNAUTHORIZED);
        assert_eq!(app.clone().oneshot(request(&path,&cookie,None)).await.unwrap().status(),StatusCode::FORBIDDEN);
        sqlx::query("UPDATE staff_identity.accounts SET flags=ARRAY['feedback_review'] WHERE id=$1").bind(account).execute(&owner).await.unwrap();
        let response=app.clone().oneshot(request(&path,&cookie,None)).await.unwrap();assert_eq!(response.status(),StatusCode::OK);assert_eq!(response.headers()["cache-control"],"private, no-store");
        let page=body(response).await;assert!(page.contains("&#60;script&#62;subject&#60;/script&#62;"));assert!(!page.contains("<script>subject</script>"));assert!(!page.contains("value=\"dismiss\""));assert!(!page.contains(&token));
        let read=body(app.clone().oneshot(request(&format!("/feedback?action=read&id={}",ids[0]),&cookie,None)).await.unwrap()).await;
        assert!(read.contains("192.0.2.225"));assert!(read.contains("&#60;Owned agent&#62;"));assert!(!read.contains("<textarea"));assert!(!read.contains("Search (pwd)"));
        let drafts=format!("/feedback?profile={profile}&answered=3");
        let page=body(app.clone().oneshot(request(&drafts,&cookie,None)).await.unwrap()).await;assert!(page.contains("Owned reply"));assert!(!page.contains("value=\"approve\""));
        sqlx::query("UPDATE staff_identity.accounts SET role='admin' WHERE id=$1").bind(account).execute(&owner).await.unwrap();
        let page=body(app.clone().oneshot(request(&drafts,&cookie,None)).await.unwrap()).await;assert!(page.contains("value=\"approve\""));assert!(page.contains("value=\"dismiss\""));
        let fields=url::form_urlencoded::Serializer::new(String::new()).append_pair("action","approve").append_pair("id",&ids[1].to_string()).append_pair("profile",&profile.to_string()).append_pair("answered","3").append_pair("csrf",&csrf).finish();
        assert_eq!(app.clone().oneshot(request("/feedback",&cookie,Some(&fields.replace(&csrf,"wrong")))).await.unwrap().status(),StatusCode::FORBIDDEN);
        let mut cross=request("/feedback",&cookie,Some(&fields));cross.headers_mut().insert("origin","https://other.invalid".parse().unwrap());assert_eq!(app.clone().oneshot(cross).await.unwrap().status(),StatusCode::FORBIDDEN);
        let response=app.clone().oneshot(request("/feedback",&cookie,Some(&fields))).await.unwrap();assert_eq!(response.status(),StatusCode::SEE_OTHER);assert_eq!(response.headers()["location"],format!("/feedback?action=review&profile={profile}&answered=3"));
        assert_eq!(app.clone().oneshot(request("/feedback",&cookie,Some(&fields))).await.unwrap().status(),StatusCode::NOT_FOUND);
        assert_eq!(app.clone().oneshot(request("/feedback?answered=0",&cookie,None)).await.unwrap().status(),StatusCode::BAD_REQUEST);
        let ready=app.clone().oneshot(request("/readyz","",None)).await.unwrap();let status=ready.status();let message=body(ready).await;assert_eq!(status,StatusCode::OK,"{message}");
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM submission_private.feedback_review_log WHERE feedback_id=ANY($1)").bind(&ids).fetch_one(&owner).await.unwrap();assert_eq!(count,1);
        if std::env::var("RUN_STAFF_FEEDBACK_BROWSER").as_deref()==Ok("1") {browser::run(app,&owner,profile,&cookie).await;}

    }}).await;
    sqlx::query("DELETE FROM submission_private.feedback_review_log WHERE feedback_id IN(SELECT id FROM submission_private.feedback WHERE profile_id=$1)").bind(profile).execute(&owner).await.unwrap();
    for query in [
        "DELETE FROM submission_private.feedback WHERE profile_id=$1",
        "DELETE FROM submission_private.feedback_categories WHERE profile_id=$1",
        "DELETE FROM submission_private.feedback_profiles WHERE id=$1",
    ] {
        sqlx::query(query)
            .bind(profile)
            .execute(&owner)
            .await
            .unwrap();
    }
    for query in [
        "DELETE FROM staff_identity.sessions WHERE account_id=$1",
        "DELETE FROM staff_identity.credentials WHERE account_id=$1",
        "DELETE FROM staff_identity.accounts WHERE id=$1",
    ] {
        sqlx::query(query)
            .bind(account)
            .execute(&owner)
            .await
            .unwrap();
    }
    sqlx::query("SELECT pg_advisory_unlock(2250258)")
        .execute(&mut *lock)
        .await
        .unwrap();
    drop(lock);
    for pool in [owner, auth_pool, staff_pool] {
        pool.close().await;
    }
    result.unwrap();
}
