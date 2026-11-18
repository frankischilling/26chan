#![cfg(feature = "database-tests")]
use axum::{
    body::{Body, to_bytes},
    extract::ConnectInfo,
    http::{Request, StatusCode, header},
};
use board_domain::{
    anonymous_session::Capability,
    challenge_secrets::{ReceiptToken, TokenKind, VerificationKey, VerificationToken},
    verification_mail::MailKey,
};
use sqlx::PgPool;
use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
};
use tower::ServiceExt;
#[path = "site_applications/browser.rs"]
mod browser;
const ORIGIN: &str = "http://localhost:3000";
fn request(path: &str, fields: Option<&[(String, String)]>, cookie: &str) -> Request<Body> {
    let mut encoded = url::form_urlencoded::Serializer::new(String::new());
    if let Some(fields) = fields {
        for (name, value) in fields {
            encoded.append_pair(name, value);
        }
    }
    Request::builder()
        .method(if fields.is_some() { "POST" } else { "GET" })
        .uri(path)
        .header("origin", ORIGIN)
        .header(header::COOKIE, cookie)
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .extension(ConnectInfo(
            "192.0.2.225:1234".parse::<SocketAddr>().unwrap(),
        ))
        .body(Body::from(encoded.finish()))
        .unwrap()
}
async fn reset(owner: &PgPool, peer: &[u8; 32]) {
    sqlx::query("UPDATE verification_private.peers SET refresh_after=clock_timestamp()-interval '1 second' WHERE peer_hash=$1").bind(peer.as_slice()).execute(owner).await.unwrap();
}
async fn answer(owner: &PgPool, key: &VerificationKey, page: &str) -> String {
    let token = VerificationToken::parse(TokenKind::Challenge, &field(page, "challenge")).unwrap();
    let suspicious: bool = sqlx::query_scalar(
        "SELECT suspicious FROM verification_private.challenges WHERE token_hash=$1",
    )
    .bind(key.token_hash(&token).as_slice())
    .fetch_one(owner)
    .await
    .unwrap();
    key.challenge_answer(&token, if suspicious { 8 } else { 6 })
        .unwrap()
        .as_str()
        .to_owned()
}
fn values(csrf: &str, campaign: i64) -> Vec<(String, String)> {
    [
        ("firstname", "Owned <name>"),
        ("handle", "<script>literal</script>"),
        ("email", "owned+application@example.org"),
        ("age", "25"),
        ("tz", "-4"),
        ("hours", "3"),
        ("times", "16:00-19:00"),
        ("board1", "g"),
        ("board2", ""),
        ("q1", "<b>literal answer</b>\r\nsecond"),
        ("q2", "Two"),
        ("q3", "Three"),
        ("q4", "Four"),
        ("action", "submit"),
        ("_cf_fuid", csrf),
        ("campaign", &campaign.to_string()),
    ]
    .into_iter()
    .map(|(k, v)| (k.into(), v.into()))
    .collect()
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
fn field(page: &str, name: &str) -> String {
    page.split(&format!("name=\"{name}\" value=\""))
        .nth(1)
        .unwrap()
        .split('"')
        .next()
        .unwrap()
        .into()
}
#[tokio::test]
#[ignore = "Requires isolated verifier credentials for the owned application form."]
async fn application_forms_preserve_private_ownership_and_captcha_refresh() {
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
    let mut lock = owner.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(2250258)")
        .execute(&mut *lock)
        .await
        .unwrap();
    let id: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&owner)
        .await
        .unwrap();
    let material = ReceiptToken::generate().unwrap().encode();
    let key = VerificationKey::parse(&material[3..]).unwrap();
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
    let web = board_public::routers_with_events(
        public.clone(),
        board_public::PublicRouterOptions {
            origin: ORIGIN.into(),
            production: false,
            media: None,
            limits: Default::default(),
            proxy_uid: None,
            poster_id_key: None,
            tripcode_key: None,
            country_database: None,
        },
        board_config::PublicVerificationSettings::parse(
            Some(&endpoint.to_string()),
            Some(&"11".repeat(32)),
        )
        .unwrap(),
        Default::default(),
        false,
    )
    .0;
    let sessions = Arc::new(Mutex::new(Vec::<[u8; 32]>::new()));
    let peer = key.network_identity("192.0.2.225".parse().unwrap());
    let work=tokio::spawn({let owner=owner.clone();let public=public.clone();let sessions=sessions.clone();async move {
        let closed=web.clone().oneshot(request("/janitorapp",None,"")).await.unwrap();
        assert_eq!(closed.status(),StatusCode::OK);assert!(!closed.headers().contains_key(header::SET_COOKIE));
        assert!(body(closed).await.contains("We are not accepting applications this time."));
        sqlx::query("INSERT INTO submission_private.application_campaigns(id,enabled,starts_at,ends_at,boards,questions,field_limit,answer_limit,capacity,cooldown_seconds) VALUES($1,true,1,253402300799,ARRAY['g'],ARRAY['<Owned question>','Two','Three','Four'],150,2000,100,0)").bind(id).execute(&owner).await.unwrap();
        let mut mobile=request("/janitorapp",None,"");mobile.headers_mut().insert(header::USER_AGENT,"Owned Android Mobile".parse().unwrap());
        let mobile=web.clone().oneshot(mobile).await.unwrap();assert_eq!(mobile.status(),StatusCode::OK);assert!(!mobile.headers().contains_key(header::SET_COOKIE));assert!(body(mobile).await.contains("Please use a desktop browser"));
        let response=web.clone().oneshot(request("/janitorapp",None,"")).await.unwrap();assert_eq!(response.status(),StatusCode::OK);
        assert_eq!(response.headers()["referrer-policy"],"strict-origin");
        let policy=response.headers()[header::CONTENT_SECURITY_POLICY].to_str().unwrap();assert!(policy.contains("application-form.v1.js"));assert!(policy.contains("data:"));
        let cookie=response.headers().get_all(header::SET_COOKIE).iter().map(|h|h.to_str().unwrap().split(';').next().unwrap()).collect::<Vec<_>>().join("; ");
        let cap=Capability::parse(cookie.split("; ").find_map(|v|v.strip_prefix("board-anon=")).unwrap()).unwrap();sessions.lock().unwrap().push(cap.storage_hash());
        let page=body(response).await;assert!(page.contains("&#60;Owned question&#62;"));assert!(page.contains("id=\"application-form\""));
        let csrf=field(&page,"_cf_fuid");let mut fields=values(&csrf,id);
        fields.push(("challenge".into(),field(&page,"challenge")));fields.push(("answer".into(),answer(&owner,&key,&page).await));
        let mut bad=fields.clone();bad.iter_mut().find(|(k,_)|k=="_cf_fuid").unwrap().1="bad".into();assert_eq!(web.clone().oneshot(request("/janitorapp",Some(&bad),&cookie)).await.unwrap().status(),StatusCode::FORBIDDEN);
        let mut cross=request("/janitorapp",Some(&fields),&cookie);cross.headers_mut().insert("origin","https://other.invalid".parse().unwrap());assert_eq!(web.clone().oneshot(cross).await.unwrap().status(),StatusCode::FORBIDDEN);
        let result=web.clone().oneshot(request("/janitorapp",Some(&fields),&cookie)).await.unwrap();let status=result.status();assert_eq!(result.headers()[header::CACHE_CONTROL],"private, no-store");let success=body(result).await;assert_eq!(status,StatusCode::OK,"{success}");assert!(success.contains("Application submitted."));
        let private_path=format!("/janitorapp?id={}",success.split("href=\"/janitorapp?id=").nth(1).unwrap().split('"').next().unwrap());assert!(private_path.starts_with("/janitorapp?id=app1."));
        assert_eq!(web.clone().oneshot(request("/janitorapp",Some(&fields),&cookie)).await.unwrap().status(),StatusCode::BAD_REQUEST);
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM submission_private.applications WHERE campaign_id=$1").bind(id).fetch_one(&owner).await.unwrap();assert_eq!(count,1);
        reset(&owner,&peer).await;
        let auth=web.clone().oneshot(request(&private_path,None,&cookie)).await.unwrap();assert_eq!(auth.status(),StatusCode::OK);let auth=body(auth).await;
        assert!(auth.contains("To edit this application"));assert!(!auth.contains("Owned &#60;name&#62;"));assert!(!auth.contains("owned+application@example.org"));
        let auth_fields=vec![("auth_email".into(),"OWNED+APPLICATION@EXAMPLE.ORG".into()),("_cf_fuid".into(),field(&auth,"_cf_fuid")),("challenge".into(),field(&auth,"challenge")),("answer".into(),answer(&owner,&key,&auth).await)];
        // Force an issuer cooldown after successful auth; the edit fields must survive it.
        sqlx::query("UPDATE verification_private.peers SET refresh_after=clock_timestamp()+interval '30 seconds' WHERE peer_hash=$1").bind(peer.as_slice()).execute(&owner).await.unwrap();
        let edit=web.clone().oneshot(request(&private_path,Some(&auth_fields),&cookie)).await.unwrap();let status=edit.status();let edit=body(edit).await;assert_eq!(status,StatusCode::OK,"{edit}");
        assert!(edit.contains("Owned &#60;name&#62;"));assert!(edit.contains("data-edit"));assert!(edit.contains("before refreshing verification"));assert!(!edit.contains("<script>literal</script>"));
        let mut edits=values(&csrf,id);edits.iter_mut().find(|(k,_)|k=="action").unwrap().1="challenge".into();
        edits.extend([("id".into(),field(&edit,"id")),("auth_email".into(),"owned+application@example.org".into()),("revision".into(),field(&edit,"revision")),("challenge".into(),String::new()),("answer".into(),String::new())]);
        reset(&owner,&peer).await;
        let refreshed=web.clone().oneshot(request("/janitorapp",Some(&edits),&cookie)).await.unwrap();let status=refreshed.status();let refreshed=body(refreshed).await;assert_eq!(status,StatusCode::OK,"{refreshed}");
        edits.iter_mut().find(|(k,_)|k=="action").unwrap().1="submit".into();edits.iter_mut().find(|(k,_)|k=="challenge").unwrap().1=field(&refreshed,"challenge");edits.iter_mut().find(|(k,_)|k=="answer").unwrap().1=answer(&owner,&key,&refreshed).await;edits.iter_mut().find(|(k,_)|k=="email").unwrap().1="changed@example.org".into();
        let saved=web.clone().oneshot(request("/janitorapp",Some(&edits),&cookie)).await.unwrap();let status=saved.status();let saved=body(saved).await;assert_eq!(status,StatusCode::OK,"{saved}");assert!(saved.contains(&private_path));
        let record:(i32,String,String)=sqlx::query_as("SELECT a.revision,i.email,a.answers[1] FROM submission_private.applications a JOIN submission_private.application_identity i ON i.application_id=a.id WHERE a.campaign_id=$1").bind(id).fetch_one(&owner).await.unwrap();assert_eq!(record,(2,"changed@example.org".into(),"<b>literal answer</b>\nsecond".into()));
        assert_eq!(web.clone().oneshot(request("/janitorapp",Some(&edits),&cookie)).await.unwrap().status(),StatusCode::BAD_REQUEST);
        if std::env::var("RUN_APPLICATION_BROWSER").as_deref()==Ok("1") {
            browser::run(browser::Fixture{owner:&owner,public:&public,endpoint,key:&key,profile:id,sessions}).await;
        }
    }}).await;
    server.abort();
    let _ = server.await;
    for query in [
        "DELETE FROM submission_private.applications WHERE campaign_id=$1",
        "DELETE FROM submission_private.application_campaigns WHERE id=$1",
    ] {
        sqlx::query(query).bind(id).execute(&owner).await.unwrap();
    }
    let tokens = sessions.lock().unwrap().clone();
    for token in tokens {
        sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
            .bind(token.as_slice())
            .execute(&owner)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM verification_private.peers WHERE peer_hash=$1")
        .bind(peer.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("SELECT pg_advisory_unlock(2250258)")
        .execute(&mut *lock)
        .await
        .unwrap();
    drop(lock);
    issuer.close().await;
    public.close().await;
    owner.close().await;
    work.unwrap();
}
