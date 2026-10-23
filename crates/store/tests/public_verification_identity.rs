#![cfg(feature = "database-tests")]
mod support;
use board_domain::{
    anonymous_session::Capability,
    challenge_secrets::{ReceiptToken, TokenKind, VerificationKey, VerificationToken},
};
use board_store::verification::{
    ChallengeCheck, ChallengePurpose, EmailAcceptance, NewChallenge, VerificationStore,
};
use sqlx::PgPool;

async fn filtered_post(
    pool: &PgPool,
    board: &str,
    fingerprints: board_domain::anonymous_session::Fingerprints,
) -> Result<i64, board_store::StoreError> {
    use board_store::{
        AnonymousPostingContext, NewPost, PostIdentityKeys, PostMetadata, PostingContext,
    };
    support::create_post_with_anonymous_session(
        pool,
        board,
        0,
        &NewPost {
            name: "Anonymous".into(),
            subject: "".into(),
            comment: "ownedverificationfilter".into(),
            deletion_hash: "owned-fixture".into(),
            sage: false,
        },
        None,
        AnonymousPostingContext {
            posting: PostingContext {
                request_start: chrono::Utc::now(),
                peer: Some("192.0.2.30".parse().unwrap()),
                op_password_proof: None,
            },
            session: board_store::anonymous_session::PostingSession {
                fingerprints,
                minted: false,
                now: chrono::Utc::now(),
            },
        },
        PostMetadata {
            event_scores: false,
            drawing: None,
            spoiler: false,
            keys: PostIdentityKeys {
                tripcode: None,
                poster_id: None,
            },
            country_database: None,
            flag: "",
            options: "",
        },
    )
    .await
}

#[tokio::test]
async fn verification_preserves_unverified_activity_and_resets_previously_verified_browsers() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let verifier =
        VerificationStore::connect(&std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT 've'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    // This owned board isolates verified content-filter admission.
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,captcha_enabled) VALUES($1,'Owned verified admission','Synthetic',2000,100,100,100,10,0,0,0,false)").bind(&board).execute(&owner).await.unwrap();
    sqlx::query("INSERT INTO admission.rules(board,pattern,lenient) VALUES($1,'ownedverificationfilter',true)").bind(&board).execute(&owner).await.unwrap();
    let key = VerificationKey::parse(&ReceiptToken::generate().unwrap().encode()[3..]).unwrap();
    let email = ReceiptToken::generate().unwrap().storage_hash();
    let recipient = ReceiptToken::generate().unwrap().storage_hash();
    let mut contexts = Vec::new();
    for index in 0..4u8 {
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
    let outcome=tokio::spawn({let owner=owner.clone();let verifier=verifier.clone();let public=public.clone();let board=board.clone();async move {
        for (index,(old,new,peer)) in contexts.iter().enumerate() {
            verifier.ensure_session(old,true).await.unwrap();
            assert!(!verifier.signout(&old.token).await.unwrap(),"signout must not destroy an ordinary anonymous browser");
            let idle=index==2;
            sqlx::query("UPDATE post_secrets.anonymous_sessions SET created_at=extract(epoch FROM statement_timestamp())::bigint-CASE WHEN $2 THEN 700000 ELSE 86400 END,activity_at=extract(epoch FROM statement_timestamp())::bigint-CASE WHEN $2 THEN 604801 ELSE 60 END,posts=7,images=2,reports=4,pending=1,verified_level=$3 WHERE token_hash=$1")
                .bind(old.token.as_slice()).bind(idle).bind(if index==0 || index==3 {0i16}else{1i16}).execute(&owner).await.unwrap();
            if index==0 {
                // A young browser with 11 posts is not known yet. Verification,
                // not elapsed age, must enable the source's lenient-rule skip.
                sqlx::query("UPDATE post_secrets.anonymous_sessions SET created_at=extract(epoch FROM statement_timestamp())::bigint-60,posts=11 WHERE token_hash=$1").bind(old.token.as_slice()).execute(&owner).await.unwrap();
                let now=chrono::Utc::now().timestamp().try_into().unwrap();
                let before=verifier.session(&old.token).await.unwrap().unwrap().for_request(now,old);
                assert_eq!(before.verified_level,0);
                assert!(!before.is_known_or_verified(now,1440,0));
                let rejected=filtered_post(&public,&board,*old).await; assert!(matches!(rejected,Err(board_store::StoreError::ContentRejected(_))),"unexpected unverified admission: {rejected:?}");
            }
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
            if index==3 {
                // The link expires after the challenge was answered. Acceptance
                // must recheck it rather than trusting the earlier proof.
                sqlx::query("UPDATE verification_private.email_requests SET created_at=statement_timestamp()-interval '24 hours 2 seconds',expires_at=statement_timestamp()-interval '2 seconds' WHERE token_hash=$1").bind(token_hash.as_slice()).execute(&owner).await.unwrap();
                assert_eq!(verifier.email_state(&token_hash).await.unwrap(),"expired");
                assert_eq!(verifier.accept_email(&EmailAcceptance {token_hash:&token_hash,peer_hash:peer,country:"US",receipt_hash:&receipt,previous:old,replacement:new}).await.unwrap(),"expired");
                assert!(verifier.session(&old.token).await.unwrap().is_some());
                assert!(verifier.session(&new.token).await.unwrap().is_none());
                assert!(sqlx::query_scalar::<_,bool>("SELECT used_at IS NULL FROM verification_private.email_requests WHERE token_hash=$1").bind(token_hash.as_slice()).fetch_one(&owner).await.unwrap());
                continue;
            }
            if index == 0 {
                assert_eq!(verifier.accept_email(&EmailAcceptance {token_hash:&token_hash,peer_hash:peer,country:"US",receipt_hash:&receipt,previous:old,replacement:new}).await.unwrap(),"verified");
            } else {
                // Qualify both reset paths in the second half of a second.
                // Capture the database clock around the real issuer, without
                // changing clocks, weakening stale-request guards, or sleeping
                // between a committed write and its observation.
                let raw = PgPool::connect(&std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap()).await.unwrap();
                let (status,before,after):(String,i64,i64)=sqlx::query_as("WITH waited AS MATERIALIZED (SELECT pg_sleep(mod(1.65-mod(extract(epoch FROM clock_timestamp()),1),1)::double precision)), started AS MATERIALIZED (SELECT floor(extract(epoch FROM clock_timestamp()))::bigint AS before FROM waited), issued AS MATERIALIZED (SELECT before,content.verification_accept_email($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) AS status FROM started) SELECT status,before,floor(extract(epoch FROM clock_timestamp()))::bigint AS after FROM issued")
                    .bind(token_hash.as_slice()).bind(peer.as_slice()).bind("US").bind(receipt.as_slice())
                    .bind(old.token.as_slice()).bind(old.network.as_slice()).bind(old.address.as_slice()).bind(old.environment.as_slice())
                    .bind(new.token.as_slice()).bind(new.network.as_slice()).bind(new.address.as_slice()).bind(new.environment.as_slice())
                    .fetch_one(&raw).await.unwrap();
                assert_eq!(status,"verified");
                let times:(i64,i64,i64,i64)=sqlx::query_as("SELECT created_at,network_at,address_at,environment_at FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
                    .bind(new.token.as_slice()).fetch_one(&owner).await.unwrap();
                for at in [times.0,times.1,times.2,times.3] {
                    assert!((before..=after).contains(&at),"Rotated session escaped issuance interval: {before} <= {at} <= {after}; idle={idle}");
                }
                raw.close().await;
            }
            let state=verifier.session(&new.token).await.unwrap().unwrap().for_request(chrono::Utc::now().timestamp().try_into().unwrap(),new);
            assert_eq!(state.verified_level,1);
            if index==0 {
                assert_eq!((state.posts,state.images,state.reports,state.pending),(11,2,4,1));
                let posted=filtered_post(&public,&board,*new).await.unwrap();
                assert!(posted>0);
                // Verification exempts only lenient rules, not every filter.
                sqlx::query("UPDATE admission.rules SET lenient=false WHERE board=$1").bind(&board).execute(&owner).await.unwrap();
                assert!(matches!(filtered_post(&public,&board,*new).await,Err(board_store::StoreError::ContentRejected(_))));
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
    let tokens: Vec<_> = cleanup
        .iter()
        .flat_map(|(old, new, _)| [old.token, new.token])
        .collect();
    let mut tx =
        support::begin_cleanup_with_sessions(&owner, std::slice::from_ref(&board), &tokens).await;
    support::cleanup_posting(&mut *tx, &board).await;
    for query in [
        "DELETE FROM admission.rules WHERE board=$1",
        "DELETE FROM admission.hits WHERE board=$1",
        "DELETE FROM admission.logs WHERE board=$1",
        "DELETE FROM admission.bans WHERE board=$1",
        "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(query)
            .bind(&board)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();
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
    public.close().await;
    verifier.close().await;
    owner.close().await;
    outcome.unwrap();
}
