#![cfg(feature = "database-tests")]
use board_domain::{
    challenge_policy::{TicketDecision, ticket_decision},
    challenge_secrets::{TokenKind, VerificationKey, VerificationToken},
};
use board_store::verification::VerificationStore;
use sqlx::PgPool;

#[tokio::test]
async fn ticket_storage_preserves_elapsed_wait_and_issuer_only_authority() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let verifier =
        VerificationStore::connect(&std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
    let key = VerificationKey::parse(&"71".repeat(32)).unwrap();
    let first = key.token_hash(&VerificationToken::generate(TokenKind::Ticket).unwrap());
    let next = key.token_hash(&VerificationToken::generate(TokenKind::Ticket).unwrap());
    let contenders = [
        key.token_hash(&VerificationToken::generate(TokenKind::Ticket).unwrap()),
        key.token_hash(&VerificationToken::generate(TokenKind::Ticket).unwrap()),
    ];
    let peer = key.network_identity("192.0.2.101".parse().unwrap());
    let original_limit: i32 = sqlx::query_scalar(
        "SELECT challenge_limit FROM verification_private.policy WHERE singleton",
    )
    .fetch_one(&owner)
    .await
    .unwrap();
    let result = tokio::spawn({
        let verifier = verifier.clone();
        let owner = owner.clone();
        async move {
            let policy = verifier.board_policy("f").await.unwrap().unwrap();
            assert!(policy.enabled);
            assert!(!policy.twister);
            assert!(verifier.board_policy("j").await.unwrap().is_none());
            let advice = verifier.ticket_advice(None, &peer, "g").await.unwrap();
            assert!(!advice.valid);
            assert_eq!(advice.issued_at, 0);
            assert!(advice.escalation_required, "missing risk input must require a ticket challenge");
            let ticket = verifier.prepare_ticket(None, &first, &peer, "g").await.unwrap().unwrap();
            assert!(ticket.issued);
            assert_eq!(ticket.issued_at, ticket.current_at);
            let advice = verifier.ticket_advice(Some(&first), &peer, "g").await.unwrap();
            assert!(advice.valid);
            assert_eq!(advice.issued_at, ticket.issued_at);
            assert!(!verifier.ticket_advice(Some(&first), &peer, "a").await.unwrap().valid);
            for (score, expected) in [(98, true), (99, false), (100, false)] {
                sqlx::query("INSERT INTO verification_private.peer_risk(peer_hash,score,expires_at) VALUES($1,$2,clock_timestamp()+interval '1 hour') ON CONFLICT(peer_hash) DO UPDATE SET score=EXCLUDED.score,created_at=clock_timestamp(),expires_at=EXCLUDED.expires_at")
                    .bind(peer.as_slice()).bind(score as i16).execute(&owner).await.unwrap();
                assert_eq!(verifier.ticket_advice(None, &peer, "g").await.unwrap().escalation_required, expected);
            }
            sqlx::query("UPDATE verification_private.peer_risk SET created_at=statement_timestamp()-interval '2 hours',expires_at=statement_timestamp()-interval '1 hour' WHERE peer_hash=$1")
                .bind(peer.as_slice()).execute(&owner).await.unwrap();
            assert!(verifier.ticket_advice(None, &peer, "g").await.unwrap().escalation_required);
            assert_eq!(ticket_decision(900, Some(ticket.issued_at as u64), ticket.current_at as u64), TicketDecision::Wait { seconds: 900 });
            // Age only this test's row; do not sleep or alter the issuer clock.
            sqlx::query("UPDATE verification_private.tickets SET created_at=date_trunc('second',statement_timestamp())-interval '900 seconds',expires_at=date_trunc('second',statement_timestamp())+interval '2700 seconds' WHERE token_hash=$1")
                .bind(first.as_slice()).execute(&owner).await.unwrap();
            let resumed = verifier.prepare_ticket(Some(&first), &next, &peer, "g").await.unwrap().unwrap();
            assert!(!resumed.issued);
            assert_eq!(ticket_decision(900, Some(resumed.issued_at as u64), resumed.current_at as u64), TicketDecision::Ready);
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM verification_private.tickets WHERE token_hash=$1")
                .bind(next.as_slice()).fetch_one(&owner).await.unwrap();
            assert_eq!(count, 0, "reuse must not insert the unused candidate");
            sqlx::query("UPDATE verification_private.tickets SET created_at=date_trunc('second',statement_timestamp())-interval '1 hour',expires_at=date_trunc('second',statement_timestamp()) WHERE token_hash=$1")
                .bind(first.as_slice()).execute(&owner).await.unwrap();
            let renewed = verifier.prepare_ticket(Some(&first), &next, &peer, "g").await.unwrap().unwrap();
            assert!(renewed.issued);
            assert_eq!(renewed.issued_at, renewed.current_at);
            sqlx::query("UPDATE verification_private.policy SET challenge_limit=(SELECT count(*) FROM verification_private.tickets) WHERE singleton")
                .execute(&owner).await.unwrap();
            assert!(verifier.prepare_ticket(None, &first, &peer, "g").await.unwrap().is_none(),
                "capacity exhaustion must reach the issuer as unavailable");
            let mut held_policy = owner.begin().await.unwrap();
            sqlx::query("SELECT singleton FROM verification_private.policy WHERE singleton FOR UPDATE")
                .execute(&mut *held_policy).await.unwrap();
            let reused_while_locked = tokio::time::timeout(std::time::Duration::from_secs(3),
                verifier.prepare_ticket(Some(&next), &first, &peer, "g")).await;
            held_policy.rollback().await.unwrap();
            assert!(!reused_while_locked.unwrap().unwrap().unwrap().issued,
                "reuse must not acquire the issuance capacity lock");
            sqlx::query("UPDATE verification_private.policy SET challenge_limit=(SELECT count(*)+1 FROM verification_private.tickets) WHERE singleton")
                .execute(&owner).await.unwrap();
            let (left, right) = tokio::join!(
                verifier.prepare_ticket(None, &contenders[0], &peer, "g"),
                verifier.prepare_ticket(None, &contenders[1], &peer, "g"),
            );
            assert_eq!(usize::from(left.unwrap().is_some()) + usize::from(right.unwrap().is_some()), 1,
                "only one concurrent issuer can take the last capacity slot");
            verifier.ready().await.unwrap();
        }
    })
    .await;
    sqlx::query("UPDATE verification_private.policy SET challenge_limit=$1 WHERE singleton")
        .bind(original_limit)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.peer_risk WHERE peer_hash=$1")
        .bind(peer.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.tickets WHERE token_hash=$1 OR token_hash=$2 OR token_hash=$3 OR token_hash=$4")
        .bind(first.as_slice())
        .bind(next.as_slice())
        .bind(contenders[0].as_slice())
        .bind(contenders[1].as_slice())
        .execute(&owner)
        .await
        .unwrap();
    verifier.close().await;
    owner.close().await;
    result.unwrap();
}
