use super::*;
use axum::body::to_bytes;
use board_domain::{
    challenge_secrets::{ReceiptToken, TokenKind, VerificationKey, VerificationToken},
    verification_mail::MailKey,
};
use board_store::verification::{VerificationStore, mail::MailStore};
use std::sync::Arc;

fn cookies(response: &Response) -> HeaderMap {
    let mut headers = HeaderMap::new();
    for value in response.headers().get_all(header::SET_COOKIE) {
        let value = value.to_str().unwrap();
        if !value.contains("Max-Age=0;") {
            headers.append(
                header::COOKIE,
                value.split(';').next().unwrap().parse().unwrap(),
            );
        }
    }
    headers.insert(
        header::CONTENT_TYPE,
        "application/x-www-form-urlencoded".parse().unwrap(),
    );
    headers
}
async fn body(response: Response) -> String {
    String::from_utf8(
        to_bytes(response.into_body(), 262144)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}
fn encoded(pairs: &[(&str, &str)]) -> Bytes {
    Bytes::from(
        url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(pairs.iter().copied())
            .finish(),
    )
}
async fn proof(
    state: &AppState,
    headers: &HeaderMap,
    nonce: &str,
    token: &str,
    key: &VerificationKey,
) -> (String, String) {
    let mut pairs = vec![("csrf", nonce)];
    if !token.is_empty() {
        pairs.push(("tkn", token));
    }
    let response = challenge(
        State(state.clone()),
        Extension(RequestPeer(Some("192.0.2.91".parse().unwrap()))),
        headers.clone(),
        encoded(&pairs),
    )
    .await
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&body(response).await).unwrap();
    assert_eq!(value["state"], "ready");
    let token = value["token"].as_str().unwrap().to_owned();
    let parsed = VerificationToken::parse(TokenKind::Challenge, &token).unwrap();
    (
        token,
        key.challenge_answer(&parsed, 6)
            .unwrap()
            .as_str()
            .to_owned(),
    )
}
#[tokio::test]
#[ignore = "Run explicitly with isolated verifier and mailer test credentials."]
async fn public_form_to_private_queue_and_browser_rotation_round_trip() {
    let owner = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = sqlx::PgPool::connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let issuer =
        VerificationStore::connect(&std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
    let mailer = MailStore::connect(&std::env::var("TEST_VERIFICATION_MAIL_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let key_material = ReceiptToken::generate().unwrap().encode();
    let mail_material = ReceiptToken::generate().unwrap().encode();
    let key = VerificationKey::parse(&key_material[3..]).unwrap();
    let mail_key = MailKey::parse(&mail_material[3..]).unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = listener.local_addr().unwrap();
    let service = board_verification::router(
        board_verification::AppState::new(
            issuer.clone(),
            VerificationKey::parse(&key_material[3..]).unwrap(),
            MailKey::parse(&mail_material[3..]).unwrap(),
            &"11".repeat(32),
        )
        .unwrap(),
    );
    let serving = tokio::spawn(async move {
        axum::serve(listener, service).await.unwrap();
    });
    let state = AppState {
        asn_database: None,
        browser_hint_key: None,
        posting_browser_compat_key: None,
        pool: public.clone(),
        origin: "http://127.0.0.1:3000".into(),
        production: false,
        limits: Arc::new(crate::security::Limits::new(
            board_config::PublicRequestLimits::default(),
        )),
        media: None,
        verification: Some(VerificationClient::new(endpoint, "11".repeat(32)).unwrap()),
        proxy_uid: None,
        poster_id_key: None,
        tripcode_key: None,
        country_database: None,
    };
    let peer = "192.0.2.91".parse().unwrap();
    let peer_hash = key.network_identity(peer);
    let email =
        board_domain::email_verification::PreparedEmail::prepare("owned.public.fixture@gmail.com")
            .unwrap();
    let identity = key.email_identity(&email);
    let outcome=tokio::spawn({let owner=owner.clone();let issuer=issuer.clone();let mailer=mailer.clone();async move {
        let response=page(State(state.clone()),Extension(RequestPeer(Some(peer))),RawQuery(None),HeaderMap::new()).await.unwrap();
        let headers=cookies(&response);let secret=FormSecret::read(&headers,false).unwrap().unwrap();let nonce=secret.nonce();
        let previous=Session::resolve(&state,&headers,Some(peer)).await.unwrap();assert!(!previous.posting.minted);
        assert!(body(response).await.contains("name=\"email\""));
        let rejected=submit(State(state.clone()),Extension(RequestPeer(Some(peer))),headers.clone(),encoded(&[("action","request"),("csrf","invalid"),("email","owned.public.fixture@gmail.com")])).await;
        assert_eq!(rejected.err().unwrap().0,StatusCode::FORBIDDEN);
        let (challenge,answer)=proof(&state,&headers,&nonce,"",&key).await;
        let response=submit(State(state.clone()),Extension(RequestPeer(Some(peer))),headers.clone(),encoded(&[("action","request"),("csrf",&nonce),("email","owned.public.fixture@gmail.com"),("t-challenge",&challenge),("t-response",&answer)])).await.unwrap();
        assert!(body(response).await.contains("will be sent out shortly"));
        let lease=ReceiptToken::generate().unwrap().storage_hash();let jobs=mailer.claim(&lease,100).await.unwrap();assert_eq!(jobs.len(),1);
        let job=&jobs[0];let payload=mail_key.open(job.token_hash.as_slice().try_into().unwrap(),&job.encrypted_payload).unwrap();
        assert!(mailer.finish(&job.request_id,&lease,true).await.unwrap());
        let query=format!("action=verify&tkn={}",payload.token());
        // External mail navigation withholds SameSite=Strict cookies. The link
        // page must leave the existing anonymous capability untouched.
        let response=page(State(state.clone()),Extension(RequestPeer(Some(peer))),RawQuery(Some(query)),HeaderMap::new()).await.unwrap();
        assert!(response.headers().get_all(header::SET_COOKIE).iter().all(|v|!v.to_str().unwrap().starts_with("board-anon=")));
        let mut return_headers=cookies(&response);
        for cookie in headers.get_all(header::COOKIE) {if cookie.to_str().unwrap().starts_with("board-anon="){return_headers.append(header::COOKIE,cookie.clone());}}
        let nonce=FormSecret::read(&return_headers,false).unwrap().unwrap().nonce();
        assert!(body(response).await.contains("Continue verification"));
        let response=submit(State(state.clone()),Extension(RequestPeer(Some(peer))),return_headers,encoded(&[("action","preview"),("csrf",&nonce),("tkn",payload.token())])).await.unwrap();
        let headers=cookies(&response);let nonce=FormSecret::read(&headers,false).unwrap().unwrap().nonce();
        assert!(body(response).await.contains("name=\"tkn\""));
        let preserved=Session::resolve(&state,&headers,Some(peer)).await.unwrap();assert_eq!(preserved.posting.fingerprints.token,previous.posting.fingerprints.token);
        assert!(matches!(state.verification.as_ref().unwrap().email_state(&protocol::EmailToken{token:payload.token().into()}).await.unwrap().state,EmailState::Pending),"GET must not consume the email token");
        sqlx::query("UPDATE verification_private.peers SET refresh_after=clock_timestamp()-interval '1 second' WHERE peer_hash=$1").bind(peer_hash.as_slice()).execute(&owner).await.unwrap();
        let (challenge,answer)=proof(&state,&headers,&nonce,payload.token(),&key).await;
        let response=submit(State(state.clone()),Extension(RequestPeer(Some(peer))),headers.clone(),encoded(&[("action","verify"),("csrf",&nonce),("tkn",payload.token()),("t-challenge",&challenge),("t-response",&answer)])).await.unwrap();
        let new_headers=cookies(&response);assert!(body(response).await.contains("This session is now verified."));
        let current=Session::resolve(&state,&new_headers,Some(peer)).await.unwrap();
        assert_ne!(current.posting.fingerprints.token,previous.posting.fingerprints.token);
        assert!(verified(&state,&current).await.unwrap());assert!(issuer.session(&previous.posting.fingerprints.token).await.unwrap().is_none());
        let response=page(State(state.clone()),Extension(RequestPeer(Some(peer))),RawQuery(None),new_headers).await.unwrap();let headers=cookies(&response);let nonce=FormSecret::read(&headers,false).unwrap().unwrap().nonce();
        assert!(body(response).await.contains(">Logout</button>"));
        let response=submit(State(state.clone()),Extension(RequestPeer(Some(peer))),headers,encoded(&[("action","signout"),("csrf",&nonce)])).await.unwrap();
        assert!(body(response).await.contains("Session cleared."));assert!(issuer.session(&current.posting.fingerprints.token).await.unwrap().is_none());
    }}).await;
    serving.abort();
    let _ = serving.await;
    // This test's unique private key scopes all retained queue/identity rows.
    sqlx::query("DELETE FROM verification_private.email_requests WHERE email_hash=$1")
        .bind(identity.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.identities WHERE email_hash=$1")
        .bind(identity.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.peers WHERE peer_hash=$1")
        .bind(peer_hash.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    issuer.close().await;
    mailer.close().await;
    public.close().await;
    owner.close().await;
    outcome.unwrap();
}
