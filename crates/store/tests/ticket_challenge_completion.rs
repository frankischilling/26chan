#![cfg(feature = "database-tests")]
use board_domain::{
    anonymous_session::Capability,
    challenge_secrets::{ReceiptToken, TokenKind, VerificationKey, VerificationToken},
};
use board_store::verification::{
    ChallengeCheck, ChallengePurpose, NewChallenge, TicketCompletion, VerificationStore,
};
use sqlx::PgPool;

#[tokio::test]
async fn solved_ticket_challenge_is_scoped_one_use_and_transactional() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let issuer_url = std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap();
    let store = VerificationStore::connect(&issuer_url).await.unwrap();
    let issuer = PgPool::connect(&issuer_url).await.unwrap();
    let material = ReceiptToken::generate().unwrap().encode();
    let key = VerificationKey::parse(&material[3..]).unwrap();
    let peer = "192.0.2.104".parse().unwrap();
    let fp = Capability::generate()
        .unwrap()
        .fingerprints(Some(peer), *b"US");
    let peer_hash = key.network_identity(peer);
    let candidate = key.token_hash(&VerificationToken::generate(TokenKind::Ticket).unwrap());
    let receipt = ReceiptToken::generate().unwrap().storage_hash();
    let work = {
        let owner = owner.clone();
        let store = store.clone();
        let issuer = issuer.clone();
        async move {
            store.ensure_session(&fp, true).await.unwrap();
            let context = key
                .form_context("ticket", Some("g"), 0, &"ab".repeat(32))
                .unwrap();
            let challenge = VerificationToken::generate(TokenKind::Challenge).unwrap();
            let answer = key.challenge_answer(&challenge, 8).unwrap();
            let answer_hash = key.answer_hash(&challenge, answer.as_str()).unwrap();
            assert!(
                store
                    .issue_challenge(&NewChallenge {
                        token_hash: &key.token_hash(&challenge),
                        answer_hash: &answer_hash,
                        peer_hash: &peer_hash,
                        session_hash: &fp.token,
                        purpose: ChallengePurpose::Ticket,
                        board: Some("g"),
                        target: 0,
                        suspicious: true,
                        context_hash: &context
                    })
                    .await
                    .unwrap()
                    .issued
            );
            assert!(
                store
                    .check_challenge(&ChallengeCheck {
                        token_hash: &key.token_hash(&challenge),
                        answer_hash: Some(&answer_hash),
                        peer_hash: &peer_hash,
                        session_hash: &fp.token,
                        purpose: ChallengePurpose::Ticket,
                        board: Some("g"),
                        target: 0,
                        context_hash: &context,
                        receipt_hash: &receipt
                    })
                    .await
                    .unwrap()
            );
            let mut input = TicketCompletion {
                presented: None,
                candidate: &candidate,
                peer: &peer_hash,
                session: &fp.token,
                board: "g",
                target: 0,
                context: &context,
                receipt: &receipt,
            };
            input.board = "a";
            assert!(store.finish_ticket(&input).await.is_err());
            input.board = "g";
            input.target = 1;
            assert!(store.finish_ticket(&input).await.is_err());
            input.target = 0;
            let other = [0x37; 32];
            input.context = &other;
            assert!(store.finish_ticket(&input).await.is_err());
            input.context = &context;
            input.peer = &other;
            assert!(store.finish_ticket(&input).await.is_err());
            input.peer = &peer_hash;
            input.session = &other;
            assert!(store.finish_ticket(&input).await.is_err());
            input.session = &fp.token;
            // An explicit surrounding rollback restores both the receipt and absence
            // of the candidate ticket, even after successful function execution.
            let mut tx = issuer.begin().await.unwrap();
            let available:bool=sqlx::query_scalar("SELECT available FROM content.verification_finish_ticket(NULL,$1,$2,$3,'g',0,$4,$5)").bind(candidate.as_slice()).bind(peer_hash.as_slice()).bind(fp.token.as_slice()).bind(context.as_slice()).bind(receipt.as_slice()).fetch_one(&mut *tx).await.unwrap();
            assert!(available);
            tx.rollback().await.unwrap();
            assert!(
                !store
                    .ticket_advice(Some(&candidate), &peer_hash, "g")
                    .await
                    .unwrap()
                    .valid
            );
            // Expiry is checked at consumption, not just at answer verification.
            let mut tx = owner.begin().await.unwrap();
            sqlx::query("UPDATE verification_private.receipts SET created_at=statement_timestamp()-interval '31 seconds',expires_at=statement_timestamp()-interval '1 second' WHERE token_hash=$1").bind(receipt.as_slice()).execute(&mut *tx).await.unwrap();
            sqlx::query("SET LOCAL ROLE board_anonymous_owner")
                .execute(&mut *tx)
                .await
                .unwrap();
            let expired = sqlx::query(
                "SELECT * FROM content.verification_finish_ticket(NULL,$1,$2,$3,'g',0,$4,$5)",
            )
            .bind(candidate.as_slice())
            .bind(peer_hash.as_slice())
            .bind(fp.token.as_slice())
            .bind(context.as_slice())
            .bind(receipt.as_slice())
            .execute(&mut *tx)
            .await
            .unwrap_err();
            assert_eq!(
                expired
                    .as_database_error()
                    .and_then(|e| e.code())
                    .as_deref(),
                Some("P0166")
            );
            tx.rollback().await.unwrap();
            let ticket = store.finish_ticket(&input).await.unwrap().unwrap();
            assert!(ticket.issued);
            assert_eq!(ticket.issued_at, ticket.current_at);
            assert!(
                store
                    .ticket_advice(Some(&candidate), &peer_hash, "g")
                    .await
                    .unwrap()
                    .valid
            );
            assert!(
                store.finish_ticket(&input).await.is_err(),
                "Receipt replay must not issue or resume a ticket"
            );
        }
    };
    let outcome = tokio::spawn(work).await;
    sqlx::query("DELETE FROM verification_private.tickets WHERE token_hash=$1")
        .bind(candidate.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
        .bind(fp.token.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.peers WHERE peer_hash=$1")
        .bind(peer_hash.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    issuer.close().await;
    store.close().await;
    owner.close().await;
    outcome.unwrap();
}
