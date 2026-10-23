use super::*;
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use serde_json::json;

async fn consume_pass(
    owner: &PgPool,
    hashes: (&[u8; 32], &[u8; 32], &[u8; 32], &[u8; 32]),
    scope: (&str, i64, &str),
) -> Result<i64, sqlx::Error> {
    let (receipt, peer, session, context) = hashes;
    let (board, target, purpose) = scope;
    let mut tx = owner.begin().await?;
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await?;
    let result = sqlx::query_scalar(
        "SELECT entitlement_id FROM verification_private.pass_take_receipt($1,$2,$3,$4,$5,$6,$7)",
    )
    .bind(receipt.as_slice())
    .bind(peer.as_slice())
    .bind(session.as_slice())
    .bind(context.as_slice())
    .bind(purpose)
    .bind(board)
    .bind(target)
    .fetch_one(&mut *tx)
    .await;
    match result {
        Ok(value) => {
            tx.commit().await?;
            Ok(value)
        }
        Err(error) => {
            tx.rollback().await?;
            Err(error)
        }
    }
}
pub(super) async fn exercise(
    owner: &PgPool,
    app: &Router,
    key: &VerificationKey,
    mut context: protocol::ChallengeContext,
) {
    let identifier = ReceiptToken::generate().unwrap().encode()[3..13].to_ascii_uppercase();
    let token_hash = key.pass_identifier(&identifier).unwrap();
    let pin_hash = Argon2::default()
        .hash_password(
            b"owned-like-membership",
            &SaltString::encode_b64(b"owned-like-fixture").unwrap(),
        )
        .unwrap()
        .to_string();
    let peer = key.network_identity(context.session.peer);
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let entitlement:i64=sqlx::query_scalar("INSERT INTO verification_private.pass_entitlements(token_hash,pin_hash,status,purchased_at,expires_at) VALUES($1,$2,0,'2020-01-02T00:00:00Z',clock_timestamp()+interval '1 year') RETURNING id")
        .bind(token_hash.as_slice()).bind(pin_hash).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    // A fresh owned run models reset without reopening the closed predecessor.
    let run:i64=sqlx::query_scalar("INSERT INTO event_private.runs(id,kind,enabled,starts_at,ends_at) SELECT coalesce(max(id),0)+1,3,true,floor(extract(epoch FROM now()))::bigint-60,floor(extract(epoch FROM now()))::bigint+3600 FROM event_private.runs RETURNING id")
        .fetch_one(owner).await.unwrap();
    context.form_nonce =
        board_domain::source_events::scores::form_nonce(run, &"ab".repeat(32)).unwrap();
    let hash = key
        .form_context(
            "like",
            context.board.as_deref(),
            context.target,
            &context.form_nonce,
        )
        .unwrap();
    let work = {
        let owner = owner.clone();
        let app = app.clone();
        async move {
            let login=call(&app,"/v1/pass/login",&json!({"identifier":identifier,"pin":"owned-like-membership","peer":context.session.peer,"remember":false})).await;
            assert_eq!(login["state"], "ready");
            let request = protocol::PassWriteRequest {
                token: login["token"].as_str().unwrap().into(),
                context,
            };
            let board = request.context.board.as_deref().unwrap();
            let target = request.context.target;
            let anonymous = request.context.session.fingerprints.token;
            let proof = call(&app, "/v1/pass/receipt", &request).await;
            assert_eq!(proof["state"], "verified");
            assert!(proof.get("entitlement_id").is_none());
            assert_eq!(proof["context_hash"], json!(hash));
            let receipt = ReceiptToken::parse(proof["receipt"].as_str().unwrap())
                .unwrap()
                .storage_hash();
            for wrong in ["thread", "reply", "report"] {
                assert!(
                    consume_pass(
                        &owner,
                        (&receipt, &peer, &anonymous, &hash),
                        (board, if wrong == "thread" { 0 } else { target }, wrong)
                    )
                    .await
                    .is_err()
                );
            }
            assert_eq!(
                consume_pass(
                    &owner,
                    (&receipt, &peer, &anonymous, &hash),
                    (board, target, "like")
                )
                .await
                .unwrap(),
                entitlement
            );
            assert!(
                consume_pass(
                    &owner,
                    (&receipt, &peer, &anonymous, &hash),
                    (board, target, "like")
                )
                .await
                .is_err()
            );
            let mut vote_request = protocol::PassWriteRequest {
                token: request.token.clone(),
                context: request.context.clone(),
            };
            vote_request.context.target = sqlx::query_scalar(
                "SELECT id FROM content.posts WHERE board=$1 AND id<>thread_id ORDER BY id LIMIT 1",
            )
            .bind(board)
            .fetch_one(&owner)
            .await
            .unwrap();
            let vote_proof = call(&app, "/v1/pass/receipt", &vote_request).await;
            assert_eq!(vote_proof["state"], "verified");
            let vote_receipt = ReceiptToken::parse(vote_proof["receipt"].as_str().unwrap())
                .unwrap()
                .storage_hash();
            let vote_context: [u8; 32] =
                serde_json::from_value(vote_proof["context_hash"].clone()).unwrap();
            let public = PgPoolOptions::new()
                .max_connections(2)
                .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
                .await
                .unwrap();
            let range_id:i64=sqlx::query_scalar("INSERT INTO admission.network_rules(range_start,range_end) VALUES(3221226219,3221226219) RETURNING id").fetch_one(&owner).await.unwrap();
            let vote = board_store::events::scores::Like {
                run,
                board,
                post: vote_request.context.target,
                actor: &[44; 32],
                session: &anonymous,
                receipt: &vote_receipt,
                peer: &peer,
                context: &vote_context,
                address: "192.0.2.235".parse().unwrap(),
            };
            // A validated Pass bypasses the range gate, but never self-voting.
            let mut capture = owner.begin().await.unwrap();
            sqlx::query("SET LOCAL ROLE board_anonymous_owner")
                .execute(&mut *capture)
                .await
                .unwrap();
            sqlx::query(
                "INSERT INTO verification_private.pass_posts(post_id,identity_hash) VALUES($1,$2)",
            )
            .bind(vote.post)
            .bind(token_hash.as_slice())
            .execute(&mut *capture)
            .await
            .unwrap();
            capture.commit().await.unwrap();
            let error = board_store::events::scores::like(&public, &vote)
                .await
                .unwrap_err();
            assert!(error.to_string().contains("You can't like your own posts."));
            let mut capture = owner.begin().await.unwrap();
            sqlx::query("SET LOCAL ROLE board_anonymous_owner")
                .execute(&mut *capture)
                .await
                .unwrap();
            sqlx::query("DELETE FROM verification_private.pass_posts WHERE post_id=$1")
                .bind(vote.post)
                .execute(&mut *capture)
                .await
                .unwrap();
            capture.commit().await.unwrap();
            assert_eq!(
                board_store::events::scores::like(&public, &vote)
                    .await
                    .unwrap(),
                1
            );
            let totals:(i64,i64)=sqlx::query_as("SELECT count(*),sum(score)::bigint FROM event_private.score_accounts WHERE run_id=$1")
                .bind(run).fetch_one(&owner).await.unwrap();
            assert_eq!(totals, (2, 11));
            sqlx::query("DELETE FROM admission.network_rules WHERE id=$1")
                .bind(range_id)
                .execute(&owner)
                .await
                .unwrap();
            public.close().await;
            let proof = call(&app, "/v1/pass/receipt", &request).await;
            assert_eq!(proof["state"], "verified");
            let receipt = ReceiptToken::parse(proof["receipt"].as_str().unwrap())
                .unwrap()
                .storage_hash();
            sqlx::query("UPDATE event_private.runs SET enabled=false WHERE id=$1")
                .bind(run)
                .execute(&owner)
                .await
                .unwrap();
            assert!(
                consume_pass(
                    &owner,
                    (&receipt, &peer, &anonymous, &hash),
                    (board, target, "like")
                )
                .await
                .is_err()
            );
        }
    };
    let outcome = tokio::spawn(work).await;
    sqlx::query("DELETE FROM event_private.runs WHERE id=$1")
        .bind(run)
        .execute(owner)
        .await
        .unwrap();
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.pass_attempts WHERE peer=$1")
        .bind(peer.as_slice())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.pass_entitlements WHERE id=$1")
        .bind(entitlement)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    outcome.unwrap();
}
