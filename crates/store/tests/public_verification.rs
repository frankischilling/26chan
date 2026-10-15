#![cfg(feature = "database-tests")]
use board_domain::anonymous_session::Capability;
use board_domain::challenge_secrets::{TokenKind, VerificationKey, VerificationToken};
use board_store::{
    StoreError,
    verification::{ChallengeCheck, ChallengePurpose, NewChallenge, VerificationStore},
};
use sqlx::PgPool;

#[tokio::test]
async fn only_verifier_mints_zero_activity_sessions_and_readiness_rejects_drift() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public_url = std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap();
    let public = PgPool::connect(&public_url).await.unwrap();
    let verifier =
        VerificationStore::connect(&std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
    assert!(matches!(
        VerificationStore::connect(&public_url).await,
        Err(StoreError::UnsafeRole)
    ));
    let token = Capability::generate().unwrap();
    let fingerprints = token.fingerprints(Some("192.0.2.19".parse().unwrap()), *b"US");
    let key = VerificationKey::parse(&"01".repeat(32)).unwrap();
    let peer = key.network_identity("192.0.2.19".parse().unwrap());
    let result = tokio::spawn({
        let verifier = verifier.clone();
        let public = public.clone();
        let owner = owner.clone();
        async move {
            assert!(verifier.ensure_session(&fingerprints, true).await.unwrap());
            let now = chrono::Utc::now().timestamp().try_into().unwrap();
            let state = verifier
                .session(&fingerprints.token)
                .await
                .unwrap()
                .unwrap()
                .for_request(now, &fingerprints);
            assert_eq!(
                (
                    state.posts,
                    state.images,
                    state.threads,
                    state.reports,
                    state.pending,
                    state.verified_level
                ),
                (0, 0, 0, 0, 0, 0)
            );
            assert_eq!((state.activity_at, state.action_at), (0, 0));
            assert!(!verifier.ensure_session(&fingerprints, false).await.unwrap());
            let after = verifier
                .session(&fingerprints.token)
                .await
                .unwrap()
                .unwrap()
                .for_request(now, &fingerprints);
            assert_eq!(state, after, "challenge reads must not fabricate activity");
            assert!(
                verifier.ensure_session(&fingerprints, true).await.is_err(),
                "minting cannot adopt an existing capability"
            );
            let denied =
                sqlx::query("SELECT content.verification_ensure_session($1,$2,$3,$4,false)")
                    .bind(fingerprints.token.as_slice())
                    .bind(fingerprints.network.as_slice())
                    .bind(fingerprints.address.as_slice())
                    .bind(fingerprints.environment.as_slice())
                    .execute(&public)
                    .await
                    .unwrap_err();
            assert_eq!(
                denied.as_database_error().and_then(|e| e.code()).as_deref(),
                Some("42501")
            );
            sqlx::query("GRANT USAGE ON SCHEMA verification_private TO board_verifier")
                .execute(&owner)
                .await
                .unwrap();
            assert!(matches!(
                verifier.ready().await,
                Err(StoreError::UnsafeRole)
            ));
            sqlx::query("REVOKE USAGE ON SCHEMA verification_private FROM board_verifier")
                .execute(&owner)
                .await
                .unwrap();
            verifier.ready().await.unwrap();
            let challenge=VerificationToken::generate(TokenKind::Challenge).unwrap();
            let challenge_hash=key.token_hash(&challenge);
            let answer=key.challenge_answer(&challenge,6).unwrap();
            let answer_hash=key.answer_hash(&challenge,answer.as_str()).unwrap();
            let context=[9;32];
            let input=NewChallenge { token_hash:&challenge_hash,answer_hash:&answer_hash,
                peer_hash:&peer,session_hash:&fingerprints.token,purpose:ChallengePurpose::EmailRequest,
                board:None,target:0,suspicious:false,context_hash:&context };
            let issued=verifier.issue_challenge(&input).await.unwrap();
            assert!(issued.issued); assert_eq!(issued.cooldown_seconds,5); assert!(issued.valid_until.is_some());
            let stored_cooldown: f64=sqlx::query_scalar("SELECT extract(epoch FROM p.refresh_after-c.created_at)::float8 FROM verification_private.peers p JOIN verification_private.challenges c USING(peer_hash) WHERE p.peer_hash=$1")
                .bind(peer.as_slice()).fetch_one(&owner).await.unwrap();
            assert_eq!(stored_cooldown,5.0);
            sqlx::query("UPDATE verification_private.peers SET refresh_after=clock_timestamp()+interval '60 seconds' WHERE peer_hash=$1")
                .bind(peer.as_slice()).execute(&owner).await.unwrap();
            let cooling=verifier.issue_challenge(&input).await.unwrap();
            assert!(!cooling.issued); assert!((1..=60).contains(&cooling.cooldown_seconds)); assert!(cooling.valid_until.is_none());
            let denied=sqlx::query("SELECT * FROM content.verification_issue_challenge($1,$2,$3,$4,'email_request',NULL,0,$5,false)")
                .bind(challenge_hash.as_slice()).bind(answer_hash.as_slice()).bind(peer.as_slice())
                .bind(fingerprints.token.as_slice()).bind(context.as_slice()).execute(&public).await.unwrap_err();
            assert_eq!(denied.as_database_error().and_then(|e|e.code()).as_deref(),Some("42501"));
            for (unsolved,expired,expected) in [(3,false,24),(10,false,80),(11,false,300),(20,true,5)] {
                sqlx::query("UPDATE verification_private.peers SET unsolved=$2,unsolved_until=clock_timestamp()+CASE WHEN $3 THEN interval '-1 second' ELSE interval '600 seconds' END,refresh_after=clock_timestamp()-interval '1 second' WHERE peer_hash=$1")
                    .bind(peer.as_slice()).bind(unsolved as i16).bind(expired).execute(&owner).await.unwrap();
                let next=VerificationToken::generate(TokenKind::Challenge).unwrap();
                let next_hash=key.token_hash(&next);
                let next_answer=key.challenge_answer(&next,6).unwrap();
                let next_answer_hash=key.answer_hash(&next,next_answer.as_str()).unwrap();
                let result=verifier.issue_challenge(&NewChallenge{token_hash:&next_hash,answer_hash:&next_answer_hash,..input}).await.unwrap();
                assert!(result.issued); assert_eq!(result.cooldown_seconds,expected);
                assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM verification_private.challenges WHERE peer_hash=$1")
                    .bind(peer.as_slice()).fetch_one(&owner).await.unwrap(),1);
                assert!(!sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM verification_private.challenges WHERE token_hash=$1)")
                    .bind(challenge_hash.as_slice()).fetch_one(&owner).await.unwrap());
            }
            // Context mismatch leaves a pending challenge intact. A genuine
            // attempt consumes it even when its answer is malformed or wrong.
            for mode in 0..4 {
                sqlx::query("UPDATE verification_private.peers SET unsolved=3,unsolved_until=clock_timestamp()+interval '600 seconds',refresh_after=clock_timestamp()-interval '1 second' WHERE peer_hash=$1")
                    .bind(peer.as_slice()).execute(&owner).await.unwrap();
                let next=VerificationToken::generate(TokenKind::Challenge).unwrap();
                let next_hash=key.token_hash(&next);
                let answer=key.challenge_answer(&next,6).unwrap();
                let answer_hash=key.answer_hash(&next,answer.as_str()).unwrap();
                assert!(verifier.issue_challenge(&NewChallenge {token_hash:&next_hash,answer_hash:&answer_hash,..input}).await.unwrap().issued);
                let receipt=board_domain::challenge_secrets::ReceiptToken::generate().unwrap();
                let receipt_hash=receipt.storage_hash();
                let check=ChallengeCheck {token_hash:&next_hash,answer_hash:Some(&answer_hash),peer_hash:&peer,
                    session_hash:&fingerprints.token,purpose:ChallengePurpose::EmailRequest,board:None,target:0,
                    context_hash:&context,receipt_hash:&receipt_hash};
                let mismatch=[88;32];
                assert!(!verifier.check_challenge(&ChallengeCheck {context_hash:&mismatch,..check}).await.unwrap());
                let wrong=[55;32];
                if mode==3 {
                    sqlx::query("UPDATE verification_private.challenges SET created_at=statement_timestamp()-interval '121 seconds',expires_at=statement_timestamp()-interval '1 second' WHERE token_hash=$1")
                        .bind(next_hash.as_slice()).execute(&owner).await.unwrap();
                }
                if mode==0 {
                    let (first,second)=tokio::join!(verifier.check_challenge(&check),verifier.check_challenge(&check));
                    assert_eq!(u8::from(first.unwrap())+u8::from(second.unwrap()),1,"only one concurrent answer may mint a receipt");
                    let result: (i16,bool,bool)=sqlx::query_as("SELECT unsolved_before,may_grant_credit,from_credit FROM verification_private.receipts WHERE token_hash=$1")
                        .bind(receipt_hash.as_slice()).fetch_one(&owner).await.unwrap();
                    assert_eq!(result,(4,false,false));
                    assert_eq!(sqlx::query_scalar::<_,i16>("SELECT unsolved FROM verification_private.peers WHERE peer_hash=$1")
                        .bind(peer.as_slice()).fetch_one(&owner).await.unwrap(),2);
                } else {
                    let answer_hash=match mode {1=>None,2=>Some(&wrong),_=>Some(&answer_hash)};
                    assert!(!verifier.check_challenge(&ChallengeCheck{answer_hash,..check}).await.unwrap());
                    assert!(!sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM verification_private.receipts WHERE token_hash=$1)")
                        .bind(receipt_hash.as_slice()).fetch_one(&owner).await.unwrap());
                }
                assert!(!verifier.check_challenge(&check).await.unwrap(),"consumed challenge cannot be replayed");
            }
            sqlx::query(
                "UPDATE post_secrets.anonymous_sessions SET expires_at=1 WHERE token_hash=$1",
            )
            .bind(fingerprints.token.as_slice())
            .execute(&owner)
            .await
            .unwrap();
            assert!(verifier.ensure_session(&fingerprints, false).await.is_err());
            assert!(
                verifier
                    .session(&fingerprints.token)
                    .await
                    .unwrap()
                    .is_none()
            );
        }
    })
    .await;
    sqlx::query("REVOKE USAGE ON SCHEMA verification_private FROM board_verifier")
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
    verifier.close().await;
    public.close().await;
    owner.close().await;
    result.unwrap();
}
