#![cfg(feature = "database-tests")]
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use board_domain::{
    anonymous_session::Capability,
    challenge_secrets::{ReceiptToken, TokenKind, VerificationKey, VerificationToken},
    email_verification::PreparedEmail,
    verification_mail::MailKey,
};
use board_store::verification::{VerificationStore, mail::MailStore};
use board_verification::{AppState, router};
use board_verification_client::protocol;
use serde::Serialize;
use sqlx::PgPool;
use tower::ServiceExt;

async fn request<T: Serialize>(
    app: &Router,
    path: &str,
    value: &T,
) -> (StatusCode, serde_json::Value) {
    let response = app
        .clone()
        .oneshot(
            Request::post(path)
                .header("authorization", format!("Bearer {}", "11".repeat(32)))
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(value).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 262_144).await.unwrap();
    (status, serde_json::from_slice(&bytes).unwrap())
}
fn transported(value: board_domain::anonymous_session::Fingerprints) -> protocol::Fingerprints {
    protocol::Fingerprints {
        token: value.token,
        network: value.network,
        address: value.address,
        environment: value.environment,
    }
}

async fn issued_answer(
    app: &Router,
    context: &protocol::ChallengeContext,
    key: &VerificationKey,
) -> (
    VerificationToken,
    board_domain::challenge_secrets::GeneratedAnswer,
) {
    use base64::Engine;
    let (status, value) = request(app, "/v1/challenge", context).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["state"], "ready");
    assert_eq!(value["slider"], true);
    assert_eq!(value["ttl_seconds"], 120);
    assert_eq!(value["cooldown_seconds"], 5);
    assert!(!value.as_object().unwrap().contains_key("answer"));
    assert!(!value.as_object().unwrap().contains_key("answer_hash"));
    for field in ["image", "background"] {
        let encoded = value[field]
            .as_str()
            .unwrap()
            .strip_prefix("data:image/png;base64,")
            .unwrap();
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
        assert!(bytes.len() <= 65_536);
    }
    let token =
        VerificationToken::parse(TokenKind::Challenge, value["token"].as_str().unwrap()).unwrap();
    // Only this synthetic test owns the issuer's key. Production responses
    // contain images and the opaque token, never the text solution.
    let answer = key.challenge_answer(&token, 6).unwrap();
    (token, answer)
}

#[tokio::test]
async fn authenticated_email_routes_preserve_validation_order_and_rotate_only_after_verified_proof()
{
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let store =
        VerificationStore::connect(&std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
    let mailer = MailStore::connect(&std::env::var("TEST_VERIFICATION_MAIL_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let material = ReceiptToken::generate().unwrap().encode();
    let key = VerificationKey::parse(&material[3..]).unwrap();
    let mail_material = ReceiptToken::generate().unwrap().encode();
    let mail_key = MailKey::parse(&mail_material[3..]).unwrap();
    let app = router(
        AppState::new(
            store.clone(),
            VerificationKey::parse(&material[3..]).unwrap(),
            MailKey::parse(&mail_material[3..]).unwrap(),
            &"11".repeat(32),
        )
        .unwrap(),
    );
    let peer = "192.0.2.41".parse().unwrap();
    let old = Capability::generate()
        .unwrap()
        .fingerprints(Some(peer), *b"US");
    let new = Capability::generate()
        .unwrap()
        .fingerprints(Some(peer), *b"US");
    let email = PreparedEmail::prepare("owned.http.fixture@gmail.com").unwrap();
    let identity = key.email_identity(&email);
    let peer_hash = key.network_identity(peer);
    let outcome=tokio::spawn({let owner=owner.clone();let store=store.clone();let mailer=mailer.clone();async move {
        let session=protocol::Session {fingerprints:transported(old),minted:true,peer,country:"US".into()};
        let (status,value)=request(&app,"/v1/session",&session).await;assert_eq!(status,StatusCode::OK);assert_eq!(value["changed"],true);
        let context=protocol::ChallengeContext {session:protocol::Session {minted:false,..session},purpose:protocol::Purpose::EmailRequest,
            board:None,target:0,form_nonce:"ab".repeat(32),email_token:None};
        let invalid=protocol::EmailRequest {context:context.clone(),email:"not-an-email".into(),challenge:"invalid".into(),answer:"invalid".into()};
        let (status,value)=request(&app,"/v1/email/request",&invalid).await;assert_eq!(status,StatusCode::OK);assert_eq!(value["state"],"bad_email");
        let (challenge,answer)=issued_answer(&app,&context,&key).await;
        let input=protocol::EmailRequest {context:context.clone(),email:email.recipient().into(),challenge:challenge.encode(),answer:answer.as_str().into()};
        let (status,value)=request(&app,"/v1/email/request",&input).await;assert_eq!(status,StatusCode::OK);assert_eq!(value["state"],"queued");
        let (_,value)=request(&app,"/v1/email/request",&input).await;assert_eq!(value["state"],"cooldown","source cooldown precedes CAPTCHA reuse errors");
        let lease=ReceiptToken::generate().unwrap().storage_hash();let jobs=mailer.claim(&lease,100).await.unwrap();assert_eq!(jobs.len(),1);
        let job=&jobs[0];let token_hash:[u8;32]=job.token_hash.as_slice().try_into().unwrap();let delivered=mail_key.open(&token_hash,&job.encrypted_payload).unwrap();
        assert_eq!(delivered.recipient(),email.recipient());assert!(mailer.finish(&job.request_id,&lease,true).await.unwrap());
        let (_,value)=request(&app,"/v1/email/state",&protocol::EmailToken{token:delivered.token().into()}).await;assert_eq!(value["state"],"pending");
        sqlx::query("UPDATE verification_private.peers SET refresh_after=clock_timestamp()-interval '1 second' WHERE peer_hash=$1")
            .bind(peer_hash.as_slice()).execute(&owner).await.unwrap();
        let verify_context=protocol::ChallengeContext {purpose:protocol::Purpose::EmailVerify,email_token:Some(delivered.token().into()),..context};
        let (challenge,answer)=issued_answer(&app,&verify_context,&key).await;
        let input=protocol::EmailVerification {context:verify_context,replacement:transported(new),challenge:challenge.encode(),answer:answer.as_str().into()};
        let (status,value)=request(&app,"/v1/email/verify",&input).await;assert_eq!(status,StatusCode::OK);assert_eq!(value["state"],"verified");
        assert!(store.session(&old.token).await.unwrap().is_none());
        assert_eq!(store.session(&new.token).await.unwrap().unwrap().for_request(chrono::Utc::now().timestamp().try_into().unwrap(),&new).verified_level,1);
        let (_,value)=request(&app,"/v1/email/state",&protocol::EmailToken{token:delivered.token().into()}).await;assert_eq!(value["state"],"already_used");
    }}).await;
    for token in [old.token, new.token] {
        sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
            .bind(token.as_slice())
            .execute(&owner)
            .await
            .unwrap();
    }
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
    mailer.close().await;
    store.close().await;
    owner.close().await;
    outcome.unwrap();
}

#[tokio::test]
async fn trusted_risk_is_snapshotted_and_escalates_local_verification() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let store =
        VerificationStore::connect(&std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
    let material = ReceiptToken::generate().unwrap().encode();
    let key = VerificationKey::parse(&material[3..]).unwrap();
    let mail_material = ReceiptToken::generate().unwrap().encode();
    let app = router(
        AppState::new(
            store.clone(),
            VerificationKey::parse(&material[3..]).unwrap(),
            MailKey::parse(&mail_material[3..]).unwrap(),
            &"11".repeat(32),
        )
        .unwrap(),
    );
    let peer = "192.0.2.43".parse().unwrap();
    let browser = Capability::generate()
        .unwrap()
        .fingerprints(Some(peer), *b"US");
    let peer_hash = key.network_identity(peer);
    let token = VerificationToken::generate(TokenKind::Email).unwrap();
    let token_hash = key.token_hash(&token);
    let other = VerificationToken::generate(TokenKind::Email).unwrap();
    let other_hash = key.token_hash(&other);
    let outcome = tokio::spawn({let owner=owner.clone();let store=store.clone();async move {
        store.ensure_session(&browser, true).await.unwrap();
        let normal=store.risk(&peer_hash,None).await.unwrap();
        assert_eq!((normal.current_score,normal.token_score),(100,100));
        sqlx::query("INSERT INTO verification_private.peer_risk(peer_hash,score,created_at,expires_at) VALUES($1,85,clock_timestamp()-interval '2 hours',clock_timestamp()+interval '1 hour')").bind(peer_hash.as_slice()).execute(&owner).await.unwrap();
        sqlx::query("INSERT INTO verification_private.email_requests(token_hash,email_hash,recipient_hash,peer_hash,country,created_at,expires_at,captcha_failure_limit) VALUES($1,$1,$1,$2,'US',clock_timestamp(),clock_timestamp()+interval '1 hour',6)").bind(token_hash.as_slice()).bind(peer_hash.as_slice()).execute(&owner).await.unwrap();
        sqlx::query("UPDATE verification_private.peer_risk SET score=79 WHERE peer_hash=$1").bind(peer_hash.as_slice()).execute(&owner).await.unwrap();
        sqlx::query("INSERT INTO verification_private.email_requests(token_hash,email_hash,recipient_hash,peer_hash,country,created_at,expires_at,captcha_failure_limit) VALUES($1,$1,$1,$2,'US',clock_timestamp(),clock_timestamp()+interval '1 hour',6)").bind(other_hash.as_slice()).bind(peer_hash.as_slice()).execute(&owner).await.unwrap();
        let risk=store.risk(&peer_hash,Some(&token_hash)).await.unwrap();
        assert_eq!((risk.current_score,risk.token_score),(79,85));assert!(risk.suspicious());assert!(risk.escalated());
        let limit:i16=sqlx::query_scalar("SELECT captcha_failure_limit FROM verification_private.email_requests WHERE token_hash=$1").bind(other_hash.as_slice()).fetch_one(&owner).await.unwrap();assert_eq!(limit,3);
        let context=protocol::ChallengeContext {session:protocol::Session {fingerprints:transported(browser),minted:false,peer,country:"US".into()},purpose:protocol::Purpose::EmailVerify,board:None,target:0,form_nonce:"ab".repeat(32),email_token:Some(token.encode())};
        let (status,value)=request(&app,"/v1/challenge",&context).await;assert_eq!(status,StatusCode::OK);assert_eq!(value["state"],"ready");
        let challenge=VerificationToken::parse(TokenKind::Challenge,value["token"].as_str().unwrap()).unwrap();
        let expected=key.answer_hash(&challenge,key.challenge_answer(&challenge,8).unwrap().as_str()).unwrap();
        let (actual,suspicious):(Vec<u8>,bool)=sqlx::query_as("SELECT answer_hash,suspicious FROM verification_private.challenges WHERE token_hash=$1").bind(key.token_hash(&challenge).as_slice()).fetch_one(&owner).await.unwrap();
        assert_eq!(actual,expected);assert!(suspicious);
        sqlx::query("UPDATE verification_private.peer_risk SET expires_at=clock_timestamp()-interval '1 minute' WHERE peer_hash=$1").bind(peer_hash.as_slice()).execute(&owner).await.unwrap();
        let risk=store.risk(&peer_hash,Some(&token_hash)).await.unwrap();assert_eq!((risk.current_score,risk.token_score),(100,85));assert!(!risk.suspicious());assert!(risk.escalated(),"stored token risk survives current policy expiry");
        // A changed live score must tighten answer consumption, not just final rotation.
        sqlx::query("UPDATE verification_private.peers SET refresh_after=clock_timestamp()-interval '1 second' WHERE peer_hash=$1").bind(peer_hash.as_slice()).execute(&owner).await.unwrap();
        let (status,value)=request(&app,"/v1/challenge",&context).await;assert_eq!(status,StatusCode::OK);
        let stale=VerificationToken::parse(TokenKind::Challenge,value["token"].as_str().unwrap()).unwrap();
        let was_suspicious:bool=sqlx::query_scalar("SELECT suspicious FROM verification_private.challenges WHERE token_hash=$1").bind(key.token_hash(&stale).as_slice()).fetch_one(&owner).await.unwrap();assert!(!was_suspicious);
        sqlx::query("UPDATE verification_private.peer_risk SET score=79,expires_at=clock_timestamp()+interval '1 hour' WHERE peer_hash=$1").bind(peer_hash.as_slice()).execute(&owner).await.unwrap();
        sqlx::query("UPDATE verification_private.email_requests SET captcha_failures=3 WHERE token_hash=$1").bind(token_hash.as_slice()).execute(&owner).await.unwrap();
        let replacement=Capability::generate().unwrap().fingerprints(Some(peer),*b"US");
        let input=protocol::EmailVerification {context:context.clone(),replacement:transported(replacement),challenge:stale.encode(),answer:key.challenge_answer(&stale,8).unwrap().as_str().into()};
        let (status,value)=request(&app,"/v1/email/verify",&input).await;assert_eq!(status,StatusCode::OK);assert_eq!(value["state"],"challenge_required");
        assert!(store.session(&replacement.token).await.unwrap().is_none());
        let receipt_exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM verification_private.receipts WHERE context_hash=$1)").bind(token_hash.as_slice()).fetch_one(&owner).await.unwrap();assert!(!receipt_exists);
        let risk=store.risk(&peer_hash,Some(&other_hash)).await.unwrap();assert!(risk.suspicious());
        sqlx::query("UPDATE verification_private.email_requests SET captcha_failures=3 WHERE token_hash=$1").bind(other_hash.as_slice()).execute(&owner).await.unwrap();
        assert_eq!(store.email_state(&other_hash).await.unwrap(),"invalid");
        for name in ["TEST_PUBLIC_DATABASE_URL","TEST_VERIFICATION_MAIL_DATABASE_URL"] {
            let runtime=PgPool::connect(&std::env::var(name).unwrap()).await.unwrap();
            let error=sqlx::query("SELECT * FROM content.verification_risk($1,NULL)").bind(peer_hash.as_slice()).execute(&runtime).await.unwrap_err();
            assert_eq!(error.as_database_error().and_then(|e|e.code()).as_deref(),Some("42501"));runtime.close().await;
        }
    }}).await;
    sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
        .bind(browser.token.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    for hash in [token_hash, other_hash] {
        sqlx::query("DELETE FROM verification_private.email_requests WHERE token_hash=$1")
            .bind(hash.as_slice())
            .execute(&owner)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM verification_private.peer_risk WHERE peer_hash=$1")
        .bind(peer_hash.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.peers WHERE peer_hash=$1")
        .bind(peer_hash.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    store.close().await;
    owner.close().await;
    outcome.unwrap();
}
