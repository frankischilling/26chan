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
#[path = "applications/browser.rs"]
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
async fn application_pages_keep_initial_review_blind_and_selection_explicit() {
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

    let campaign: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&owner)
        .await
        .unwrap();
    sqlx::query("INSERT INTO submission_private.application_campaigns(id,starts_at,ends_at,boards,questions,field_limit,answer_limit,capacity) VALUES($1,1,253402300799,ARRAY['g'],ARRAY['One','Two','Three','Four'],150,2000,100)").bind(campaign).execute(&owner).await.unwrap();
    let id:i64=sqlx::query_scalar("INSERT INTO submission_private.applications(campaign_id,age,timezone,hours,times,board1,answers,created_at,updated_at) VALUES($1,21,-4,3,'Evenings','g',ARRAY['<script>answer</script>','Two','Three','Four'],1,1) RETURNING id").bind(campaign).fetch_one(&owner).await.unwrap();
    sqlx::query("INSERT INTO submission_private.application_identity(application_id,campaign_id,ownership_hash,actor_hash,firstname,handle,email,email_identity) VALUES($1,$2,sha256(convert_to(gen_random_uuid()::text,'UTF8')),sha256(convert_to(gen_random_uuid()::text,'UTF8')),'Private <name>','Private handle','owned@example.invalid','owned@example.invalid')").bind(id).bind(campaign).execute(&owner).await.unwrap();
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
        let app=router(state);let cookie=format!("staff={token}; staff-csrf={csrf}");
        let path=format!("/applications?campaign={campaign}");let detail=format!("/applications?id={id}");
        assert_eq!(app.clone().oneshot(request(&path,"",None)).await.unwrap().status(),StatusCode::UNAUTHORIZED);
        assert_eq!(app.clone().oneshot(request(&path,&cookie,None)).await.unwrap().status(),StatusCode::FORBIDDEN);
        sqlx::query("UPDATE staff_identity.accounts SET flags=ARRAY['application_review'] WHERE id=$1").bind(account).execute(&owner).await.unwrap();
        let response=app.clone().oneshot(request(&path,&cookie,None)).await.unwrap();
        assert_eq!(response.status(),StatusCode::OK);assert_eq!(response.headers()["cache-control"],"private, no-store");
        assert_eq!(response.headers()["referrer-policy"],"same-origin");
        let page=body(response).await;assert!(page.contains(&detail.replace("&","&amp;")));assert!(!page.contains("owned@example.invalid"));assert!(!page.contains(&token));
        let page=body(app.clone().oneshot(request(&detail,&cookie,None)).await.unwrap()).await;
        assert!(page.contains("&#60;script&#62;answer&#60;/script&#62;"));
        assert!(!page.contains("<script>answer</script>"));assert!(!page.contains("value=\"accept\""));
        assert!(!page.contains("Private"));assert!(!page.contains("owned@example.invalid"));
        let contact=format!("{detail}&identity=true");
        assert_eq!(app.clone().oneshot(request(&contact,&cookie,None)).await.unwrap().status(),StatusCode::FORBIDDEN);
        sqlx::query("UPDATE staff_identity.accounts SET role='admin' WHERE id=$1").bind(account).execute(&owner).await.unwrap();
        assert_eq!(app.clone().oneshot(request(&contact,&cookie,None)).await.unwrap().status(),StatusCode::NOT_FOUND);
        let page=body(app.clone().oneshot(request(&detail,&cookie,None)).await.unwrap()).await;
        assert!(page.contains("value=\"accept\""));assert!(page.contains("value=\"decline\""));assert!(!page.contains("owned@example.invalid"));
        let fields=url::form_urlencoded::Serializer::new(String::new()).append_pair("action","accept").append_pair("id",&id.to_string()).append_pair("revision","1").append_pair("csrf",&csrf).finish();
        assert_eq!(app.clone().oneshot(request("/applications",&cookie,Some(&fields.replace(&csrf,"wrong")))).await.unwrap().status(),StatusCode::FORBIDDEN);
        let mut cross=request("/applications",&cookie,Some(&fields));cross.headers_mut().insert("origin","https://foreign.invalid".parse().unwrap());
        assert_eq!(app.clone().oneshot(cross).await.unwrap().status(),StatusCode::FORBIDDEN);
        let response=app.clone().oneshot(request("/applications",&cookie,Some(&fields))).await.unwrap();
        assert_eq!(response.status(),StatusCode::SEE_OTHER);assert_eq!(response.headers()["location"],detail);
        assert_eq!(app.clone().oneshot(request("/applications",&cookie,Some(&fields))).await.unwrap().status(),StatusCode::NOT_FOUND);
        let page=body(app.clone().oneshot(request(&detail,&cookie,None)).await.unwrap()).await;
        assert!(!page.contains("owned@example.invalid"));assert!(!page.contains("value=\"accept\""));
        let response=app.clone().oneshot(request(&contact,&cookie,None)).await.unwrap();
        assert_eq!(response.status(),StatusCode::OK);assert_eq!(response.headers()["cache-control"],"private, no-store");
        let page=body(response).await;assert!(page.contains("owned@example.invalid"));assert!(page.contains("Private &#60;name&#62;"));assert!(!page.contains("Private <name>"));
        sqlx::query("UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp()-interval '11 minutes' WHERE token_hash=$1").bind(auth::hash(&token)).execute(&owner).await.unwrap();
        assert_ne!(app.clone().oneshot(request(&contact,&cookie,None)).await.unwrap().status(),StatusCode::OK);
        assert_eq!(app.clone().oneshot(request("/readyz","",None)).await.unwrap().status(),StatusCode::OK);
        sqlx::query("UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp() WHERE token_hash=$1").bind(auth::hash(&token)).execute(&owner).await.unwrap();
        if std::env::var("RUN_STAFF_APPLICATION_BROWSER").as_deref()==Ok("1") {
            browser::run(app,&owner,campaign,&cookie).await;
        }

    }}).await;
    for query in [
        "DELETE FROM submission_private.application_review_log WHERE application_id IN(SELECT id FROM submission_private.applications WHERE campaign_id=$1)",
        "DELETE FROM submission_private.application_identity WHERE campaign_id=$1",
        "DELETE FROM submission_private.applications WHERE campaign_id=$1",
        "DELETE FROM submission_private.application_campaigns WHERE id=$1",
    ] {
        sqlx::query(query)
            .bind(campaign)
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
