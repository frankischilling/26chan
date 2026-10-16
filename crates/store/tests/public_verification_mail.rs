#![cfg(feature = "database-tests")]
use board_domain::{
    anonymous_session::Capability,
    challenge_secrets::{ReceiptToken, TokenKind, VerificationKey, VerificationToken},
    email_verification::PreparedEmail,
    verification_mail::{MailKey, MailPayload},
};
use board_store::{
    StoreError,
    verification::{
        ChallengeCheck, ChallengePurpose, EmailAcceptance, EmailRequest, NewChallenge,
        VerificationStore, mail::MailStore,
    },
};
use sqlx::PgPool;

#[tokio::test]
async fn encrypted_delivery_requires_receipt_and_exclusive_current_lease() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public_url = std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap();
    let issuer_url = std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap();
    let mail_url = std::env::var("TEST_VERIFICATION_MAIL_DATABASE_URL").unwrap();
    let verifier = VerificationStore::connect(&issuer_url).await.unwrap();
    let mailer = MailStore::connect(&mail_url).await.unwrap();
    assert!(matches!(
        MailStore::connect(&public_url).await,
        Err(StoreError::UnsafeRole)
    ));
    assert!(matches!(
        MailStore::connect(&issuer_url).await,
        Err(StoreError::UnsafeRole)
    ));
    assert!(matches!(
        VerificationStore::connect(&mail_url).await,
        Err(StoreError::UnsafeRole)
    ));
    let material = ReceiptToken::generate().unwrap().encode();
    let key = VerificationKey::parse(&material[3..]).unwrap();
    let mail_key = MailKey::parse(&ReceiptToken::generate().unwrap().encode()[3..]).unwrap();
    let session = Capability::generate().unwrap();
    let fingerprints = session.fingerprints(Some("192.0.2.20".parse().unwrap()), *b"US");
    let replacement = Capability::generate()
        .unwrap()
        .fingerprints(Some("192.0.2.20".parse().unwrap()), *b"US");
    let peer = key.network_identity("192.0.2.20".parse().unwrap());
    let email = PreparedEmail::prepare("owned.fixture@gmail.com").unwrap();
    let identity = key.email_identity(&email);
    let recipient = key.email_recipient(&email);
    let result=tokio::spawn({
        let owner=owner.clone();let verifier=verifier.clone();let mailer=mailer.clone();
        async move {
            verifier.ensure_session(&fingerprints,true).await.unwrap();
            let token=VerificationToken::generate(TokenKind::Email).unwrap();
            let token_hash=key.token_hash(&token);
            let payload=MailPayload::new(email.recipient(),&token.encode()).unwrap();
            let encrypted=mail_key.seal(&token_hash,&payload).unwrap();
            let receipt=ReceiptToken::generate().unwrap();let receipt_hash=receipt.storage_hash();
            let request=EmailRequest {token_hash:&token_hash,email_hash:&identity,recipient_hash:&recipient,
                peer_hash:&peer,session_hash:&fingerprints.token,country:"US",encrypted_payload:&encrypted,receipt_hash:&receipt_hash,context_hash:&recipient};
            assert_eq!(verifier.queue_email(&request).await.unwrap().status,"challenge_required");
            let challenge=VerificationToken::generate(TokenKind::Challenge).unwrap();
            let challenge_hash=key.token_hash(&challenge);
            let answer=key.challenge_answer(&challenge,6).unwrap();
            let answer_hash=key.answer_hash(&challenge,answer.as_str()).unwrap();
            assert!(verifier.issue_challenge(&NewChallenge{token_hash:&challenge_hash,answer_hash:&answer_hash,peer_hash:&peer,
                session_hash:&fingerprints.token,purpose:ChallengePurpose::EmailRequest,board:None,target:0,context_hash:&recipient,suspicious:false}).await.unwrap().issued);
            assert!(verifier.check_challenge(&ChallengeCheck{token_hash:&challenge_hash,answer_hash:Some(&answer_hash),peer_hash:&peer,
                session_hash:&fingerprints.token,purpose:ChallengePurpose::EmailRequest,board:None,target:0,context_hash:&recipient,receipt_hash:&receipt_hash}).await.unwrap());
            let (first,second)=tokio::join!(verifier.queue_email(&request),verifier.queue_email(&request));
            let statuses=[first.unwrap().status,second.unwrap().status];
            assert_eq!(statuses.iter().filter(|s|s.as_str()=="queued").count(),1);
            assert_eq!(statuses.iter().filter(|s|s.as_str()=="challenge_required").count(),1);
            let stored:Vec<u8>=sqlx::query_scalar("SELECT encrypted_payload FROM verification_private.mail_outbox WHERE recipient_hash=$1")
                .bind(recipient.as_slice()).fetch_one(&owner).await.unwrap();
            assert_eq!(stored,encrypted);
            let lease=ReceiptToken::generate().unwrap().storage_hash();
            let other=ReceiptToken::generate().unwrap().storage_hash();
            assert!(mailer.claim(&lease,101).await.is_err());
            let (a,b)=tokio::join!(mailer.claim(&lease,100),mailer.claim(&other,100));
            let a=a.unwrap();let b=b.unwrap();
            assert_eq!(a.len()+b.len(),1,"concurrent workers cannot claim the same pending delivery");
            let (job,active_lease)=if a.is_empty(){(&b[0],&other)}else{(&a[0],&lease)};
            let context:[u8;32]=job.token_hash.as_slice().try_into().unwrap();
            let decoded=mail_key.open(&context,&job.encrypted_payload).unwrap();
            assert_eq!(decoded.recipient(),email.recipient());assert_eq!(decoded.token(),token.encode());
            assert!(!mailer.finish(&job.request_id,&[0;32],true).await.unwrap());
            assert!(mailer.finish(&job.request_id,active_lease,false).await.unwrap());
            assert!(mailer.claim(&lease,100).await.unwrap().is_empty(),"failed delivery waits before retry");
            sqlx::query("UPDATE verification_private.mail_outbox SET retry_after=clock_timestamp()-interval '1 second' WHERE recipient_hash=$1")
                .bind(recipient.as_slice()).execute(&owner).await.unwrap();
            let retry=mailer.claim(&lease,100).await.unwrap();assert_eq!(retry.len(),1);
            assert!(mailer.finish(&retry[0].request_id,&lease,true).await.unwrap());
            assert!(!mailer.finish(&retry[0].request_id,&lease,true).await.unwrap(),"delivery receipt is single-use");
            assert!(mailer.claim(&other,100).await.unwrap().is_empty());
            assert!(sqlx::query_scalar::<_,bool>("SELECT delivered_at IS NOT NULL FROM verification_private.email_requests WHERE token_hash=$1")
                .bind(token_hash.as_slice()).fetch_one(&owner).await.unwrap());
            assert_eq!(verifier.email_state(&token_hash).await.unwrap(),"pending");
            sqlx::query("UPDATE verification_private.peers SET refresh_after=clock_timestamp()-interval '1 second' WHERE peer_hash=$1")
                .bind(peer.as_slice()).execute(&owner).await.unwrap();
            sqlx::query("UPDATE post_secrets.anonymous_sessions SET posts=7,images=2,reports=4,pending=1,created_at=extract(epoch FROM clock_timestamp())::bigint-86400,activity_at=extract(epoch FROM clock_timestamp())::bigint-60 WHERE token_hash=$1")
                .bind(fingerprints.token.as_slice()).execute(&owner).await.unwrap();
            let verify_challenge=VerificationToken::generate(TokenKind::Challenge).unwrap();
            let verify_hash=key.token_hash(&verify_challenge);
            let answer=key.challenge_answer(&verify_challenge,6).unwrap();
            let answer_hash=key.answer_hash(&verify_challenge,answer.as_str()).unwrap();
            let verify_receipt=ReceiptToken::generate().unwrap().storage_hash();
            assert!(verifier.issue_challenge(&NewChallenge {token_hash:&verify_hash,answer_hash:&answer_hash,peer_hash:&peer,
                session_hash:&fingerprints.token,purpose:ChallengePurpose::EmailVerify,board:None,target:0,context_hash:&token_hash,suspicious:false}).await.unwrap().issued);
            assert!(verifier.check_challenge(&ChallengeCheck {token_hash:&verify_hash,answer_hash:Some(&answer_hash),peer_hash:&peer,
                session_hash:&fingerprints.token,purpose:ChallengePurpose::EmailVerify,board:None,target:0,context_hash:&token_hash,receipt_hash:&verify_receipt}).await.unwrap());
            let acceptance=EmailAcceptance {token_hash:&token_hash,peer_hash:&peer,country:"US",receipt_hash:&verify_receipt,previous:&fingerprints,replacement:&replacement};
            let (first,second)=tokio::join!(verifier.accept_email(&acceptance),verifier.accept_email(&acceptance));
            let statuses=[first.unwrap(),second.unwrap()];
            assert_eq!(statuses.iter().filter(|s|s.as_str()=="verified").count(),1);
            assert_eq!(statuses.iter().filter(|s|s.as_str()=="authorization_changed").count(),1);
            assert_eq!(verifier.email_state(&token_hash).await.unwrap(),"already_used");
            assert!(verifier.session(&fingerprints.token).await.unwrap().is_none());
            assert!(verifier.ensure_session(&fingerprints,false).await.is_err());
            let now=chrono::Utc::now().timestamp().try_into().unwrap();
            let state=verifier.session(&replacement.token).await.unwrap().unwrap().for_request(now,&replacement);
            assert_eq!((state.verified_level,state.posts,state.images,state.reports,state.pending),(1,7,2,4,1));
            assert!(verifier.signout(&replacement.token).await.unwrap());
            assert!(!verifier.signout(&replacement.token).await.unwrap());
            assert!(verifier.session(&replacement.token).await.unwrap().is_none());
        }
    }).await;
    sqlx::query("DELETE FROM verification_private.email_requests WHERE email_hash=$1")
        .bind(identity.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
        .bind(fingerprints.token.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.peers WHERE peer_hash=$1")
        .bind(peer.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
        .bind(replacement.token.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.identities WHERE email_hash=$1")
        .bind(identity.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    mailer.close().await;
    verifier.close().await;
    owner.close().await;
    result.unwrap();
}
