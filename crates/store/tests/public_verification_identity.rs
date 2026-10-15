#![cfg(feature = "database-tests")]
use board_domain::{
    anonymous_session::Capability,
    challenge_secrets::{ReceiptToken, TokenKind, VerificationKey, VerificationToken},
};
use board_store::verification::{
    ChallengeCheck, ChallengePurpose, EmailAcceptance, NewChallenge, VerificationStore,
};
use sqlx::PgPool;

#[tokio::test]
async fn verification_preserves_unverified_activity_and_resets_previously_verified_browsers() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let verifier =
        VerificationStore::connect(&std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
    let key = VerificationKey::parse(&ReceiptToken::generate().unwrap().encode()[3..]).unwrap();
    let email = ReceiptToken::generate().unwrap().storage_hash();
    let recipient = ReceiptToken::generate().unwrap().storage_hash();
    let mut contexts = Vec::new();
    for index in 0..3u8 {
        let address = std::net::IpAddr::V4(std::net::Ipv4Addr::new(192, 0, 2, 30 + index));
        let old = Capability::generate()
            .unwrap()
            .fingerprints(Some(address), *b"US");
        let new = Capability::generate()
            .unwrap()
            .fingerprints(Some(address), *b"US");
        contexts.push((old, new, key.network_identity(address)));
    }
    let cleanup = contexts.clone();
    let outcome=tokio::spawn({let owner=owner.clone();let verifier=verifier.clone();async move {
        for (index,(old,new,peer)) in contexts.iter().enumerate() {
            verifier.ensure_session(old,true).await.unwrap();
            assert!(!verifier.signout(&old.token).await.unwrap(),"signout must not destroy an ordinary anonymous browser");
            let idle=index==2;
            sqlx::query("UPDATE post_secrets.anonymous_sessions SET created_at=extract(epoch FROM statement_timestamp())::bigint-CASE WHEN $2 THEN 700000 ELSE 86400 END,activity_at=extract(epoch FROM statement_timestamp())::bigint-CASE WHEN $2 THEN 604801 ELSE 60 END,posts=7,images=2,reports=4,pending=1,verified_level=$3 WHERE token_hash=$1")
                .bind(old.token.as_slice()).bind(idle).bind(if index==0 {0i16}else{1i16}).execute(&owner).await.unwrap();
            let token=VerificationToken::generate(TokenKind::Email).unwrap();let token_hash=key.token_hash(&token);
            // Owned request fixture isolates identity transitions. Delivery and
            // recipient normalization are tested by the queue integration test.
            sqlx::query("INSERT INTO verification_private.email_requests(token_hash,email_hash,recipient_hash,peer_hash,country,captcha_failure_limit,created_at,expires_at) VALUES($1,$2,$3,$4,'US',6,statement_timestamp(),statement_timestamp()+interval '24 hours')")
                .bind(token_hash.as_slice()).bind(email.as_slice()).bind(recipient.as_slice()).bind(peer.as_slice()).execute(&owner).await.unwrap();
            assert_eq!(verifier.email_state(&token_hash).await.unwrap(),"pending");
            assert_eq!(verifier.email_state(&token_hash).await.unwrap(),"pending","read-only link inspection must not consume it");
            let challenge=VerificationToken::generate(TokenKind::Challenge).unwrap();let challenge_hash=key.token_hash(&challenge);
            let answer=key.challenge_answer(&challenge,6).unwrap();let answer_hash=key.answer_hash(&challenge,answer.as_str()).unwrap();
            let receipt=ReceiptToken::generate().unwrap().storage_hash();
            assert!(verifier.issue_challenge(&NewChallenge {token_hash:&challenge_hash,answer_hash:&answer_hash,peer_hash:peer,
                session_hash:&old.token,purpose:ChallengePurpose::EmailVerify,board:None,target:0,context_hash:&token_hash,suspicious:false}).await.unwrap().issued);
            assert!(verifier.check_challenge(&ChallengeCheck {token_hash:&challenge_hash,answer_hash:Some(&answer_hash),peer_hash:peer,
                session_hash:&old.token,purpose:ChallengePurpose::EmailVerify,board:None,target:0,context_hash:&token_hash,receipt_hash:&receipt}).await.unwrap());
            assert_eq!(verifier.accept_email(&EmailAcceptance {token_hash:&token_hash,peer_hash:peer,country:"US",receipt_hash:&receipt,previous:old,replacement:new}).await.unwrap(),"verified");
            let state=verifier.session(&new.token).await.unwrap().unwrap().for_request(chrono::Utc::now().timestamp().try_into().unwrap(),new);
            assert_eq!(state.verified_level,1);
            if index==0 {
                assert_eq!((state.posts,state.images,state.reports,state.pending),(7,2,4,1));
            } else {
                assert_eq!((state.posts,state.images,state.reports,state.pending),(0,0,0,0));
                assert_eq!(state.activity_at,0);
                if idle {assert_eq!(state.action_at,state.created_at);} else {assert_eq!(state.action_at,0);}
            }
            assert!(verifier.session(&old.token).await.unwrap().is_none());
        }
        let identity_count:i64=sqlx::query_scalar("SELECT count(DISTINCT b.identity_id) FROM verification_private.browser_identities b JOIN verification_private.identities i ON i.id=b.identity_id WHERE i.email_hash=$1")
            .bind(email.as_slice()).fetch_one(&owner).await.unwrap();assert_eq!(identity_count,1);
        assert!(verifier.signout(&contexts[0].1.token).await.unwrap());
        assert!(verifier.session(&contexts[1].1.token).await.unwrap().is_some(),"signout must not revoke another browser");
    }}).await;
    for (old, new, peer) in cleanup {
        for hash in [old.token, new.token] {
            sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
                .bind(hash.as_slice())
                .execute(&owner)
                .await
                .unwrap();
        }
        sqlx::query("DELETE FROM verification_private.peers WHERE peer_hash=$1")
            .bind(peer.as_slice())
            .execute(&owner)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM verification_private.email_requests WHERE email_hash=$1")
        .bind(email.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.identities WHERE email_hash=$1")
        .bind(email.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    verifier.close().await;
    owner.close().await;
    outcome.unwrap();
}
