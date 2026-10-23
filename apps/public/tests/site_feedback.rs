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
const ORIGIN: &str = "http://localhost:3000";
fn request(path: &str, fields: Option<&[(&str, &str)]>, cookie: &str) -> Request<Body> {
    let mut body = String::new();
    if let Some(fields) = fields {
        for (name, value) in fields {
            body.push_str(&format!(
                "--owned\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"
            ));
        }
        body.push_str("--owned--\r\n");
    }
    Request::builder()
        .method(if fields.is_some() { "POST" } else { "GET" })
        .uri(path)
        .header("origin", ORIGIN)
        .header(header::COOKIE, cookie)
        .header(header::CONTENT_TYPE, "multipart/form-data; boundary=owned")
        .extension(ConnectInfo(
            "192.0.2.225:1234".parse::<SocketAddr>().unwrap(),
        ))
        .body(Body::from(body))
        .unwrap()
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
#[ignore = "Requires isolated verifier credentials for the owned feedback form."]
async fn source_feedback_form_has_bound_single_use_writes_and_read_only_redirects() {
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
    let work=tokio::spawn({let owner=owner.clone();let sessions=sessions.clone();async move {
        let closed=web.clone().oneshot(request("/feedback",None,"")).await.unwrap();
        assert_eq!(closed.status(),StatusCode::OK);assert!(!closed.headers().contains_key(header::SET_COOKIE));
        assert!(!body(closed).await.contains("name=\"challenge\""));
        sqlx::query("INSERT INTO submission_private.feedback_profiles(id,enabled,subject_limit,message_limit,capacity,cooldown_seconds) VALUES($1,true,150,2000,100,0)").bind(id).execute(&owner).await.unwrap();
        sqlx::query("INSERT INTO submission_private.feedback_categories VALUES($1,1,0,'<Owned>','Owned')").bind(id).execute(&owner).await.unwrap();
        let response=web.clone().oneshot(request("/feedback",None,"")).await.unwrap();
        assert_eq!(response.status(),StatusCode::OK);
        assert!(response.headers()[header::CONTENT_SECURITY_POLICY].to_str().unwrap().contains("data:"));
        let cookie=response.headers().get_all(header::SET_COOKIE).iter().map(|h|h.to_str().unwrap().split(';').next().unwrap()).collect::<Vec<_>>().join("; ");
        let cap=Capability::parse(cookie.split("; ").find_map(|v|v.strip_prefix("board-anon=")).unwrap()).unwrap();
        sessions.lock().unwrap().push(cap.storage_hash());
        let page=body(response).await;
        assert!(page.contains("&#60;Owned&#62;"));assert!(page.contains("enctype=\"multipart/form-data\""));
        let csrf=field(&page,"csrf");let challenge=field(&page,"challenge");let profile=field(&page,"profile");
        let token=VerificationToken::parse(TokenKind::Challenge,&challenge).unwrap();
        let suspicious:bool=sqlx::query_scalar("SELECT suspicious FROM verification_private.challenges WHERE token_hash=$1").bind(key.token_hash(&token).as_slice()).fetch_one(&owner).await.unwrap();
        let answer=key.challenge_answer(&token,if suspicious{8}else{6}).unwrap();
        let mut fields=vec![("action","submit"),("category","1"),("subject","Owned <subject>"),("message","<script>literal</script>\r\nsecond"),("csrf",csrf.as_str()),("profile",profile.as_str()),("challenge",challenge.as_str()),("answer",answer.as_str())];
        fields[4].1="wrong";
        assert_eq!(web.clone().oneshot(request("/feedback",Some(&fields),&cookie)).await.unwrap().status(),StatusCode::FORBIDDEN);
        fields[4].1=&csrf;
        let mut cross=request("/feedback",Some(&fields),&cookie);cross.headers_mut().insert("origin","https://other.invalid".parse().unwrap());
        assert_eq!(web.clone().oneshot(cross).await.unwrap().status(),StatusCode::FORBIDDEN);
        let response=web.clone().oneshot(request("/feedback",Some(&fields),&cookie)).await.unwrap();
        let status=response.status();let location=response.headers().get(header::LOCATION).cloned();let text=body(response).await;
        assert_eq!(status,StatusCode::SEE_OTHER,"{text}");assert_eq!(location.unwrap(),"/feedback?sent=1");
        let replay=web.clone().oneshot(request("/feedback",Some(&fields),&cookie)).await.unwrap();assert_eq!(replay.status(),StatusCode::BAD_REQUEST);
        for _ in 0..2 {let done=web.clone().oneshot(request("/feedback?sent=1",None,&cookie)).await.unwrap();assert_eq!(done.status(),StatusCode::OK);assert!(!done.headers().contains_key(header::SET_COOKIE));}
        let rows:Vec<(String,String)>=sqlx::query_as("SELECT subject,message FROM submission_private.feedback WHERE profile_id=$1").bind(id).fetch_all(&owner).await.unwrap();
        assert_eq!(rows,vec![("Owned <subject>".into(),"<script>literal</script>\nsecond".into())]);
    }}).await;
    server.abort();
    let _ = server.await;
    for query in [
        "DELETE FROM submission_private.feedback WHERE profile_id=$1",
        "DELETE FROM submission_private.feedback_categories WHERE profile_id=$1",
        "DELETE FROM submission_private.feedback_profiles WHERE id=$1",
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
