#![cfg(feature = "database-tests")]
use board_domain::{anonymous_session::Capability, challenge_secrets::ReceiptToken};
use board_store::verification::{EmailAcceptance, EmailRequest, VerificationStore};
use sqlx::PgPool;

async fn receipt(
    owner: &PgPool,
    session: &[u8; 32],
    peer: &[u8; 32],
    context: &[u8; 32],
    purpose: &str,
) -> [u8; 32] {
    let hash = ReceiptToken::generate().unwrap().storage_hash();
    sqlx::query("INSERT INTO verification_private.receipts(token_hash,peer_hash,session_hash,purpose,board,target,context_hash,created_at,expires_at,unsolved_before,may_grant_credit,from_credit,suspicious) VALUES($1,$2,$3,$4,NULL,0,$5,statement_timestamp(),statement_timestamp()+interval '30 seconds',0,false,false,false)")
        .bind(hash.as_slice()).bind(peer.as_slice()).bind(session.as_slice()).bind(purpose).bind(context.as_slice()).execute(owner).await.unwrap();
    hash
}
// Operator INSERT/UPDATE/DELETE must serialize with admission decisions.
async fn recipient_writes_wait_for_admission(owner: &PgPool, recipient: &[u8; 32]) {
    let mut decision = owner.begin().await.unwrap();
    sqlx::query("SELECT singleton FROM verification_private.policy WHERE singleton FOR UPDATE")
        .execute(&mut *decision)
        .await
        .unwrap();
    let mut writer = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL lock_timeout='100ms'")
        .execute(&mut *writer)
        .await
        .unwrap();
    let error = sqlx::query(
        "INSERT INTO verification_private.blocked_recipients(recipient_hash) VALUES($1)",
    )
    .bind(recipient.as_slice())
    .execute(&mut *writer)
    .await
    .unwrap_err();
    writer.rollback().await.unwrap();
    decision.rollback().await.unwrap();
    assert_eq!(
        error.as_database_error().and_then(|e| e.code()).as_deref(),
        Some("55P03")
    );
    let present: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM verification_private.blocked_recipients WHERE recipient_hash=$1)")
        .bind(recipient.as_slice()).fetch_one(owner).await.unwrap();
    assert!(!present);
}

#[tokio::test]
async fn recipient_blacklist_and_existing_bans_cannot_mint_a_new_verified_identity() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let verifier =
        VerificationStore::connect(&std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
    let old = Capability::generate()
        .unwrap()
        .fingerprints(Some("192.0.2.92".parse().unwrap()), *b"US");
    let new = Capability::generate()
        .unwrap()
        .fingerprints(Some("192.0.2.92".parse().unwrap()), *b"US");
    let peer = ReceiptToken::generate().unwrap().storage_hash();
    let email = ReceiptToken::generate().unwrap().storage_hash();
    let recipient = ReceiptToken::generate().unwrap().storage_hash();
    let token = ReceiptToken::generate().unwrap().storage_hash();
    verifier.ensure_session(&old, true).await.unwrap();
    let identity:String=sqlx::query_scalar("UPDATE post_secrets.anonymous_sessions SET automatic_identity=gen_random_uuid() WHERE token_hash=$1 RETURNING automatic_identity::text").bind(old.token.as_slice()).fetch_one(&owner).await.unwrap();
    let canonical: String = sqlx::query_scalar(
        "INSERT INTO verification_private.identities(email_hash) VALUES($1) RETURNING id::text",
    )
    .bind(email.as_slice())
    .fetch_one(&owner)
    .await
    .unwrap();
    for name in [
        "TEST_PUBLIC_DATABASE_URL",
        "TEST_VERIFICATION_DATABASE_URL",
        "TEST_VERIFICATION_MAIL_DATABASE_URL",
    ] {
        let runtime = PgPool::connect(&std::env::var(name).unwrap())
            .await
            .unwrap();
        let error = sqlx::query_scalar::<_, bool>(
            "SELECT content.verification_identity_banned($1::text::uuid)",
        )
        .bind(&identity)
        .fetch_one(&runtime)
        .await
        .unwrap_err();
        assert_eq!(
            error.as_database_error().and_then(|e| e.code()).as_deref(),
            Some("42501")
        );
        let error =
            sqlx::query("SELECT recipient_hash FROM verification_private.blocked_recipients")
                .execute(&runtime)
                .await
                .unwrap_err();
        assert_eq!(
            error.as_database_error().and_then(|e| e.code()).as_deref(),
            Some("42501")
        );
        runtime.close().await;
    }
    let result=tokio::spawn({let owner=owner.clone();let verifier=verifier.clone();let identity=identity.clone();let canonical=canonical.clone();async move {
        recipient_writes_wait_for_admission(&owner, &recipient).await;
        sqlx::query("INSERT INTO verification_private.blocked_recipients(recipient_hash) VALUES($1)").bind(recipient.as_slice()).execute(&owner).await.unwrap();
        let proof=receipt(&owner,&old.token,&peer,&recipient,"email_request").await;
        let request=EmailRequest {token_hash:&token,email_hash:&email,recipient_hash:&recipient,peer_hash:&peer,session_hash:&old.token,country:"US",encrypted_payload:&[7;128],receipt_hash:&proof,context_hash:&recipient};
        assert_eq!(verifier.queue_email(&request).await.unwrap().status,"queued");
        let queued:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM verification_private.email_requests WHERE token_hash=$1)").bind(token.as_slice()).fetch_one(&owner).await.unwrap();assert!(!queued);
        assert_eq!(verifier.queue_email(&request).await.unwrap().status,"challenge_required","fake success still consumes the proof");
        sqlx::query("DELETE FROM verification_private.blocked_recipients WHERE recipient_hash=$1").bind(recipient.as_slice()).execute(&owner).await.unwrap();
        let mut tx=owner.begin().await.unwrap();sqlx::query("SET LOCAL ROLE board_restriction_owner").execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO restrictions.bans(board,post_id,actor_hash,automatic_identity,global,warning,starts_at,ends_at,public_reason,private_reason,account_id,charged_bytes) VALUES('a',1,$1,$2::text::uuid,false,false,clock_timestamp()-interval '1 hour',clock_timestamp()+interval '1 hour','Owned ban','',1,512)").bind(peer.as_slice()).bind(&identity).execute(&mut *tx).await.unwrap();
        sqlx::query("UPDATE restrictions.capacity SET used_bytes=used_bytes+512 WHERE singleton").execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
        let proof=receipt(&owner,&old.token,&peer,&recipient,"email_request").await;let request=EmailRequest {receipt_hash:&proof,..request};
        assert_eq!(verifier.queue_email(&request).await.unwrap().status,"queued");
        let queued:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM verification_private.email_requests WHERE token_hash=$1)").bind(token.as_slice()).fetch_one(&owner).await.unwrap();assert!(!queued,"a board-specific password ban also suppresses the source request");
        let mut tx=owner.begin().await.unwrap();sqlx::query("SET LOCAL ROLE board_restriction_owner").execute(&mut *tx).await.unwrap();
        sqlx::query("UPDATE restrictions.bans SET ends_at=clock_timestamp()-interval '1 minute' WHERE automatic_identity=$1::text::uuid").bind(&identity).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
        let proof=receipt(&owner,&old.token,&peer,&recipient,"email_request").await;let request=EmailRequest {receipt_hash:&proof,..request};assert_eq!(verifier.queue_email(&request).await.unwrap().status,"queued");
        let queued:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM verification_private.email_requests WHERE token_hash=$1)").bind(token.as_slice()).fetch_one(&owner).await.unwrap();assert!(queued,"expired bans no longer suppress delivery");
        let mut tx=owner.begin().await.unwrap();sqlx::query("SET LOCAL ROLE board_restriction_owner").execute(&mut *tx).await.unwrap();
        sqlx::query("UPDATE restrictions.bans SET ends_at=clock_timestamp()+interval '1 hour' WHERE automatic_identity=$1::text::uuid").bind(&identity).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
        let proof=receipt(&owner,&old.token,&peer,&token,"email_verify").await;
        let accept=EmailAcceptance {token_hash:&token,peer_hash:&peer,country:"US",receipt_hash:&proof,previous:&old,replacement:&new};
        assert_eq!(verifier.accept_email(&accept).await.unwrap(),"invalid");assert!(verifier.session(&old.token).await.unwrap().is_some());assert!(verifier.session(&new.token).await.unwrap().is_none());assert_eq!(verifier.email_state(&token).await.unwrap(),"pending");
        let mut tx=owner.begin().await.unwrap();sqlx::query("SET LOCAL ROLE board_restriction_owner").execute(&mut *tx).await.unwrap();
        sqlx::query("UPDATE restrictions.bans SET ends_at=clock_timestamp()-interval '1 minute' WHERE automatic_identity=$1::text::uuid").bind(&identity).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
        sqlx::query("INSERT INTO verification_private.blocked_recipients(recipient_hash) VALUES($1)").bind(recipient.as_slice()).execute(&owner).await.unwrap();
        let proof=receipt(&owner,&old.token,&peer,&token,"email_verify").await;let accept=EmailAcceptance {receipt_hash:&proof,..accept};
        assert_eq!(verifier.accept_email(&accept).await.unwrap(),"invalid","blacklists are rechecked after delivery");
        sqlx::query("DELETE FROM verification_private.blocked_recipients WHERE recipient_hash=$1").bind(recipient.as_slice()).execute(&owner).await.unwrap();
        let mut tx=owner.begin().await.unwrap();sqlx::query("SET LOCAL ROLE board_restriction_owner").execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO restrictions.bans(board,post_id,actor_hash,automatic_identity,global,warning,starts_at,ends_at,public_reason,private_reason,account_id,charged_bytes) VALUES('a',1,$1,$2::text::uuid,true,false,clock_timestamp()-interval '1 hour',NULL,'Owned email identity ban','',1,512)").bind(peer.as_slice()).bind(&canonical).execute(&mut *tx).await.unwrap();
        sqlx::query("UPDATE restrictions.capacity SET used_bytes=used_bytes+512 WHERE singleton").execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
        let proof=receipt(&owner,&old.token,&peer,&token,"email_verify").await;let accept=EmailAcceptance {receipt_hash:&proof,..accept};
        assert_eq!(verifier.accept_email(&accept).await.unwrap(),"invalid","an unbanned browser cannot adopt a banned email identity");
        assert!(verifier.session(&new.token).await.unwrap().is_none());
        let mut tx=owner.begin().await.unwrap();sqlx::query("SET LOCAL ROLE board_restriction_owner").execute(&mut *tx).await.unwrap();
        sqlx::query("UPDATE restrictions.bans SET ends_at=clock_timestamp()-interval '1 minute' WHERE automatic_identity=$1::text::uuid").bind(&canonical).execute(&mut *tx).await.unwrap();tx.commit().await.unwrap();
        let proof=receipt(&owner,&old.token,&peer,&token,"email_verify").await;let accept=EmailAcceptance {receipt_hash:&proof,..accept};
        assert_eq!(verifier.accept_email(&accept).await.unwrap(),"verified");assert!(verifier.session(&old.token).await.unwrap().is_none());
    }}).await;
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("WITH removed AS(DELETE FROM restrictions.bans WHERE automatic_identity IN($1::text::uuid,$2::text::uuid) RETURNING charged_bytes) UPDATE restrictions.capacity SET used_bytes=used_bytes-coalesce((SELECT sum(charged_bytes) FROM removed),0) WHERE singleton").bind(&identity).bind(&canonical).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    sqlx::query("DELETE FROM verification_private.blocked_recipients WHERE recipient_hash=$1")
        .bind(recipient.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.email_requests WHERE token_hash=$1")
        .bind(token.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    for hash in [old.token, new.token] {
        sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
            .bind(hash.as_slice())
            .execute(&owner)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM verification_private.identities WHERE email_hash=$1")
        .bind(email.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    verifier.close().await;
    owner.close().await;
    result.unwrap();
}
