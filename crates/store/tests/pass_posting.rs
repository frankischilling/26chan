#![cfg(feature = "database-tests")]
use board_domain::{
    anonymous_session::Capability,
    challenge_secrets::{PassSessionToken, ReceiptToken, VerificationKey},
};
use board_store::{
    anonymous_session::PostingSession,
    verification::{VerificationStore, pass::WriteRequest},
    *,
};
use sqlx::PgPool;

#[tokio::test]
async fn pass_receipts_authorize_only_the_bound_post_without_courtesy_credits() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let verifier =
        VerificationStore::connect(&std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT 'pp'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,text_only,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,captcha_enabled,captcha_twister,captcha_allow_bypass) VALUES($1,'Owned Pass posting','',2000,100,100,100,10,true,0,0,0,true,true,true)")
        .bind(&board).execute(&owner).await.unwrap();
    let material = PassSessionToken::generate().unwrap();
    let key = VerificationKey::parse(&material.encode()[3..]).unwrap();
    let poster_key = board_domain::poster_id::PosterIdKey::parse(&material.encode()[3..]).unwrap();
    let peer = "192.0.2.121".parse().unwrap();
    let networks = key.pass_networks(peer).unwrap();
    let fingerprints = Capability::generate()
        .unwrap()
        .fingerprints(Some(peer), *b"US");
    verifier.ensure_session(&fingerprints, true).await.unwrap();
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let account: i64 = sqlx::query_scalar("INSERT INTO verification_private.pass_entitlements(token_hash,pin_hash,status,purchased_at,expires_at) VALUES($1,repeat('owned-placeholder',3),0,'2020-01-02',clock_timestamp()+interval '1 year') RETURNING id")
        .bind(material.storage_hash().as_slice()).fetch_one(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO verification_private.pass_sessions VALUES($1,$2,1,clock_timestamp(),clock_timestamp()+interval '1 day')")
        .bind(material.storage_hash().as_slice()).bind(account).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let outcome = tokio::spawn({let owner=owner.clone();let public=public.clone();let verifier=verifier.clone();let board=board.clone();async move {
        let context = key.form_context("thread",Some(&board),0,&"ab".repeat(32)).unwrap();
        let receipt = ReceiptToken::generate().unwrap();
        let issue = |receipt: [u8;32]| { let verifier=verifier.clone();let board=board.clone();let session=material.storage_hash();let networks=key.pass_networks(peer).unwrap(); async move {
            verifier.pass_write_receipt(&WriteRequest {pass_session:&session,receipt:&receipt,networks:&networks,anonymous:&fingerprints.token,context:&context,purpose:"thread",board:&board,target:0}).await.unwrap()
        }};
        assert_eq!(issue(receipt.storage_hash()).await.status,"ready");
        let submit = |number, proof| { let public=public.clone();let board=board.clone();let poster_key=&poster_key; async move {
            let now=chrono::Utc::now();
            create_post_with_challenge(&public,&board,0,&NewPost{name:"Anonymous".into(),subject:format!("Owned Pass {number}"),comment:format!("Owned Pass posting attempt {number}"),deletion_hash:"fixture".into(),sage:false},None,
                ProtectedPostingContext {request_facts:None,anonymous:AnonymousPostingContext{posting:PostingContext{request_start:now,peer:Some(peer),op_password_proof:None},session:PostingSession{fingerprints,minted:false,now}},challenge:proof},
                PostMetadata{keys:PostIdentityKeys{tripcode:None,poster_id:Some(poster_key)},country_database:None,flag:"",options:"",spoiler:false,drawing:None}).await
        }};
        let missing=submit(0,None).await.unwrap_err();
        assert!(matches!(missing,StoreError::Database(ref e) if e.as_database_error().and_then(|e|e.code()).as_deref()==Some("P0166")),"{missing:?}");
        sqlx::query("UPDATE content.boards SET pass_post_only=true WHERE slug=$1").bind(&board).execute(&owner).await.unwrap();
        let members_only=submit(4,None).await.unwrap_err();
        assert!(matches!(members_only,StoreError::Database(ref e) if e.as_database_error().and_then(|e|e.code()).as_deref()==Some("P0201")),"{members_only:?}");
        let ordinary=ReceiptToken::generate().unwrap();
        let mut tx=owner.begin().await.unwrap();sqlx::query("SET LOCAL ROLE board_anonymous_owner").execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO verification_private.receipts(token_hash,peer_hash,session_hash,purpose,board,target,context_hash,from_credit,suspicious,unsolved_before,may_grant_credit,created_at,expires_at) VALUES($1,$2,$3,'thread',$4,0,$5,false,false,0,false,statement_timestamp(),statement_timestamp()+interval '30 seconds')")
            .bind(ordinary.storage_hash().as_slice()).bind(networks.peer.as_slice()).bind(fingerprints.token.as_slice()).bind(&board).bind(context.as_slice()).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
        let nonmember=submit(5,WriteChallengeProof::from_issuer(&ordinary.encode(),networks.peer,context)).await.unwrap_err();
        assert!(matches!(nonmember,StoreError::Database(ref e) if e.as_database_error().and_then(|e|e.code()).as_deref()==Some("P0201")),"{nonmember:?}");
        let proof=||WriteChallengeProof::from_issuer(&receipt.encode(),networks.peer,context);
        let posted=submit(1,proof()).await.unwrap();assert!(posted>0);
        let persisted:bool=sqlx::query_scalar("SELECT content.pass_post_bypass($1,$2)").bind(posted).bind(fingerprints.token.as_slice()).fetch_one(&public).await.unwrap();
        assert!(!persisted,"committed membership handoff must not authorize a later transaction");
        let replay=submit(2,proof()).await.unwrap_err();
        assert!(matches!(replay,StoreError::Database(ref e) if e.as_database_error().and_then(|e|e.code()).as_deref()==Some("P0166")),"{replay:?}");
        let next=ReceiptToken::generate().unwrap();assert_eq!(issue(next.storage_hash()).await.status,"ready");
        let mut tx=owner.begin().await.unwrap();sqlx::query("SET LOCAL ROLE board_anonymous_owner").execute(&mut *tx).await.unwrap();
        sqlx::query("UPDATE verification_private.pass_entitlements SET pin_hash=pin_hash||'-rotated' WHERE id=$1").bind(account).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
        let revoked=submit(3,WriteChallengeProof::from_issuer(&next.encode(),networks.peer,context)).await.unwrap_err();
        assert!(matches!(revoked,StoreError::Database(ref e) if e.as_database_error().and_then(|e|e.code()).as_deref()==Some("P0166")),"{revoked:?}");
        // CAPTCHA-off is a trusted source bypass even on a Pass-only board.
        sqlx::query("UPDATE content.boards SET captcha_enabled=false WHERE slug=$1").bind(&board).execute(&owner).await.unwrap();
        assert!(submit(6,None).await.unwrap()>0);
        assert!(submit(7,WriteChallengeProof::from_issuer(&ordinary.encode(),networks.peer,context)).await.unwrap()>0);
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1").bind(&board).fetch_one(&owner).await.unwrap();assert_eq!(count,3);
        let credits:i64=sqlx::query_scalar("SELECT count(*) FROM verification_private.credits WHERE identity_key=sha256(convert_to('26chan:captcha-credit:browser:v1','UTF8')||$1::bytea)").bind(fingerprints.token.as_slice()).fetch_one(&owner).await.unwrap();assert_eq!(credits,0);
    }}).await;
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(&board)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM content.threads WHERE board=$1 ORDER BY id FOR UPDATE")
        .bind(&board)
        .execute(&mut *tx)
        .await
        .unwrap();
    for statement in [
        "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM post_secrets.posting_thread_actions WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(statement)
            .bind(&board)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
        .bind(fingerprints.token.as_slice())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.pass_entitlements WHERE id=$1")
        .bind(account)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    public.close().await;
    verifier.close().await;
    owner.close().await;
    outcome.unwrap();
}
