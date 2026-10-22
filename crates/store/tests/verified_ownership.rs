#![cfg(feature = "database-tests")]
mod support;
use board_domain::anonymous_session::Capability;
use board_store::{
    AnonymousPostingContext, NewPost, PostIdentityKeys, PostMetadata, PostingContext,
    anonymous_session::{PostingSession, post_proof},
    verification::VerificationStore,
};
use chrono::{Timelike, Utc};
use sqlx::PgPool;

async fn create(public: &PgPool, board: &str, capability: &Capability) -> i64 {
    support::create_post_with_anonymous_session(
        public,
        board,
        0,
        &NewPost {
            name: "Anonymous".into(),
            subject: "Owned verification fixture".into(),
            comment: support::distinct_comment("Synthetic verified ownership"),
            deletion_hash: "owned-verification-proof".into(),
            sage: false,
        },
        None,
        AnonymousPostingContext {
            posting: PostingContext {
                request_start: Utc::now(),
                peer: Some("192.0.2.23".parse().unwrap()),
                op_password_proof: None,
            },
            session: PostingSession {
                fingerprints: capability.fingerprints(Some("192.0.2.23".parse().unwrap()), *b"US"),
                minted: false,
                now: Utc::now().with_nanosecond(0).unwrap(),
            },
        },
        PostMetadata {
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
    .unwrap()
}

#[tokio::test]
async fn verified_ownership_survives_browser_revocation_without_sharing_activity_or_adopting_old_posts()
 {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let verifier =
        VerificationStore::connect(&std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT 'ev'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(captcha_enabled,slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,deletion_known_min_seconds,deletion_unknown_min_seconds,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) VALUES(false,$1,'Owned verification','Synthetic',2000,100,100,100,10,0,0,0,0,0)").bind(&board).execute(&owner).await.unwrap();
    let a = Capability::generate().unwrap();
    let b = Capability::generate().unwrap();
    let c = Capability::generate().unwrap();
    let tokens = [a.storage_hash(), b.storage_hash(), c.storage_hash()];
    let email = Capability::generate().unwrap().storage_hash();
    for cap in [&a, &b, &c] {
        verifier
            .ensure_session(
                &cap.fingerprints(Some("192.0.2.23".parse().unwrap()), *b"US"),
                true,
            )
            .await
            .unwrap();
    }
    // This fixture seeds the private association to isolate ownership tests.
    // The separate mail integration test exercises real receipt-backed linking.
    let identity: String = sqlx::query_scalar(
        "INSERT INTO verification_private.identities(email_hash) VALUES($1) RETURNING id::text",
    )
    .bind(email.as_slice())
    .fetch_one(&owner)
    .await
    .unwrap();
    for token in &tokens[..2] {
        sqlx::query("INSERT INTO verification_private.browser_identities(session_hash,identity_id) VALUES($1,$2::text::uuid)")
            .bind(token.as_slice()).bind(&identity).execute(&owner).await.unwrap();
        sqlx::query(
            "UPDATE post_secrets.anonymous_sessions SET verified_level=1 WHERE token_hash=$1",
        )
        .bind(token.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    }
    let outcome=tokio::spawn({let owner=owner.clone();let public=public.clone();let verifier=verifier.clone();let board=board.clone();let identity=identity.clone();async move {
        let anonymous=create(&public,&board,&c).await;
        let first=create(&public,&board,&a).await;
        assert!(post_proof(&public,&tokens[1],&board,first).await.unwrap().is_some());
        assert!(post_proof(&public,&tokens[2],&board,first).await.unwrap().is_none());
        let now=Utc::now().timestamp().try_into().unwrap();
        let bfp=b.fingerprints(Some("192.0.2.23".parse().unwrap()),*b"US");
        let bstate=verifier.session(&tokens[1]).await.unwrap().unwrap().for_request(now,&bfp);
        assert_eq!((bstate.posts,bstate.pending),(0,0),"another browser's post must not create activity here");
        let second=create(&public,&board,&b).await;
        let identities:Vec<String>=sqlx::query_scalar("SELECT automatic_identity::text FROM post_secrets.posting_history WHERE post_id=ANY($1) ORDER BY post_id")
            .bind(vec![first,second]).fetch_all(&owner).await.unwrap();
        assert_eq!(identities,vec![identity.clone(),identity.clone()]);
        let distinct:i64=sqlx::query_scalar("SELECT count(DISTINCT automatic_identity) FROM post_secrets.anonymous_sessions WHERE token_hash=ANY($1::bytea[])")
            .bind(vec![tokens[0].as_slice(),tokens[1].as_slice()]).fetch_one(&owner).await.unwrap();assert_eq!(distinct,2);
        sqlx::query("INSERT INTO verification_private.browser_identities(session_hash,identity_id) VALUES($1,$2::text::uuid)")
            .bind(tokens[2].as_slice()).bind(&identity).execute(&owner).await.unwrap();
        assert!(post_proof(&public,&tokens[1],&board,anonymous).await.unwrap().is_none(),"linking must not backfill old anonymous posts");
        assert!(verifier.signout(&tokens[0]).await.unwrap());
        assert!(post_proof(&public,&tokens[0],&board,first).await.unwrap().is_none());
        assert!(post_proof(&public,&tokens[1],&board,first).await.unwrap().is_some());
        sqlx::query("UPDATE post_secrets.anonymous_sessions SET activity_at=extract(epoch FROM clock_timestamp())::bigint-604801 WHERE token_hash=$1")
            .bind(tokens[1].as_slice()).execute(&owner).await.unwrap();
        let state=verifier.session(&tokens[1]).await.unwrap().unwrap().for_request(Utc::now().timestamp().try_into().unwrap(),&bfp);
        assert_eq!(state.verified_level,0);
        assert!(post_proof(&public,&tokens[1],&board,first).await.unwrap().is_some(),"idle activity reset retains identity ownership");
    }}).await;
    let mut tx =
        support::begin_cleanup_with_sessions(&owner, std::slice::from_ref(&board), &tokens).await;
    support::cleanup_posting(&mut tx, &board).await;
    for query in [
        "DELETE FROM content.reports WHERE board=$1",
        "DELETE FROM content.post_media WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
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
    for token in tokens {
        sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
            .bind(token.as_slice())
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM verification_private.identities WHERE email_hash=$1")
        .bind(email.as_slice())
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    verifier.close().await;
    public.close().await;
    owner.close().await;
    outcome.unwrap();
}
