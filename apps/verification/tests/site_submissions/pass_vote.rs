use super::*;
use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
use board_domain::challenge_secrets::PassSessionToken;
use serde_json::json;

async fn take(
    owner: &PgPool,
    proof: (&[u8; 32], &[u8; 32], &[u8; 32], &[u8; 32]),
    entry: i64,
    campaign: i64,
    commit: bool,
) -> Result<Vec<u8>, sqlx::Error> {
    let mut tx = owner.begin().await?;
    sqlx::query("SET LOCAL ROLE board_submission_owner")
        .execute(&mut *tx)
        .await?;
    let result = sqlx::query_scalar::<_, Vec<u8>>(
        "SELECT submission_api.consume_pass_vote($1,$2,$3,$4,$5,$6)",
    )
    .bind(proof.0.as_slice())
    .bind(proof.1.as_slice())
    .bind(proof.2.as_slice())
    .bind(proof.3.as_slice())
    .bind(entry)
    .bind(campaign)
    .fetch_one(&mut *tx)
    .await;
    match result {
        Ok(value) => {
            if commit {
                tx.commit().await?;
            } else {
                tx.rollback().await?;
            }
            Ok(value)
        }
        Err(error) => {
            tx.rollback().await?;
            Err(error)
        }
    }
}
async fn expiry_after_lock_wait(
    owner: &PgPool,
    proof: ([u8; 32], [u8; 32], [u8; 32], [u8; 32]),
    entry: i64,
    campaign: i64,
) {
    let mut gate = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *gate)
        .await
        .unwrap();
    sqlx::query(
        "SELECT singleton FROM verification_private.pass_policy WHERE singleton FOR UPDATE",
    )
    .execute(&mut *gate)
    .await
    .unwrap();
    sqlx::query("UPDATE submission_private.contest_campaigns SET voting_ends=floor(extract(epoch FROM clock_timestamp()))::bigint+2 WHERE id=$1")
        .bind(campaign).execute(owner).await.unwrap();
    let mut attempt = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_submission_owner")
        .execute(&mut *attempt)
        .await
        .unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *attempt)
        .await
        .unwrap();
    let task = tokio::spawn(async move {
        let result = sqlx::query_scalar::<_, Vec<u8>>(
            "SELECT submission_api.consume_pass_vote($1,$2,$3,$4,$5,$6)",
        )
        .bind(proof.0.as_slice())
        .bind(proof.1.as_slice())
        .bind(proof.2.as_slice())
        .bind(proof.3.as_slice())
        .bind(entry)
        .bind(campaign)
        .fetch_one(&mut *attempt)
        .await;
        attempt.rollback().await.unwrap();
        result
    });
    let mut blocked = false;
    for _ in 0..50 {
        blocked = sqlx::query_scalar("SELECT cardinality(pg_blocking_pids($1))>0")
            .bind(pid)
            .fetch_one(owner)
            .await
            .unwrap();
        if blocked {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    if blocked {
        tokio::time::sleep(std::time::Duration::from_millis(2200)).await;
    }
    gate.rollback().await.unwrap();
    let result = task.await.unwrap();
    assert!(
        blocked,
        "The owned proof must reach the policy lock before its window expires"
    );
    let error = result.unwrap_err();
    assert_eq!(
        error.as_database_error().and_then(|e| e.code()).as_deref(),
        Some("P0002")
    );
    sqlx::query(
        "UPDATE submission_private.contest_campaigns SET voting_ends=253402300799 WHERE id=$1",
    )
    .bind(campaign)
    .execute(owner)
    .await
    .unwrap();
}
async fn member_update(owner: &PgPool, id: i64, query: &'static str) {
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(query).bind(id).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
}
async fn issue(app: &Router, request: &protocol::PassWriteRequest) -> serde_json::Value {
    let (status, value) = call(app, "/v1/pass/receipt", request).await;
    assert_eq!(status, StatusCode::OK);
    value
}
fn receipt(value: &serde_json::Value) -> [u8; 32] {
    assert_eq!(value["state"], "verified");
    assert!(value.get("entitlement_id").is_none());
    ReceiptToken::parse(value["receipt"].as_str().unwrap())
        .unwrap()
        .storage_hash()
}

async fn owned_device(owner: &PgPool, member: i64) -> PassSessionToken {
    let device = PassSessionToken::generate().unwrap();
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO verification_private.pass_sessions(token_hash,entitlement_id,credential_version,created_at,expires_at) SELECT $1,id,credential_version,clock_timestamp(),clock_timestamp()+interval '1 hour' FROM verification_private.pass_entitlements WHERE id=$2")
        .bind(device.storage_hash().as_slice()).bind(member).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    device
}
async fn network_snapshot(owner: &PgPool, member: i64) -> serde_json::Value {
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let value=sqlx::query_scalar("SELECT jsonb_build_array(last_peer,login_scope,posting_scope,last_used) FROM verification_private.pass_entitlements WHERE id=$1")
        .bind(member).fetch_one(&mut *tx).await.unwrap();
    tx.rollback().await.unwrap();
    value
}
pub(super) async fn exercise(
    owner: &PgPool,
    public: &PgPool,
    app: &Router,
    key: &VerificationKey,
    mut context: protocol::ChallengeContext,
    campaign: i64,
    entry: i64,
) {
    let identifier = ReceiptToken::generate().unwrap().encode()[3..13].to_ascii_uppercase();
    let token_hash = key.pass_identifier(&identifier).unwrap();
    let pin_hash = Argon2::default()
        .hash_password(
            b"owned-contest-pass",
            &SaltString::encode_b64(b"owned-contest-fixture").unwrap(),
        )
        .unwrap()
        .to_string();
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let member:i64=sqlx::query_scalar("INSERT INTO verification_private.pass_entitlements(token_hash,pin_hash,status,purchased_at,expires_at) VALUES($1,$2,0,'2020-01-02T00:00:00Z',clock_timestamp()+interval '1 year') RETURNING id")
        .bind(token_hash.as_slice()).bind(pin_hash).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let peer = key.network_identity(context.session.peer);
    let anonymous = context.session.fingerprints.token;
    let hash = key
        .form_context("site_contest_vote", None, entry, &context.form_nonce)
        .unwrap();
    let work = {
        let owner = owner.clone();
        let public = public.clone();
        let app = app.clone();
        async move {
            let (status,login)=call(&app,"/v1/pass/login",&json!({"identifier":identifier,"pin":"owned-contest-pass","peer":context.session.peer,"remember":false})).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(login["state"], "ready");
            context.purpose = protocol::Purpose::SiteContestVote;
            context.target = entry;
            let mut request = protocol::PassWriteRequest {
                token: login["token"].as_str().unwrap().into(),
                context,
            };
            assert_eq!(
                issue(&app, &request).await["state"],
                "none",
                "Submission phase cannot issue a vote proof"
            );
            sqlx::query("UPDATE submission_private.contest_campaigns SET submission_ends=floor(extract(epoch FROM clock_timestamp()))::bigint-60,voting_starts=floor(extract(epoch FROM clock_timestamp()))::bigint-30 WHERE id=$1").bind(campaign).execute(&owner).await.unwrap();
            assert_eq!(
                issue(&app, &request).await["state"],
                "none",
                "Unapproved entry cannot receive votes"
            );
            sqlx::query("UPDATE submission_private.contest_entries SET state=1 WHERE id=$1")
                .bind(entry)
                .execute(&owner)
                .await
                .unwrap();
            let other:i64=sqlx::query_scalar("INSERT INTO submission_private.contest_entries(campaign_id,board,ownership_hash,actor_hash,author,media_id,state,created_at,updated_at) VALUES($1,'g',decode(repeat('03',32),'hex'),decode(repeat('04',32),'hex'),'Owned other',repeat('b',32),1,1,1) RETURNING id").bind(campaign).fetch_one(&owner).await.unwrap();
            let before = network_snapshot(&owner, member).await;
            let proof = issue(&app, &request).await;
            let digest = receipt(&proof);
            assert_eq!(proof["peer_hash"], json!(peer));
            assert_eq!(proof["context_hash"], json!(hash));
            assert!(
                take(
                    &owner,
                    (&digest, &peer, &anonymous, &hash),
                    other,
                    campaign,
                    true
                )
                .await
                .is_err()
            );
            assert!(
                take(
                    &owner,
                    (&digest, &peer, &anonymous, &[0; 32]),
                    entry,
                    campaign,
                    true
                )
                .await
                .is_err()
            );
            let denied = sqlx::query("SELECT submission_api.consume_pass_vote($1,$2,$3,$4,$5,$6)")
                .bind(digest.as_slice())
                .bind(peer.as_slice())
                .bind(anonymous.as_slice())
                .bind(hash.as_slice())
                .bind(entry)
                .bind(campaign)
                .execute(&public)
                .await
                .unwrap_err();
            assert_eq!(
                denied.as_database_error().and_then(|e| e.code()).as_deref(),
                Some("42501")
            );
            expiry_after_lock_wait(&owner, (digest, peer, anonymous, hash), entry, campaign).await;
            let voter = take(
                &owner,
                (&digest, &peer, &anonymous, &hash),
                entry,
                campaign,
                false,
            )
            .await
            .unwrap();
            assert_eq!(voter.len(), 32);
            assert_eq!(
                take(
                    &owner,
                    (&digest, &peer, &anonymous, &hash),
                    entry,
                    campaign,
                    true
                )
                .await
                .unwrap(),
                voter
            );
            assert!(
                take(
                    &owner,
                    (&digest, &peer, &anonymous, &hash),
                    entry,
                    campaign,
                    true
                )
                .await
                .is_err()
            );
            let stale = receipt(&issue(&app, &request).await);
            member_update(
                &owner,
                member,
                "UPDATE verification_private.pass_entitlements SET status=1 WHERE id=$1",
            )
            .await;
            assert!(
                take(
                    &owner,
                    (&stale, &peer, &anonymous, &hash),
                    entry,
                    campaign,
                    true
                )
                .await
                .is_err()
            );
            assert_eq!(issue(&app, &request).await["state"], "none");
            member_update(
                &owner,
                member,
                "UPDATE verification_private.pass_entitlements SET status=0 WHERE id=$1",
            )
            .await;
            let device = owned_device(&owner, member).await;
            request.token = device.encode();
            let fresh = receipt(&issue(&app, &request).await);
            assert_eq!(
                take(
                    &owner,
                    (&fresh, &peer, &anonymous, &hash),
                    entry,
                    campaign,
                    true
                )
                .await
                .unwrap(),
                voter,
                "Credential/device changes must not reset the voter identity"
            );
            let suspended = receipt(&issue(&app, &request).await);
            member_update(
                &owner,
                member,
                "UPDATE verification_private.pass_entitlements SET status=1 WHERE id=$1",
            )
            .await;
            assert!(
                take(
                    &owner,
                    (&suspended, &peer, &anonymous, &hash),
                    entry,
                    campaign,
                    true
                )
                .await
                .is_err()
            );
            assert_eq!(
                issue(&app, &request).await["state"],
                "none",
                "Status changes revoke the old credential version"
            );
            member_update(
                &owner,
                member,
                "UPDATE verification_private.pass_entitlements SET status=0 WHERE id=$1",
            )
            .await;
            request.token = owned_device(&owner, member).await.encode();
            let expired = receipt(&issue(&app, &request).await);
            member_update(&owner,member,"UPDATE verification_private.pass_entitlements SET expires_at=clock_timestamp()-interval '1 second' WHERE id=$1").await;
            assert!(
                take(
                    &owner,
                    (&expired, &peer, &anonymous, &hash),
                    entry,
                    campaign,
                    true
                )
                .await
                .is_err()
            );
            assert_eq!(issue(&app, &request).await["state"], "expired");
            member_update(&owner,member,"UPDATE verification_private.pass_entitlements SET expires_at=clock_timestamp()+interval '1 year' WHERE id=$1").await;
            vote_cases(
                &owner,
                &public,
                &app,
                &peer,
                &mut request,
                (campaign, entry, other, member),
            )
            .await;
            let after = network_snapshot(&owner, member).await;
            assert_eq!(before, after, "Voting must not alter posting-network holds");
            sqlx::query(
                "UPDATE submission_private.contest_campaigns SET enabled=false WHERE id=$1",
            )
            .bind(campaign)
            .execute(&owner)
            .await
            .unwrap();
            assert!(
                take(
                    &owner,
                    (&suspended, &peer, &anonymous, &hash),
                    entry,
                    campaign,
                    true
                )
                .await
                .is_err()
            );
            assert_eq!(issue(&app, &request).await["state"], "none");
        }
    };
    let result = tokio::spawn(work).await;
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.pass_entitlements WHERE id=$1")
        .bind(member)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    result.unwrap();
}

async fn vote_cases(
    owner: &PgPool,
    public: &PgPool,
    app: &Router,
    peer: &[u8; 32],
    request: &mut protocol::PassWriteRequest,
    ids: (i64, i64, i64, i64),
) {
    use board_store::site_submissions::{contests, feedback::Proof};
    let (campaign, entry, other, member) = ids;
    request.token = owned_device(owner, member).await.encode();
    let session = request.context.session.fingerprints.token;
    let mut other_context = request.context.clone();
    other_context.target = other;
    let second_request = protocol::PassWriteRequest {
        token: request.token.clone(),
        context: other_context,
    };
    let first_result = issue(app, request).await;
    let second_result = issue(app, &second_request).await;
    let hash: [u8; 32] = serde_json::from_value(first_result["context_hash"].clone()).unwrap();
    let other_hash: [u8; 32] =
        serde_json::from_value(second_result["context_hash"].clone()).unwrap();
    let first = receipt(&first_result);
    let second = receipt(&second_result);
    let p1 = Proof {
        session: &session,
        receipt: &first,
        peer,
        context: &hash,
    };
    let p2 = Proof {
        session: &session,
        receipt: &second,
        peer,
        context: &other_hash,
    };
    let (left, right) = tokio::join!(
        contests::vote(public, campaign, entry, &p1),
        contests::vote(public, campaign, other, &p2)
    );
    assert_ne!(
        left.unwrap(),
        right.unwrap(),
        "One Pass can vote only once for this board"
    );
    assert!(contests::vote(public, campaign, entry, &p1).await.is_err());
    let totals:(i64,i64)=sqlx::query_as("SELECT count(*),(SELECT sum(score)::bigint FROM submission_private.contest_entries WHERE campaign_id=$1) FROM submission_private.contest_votes WHERE campaign_id=$1").bind(campaign).fetch_one(owner).await.unwrap();
    assert_eq!(totals, (1, 1));
    let listing = contests::listing(public, campaign, "g", 0)
        .await
        .unwrap()
        .unwrap();
    assert!(listing.visible && listing.voting);
    assert_eq!(listing.entries.len(), 2);
    assert!(listing.entries.iter().all(|e| e.score.is_none()));
    for query in [
        "SELECT email FROM submission_private.contest_entries",
        "SELECT voter_hash FROM submission_private.contest_votes",
        "SET ROLE board_contest_owner",
    ] {
        assert!(sqlx::query(query).execute(public).await.is_err());
    }
    let mut owner_probe = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_contest_owner")
        .execute(&mut *owner_probe)
        .await
        .unwrap();
    assert!(
        sqlx::query("SELECT email FROM submission_private.contest_entries")
            .execute(&mut *owner_probe)
            .await
            .is_err()
    );
    owner_probe.rollback().await.unwrap();
    let mut drift = owner.begin().await.unwrap();
    sqlx::query("GRANT SELECT(email) ON submission_private.contest_entries TO board_contest_owner")
        .execute(&mut *drift)
        .await
        .unwrap();
    sqlx::query("SET LOCAL ROLE board_submission_owner")
        .execute(&mut *drift)
        .await
        .unwrap();
    assert!(
        !sqlx::query_scalar::<_, bool>("SELECT submission_api.ready()")
            .fetch_one(&mut *drift)
            .await
            .unwrap()
    );
    drift.rollback().await.unwrap();
    request.token = owned_device(owner, member).await.encode();
    let repeat = receipt(&issue(app, request).await);
    assert!(
        !contests::vote(
            public,
            campaign,
            entry,
            &Proof {
                session: &session,
                receipt: &repeat,
                peer,
                context: &hash
            }
        )
        .await
        .unwrap(),
        "A new device must not reset the vote ledger"
    );
    sqlx::query(
        "UPDATE submission_private.contest_campaigns SET boards=ARRAY['g','a','b'] WHERE id=$1",
    )
    .bind(campaign)
    .execute(owner)
    .await
    .unwrap();
    for (board, ownership, media) in [
        ("a", vec![0xa1_u8; 32], "c".repeat(32)),
        ("b", vec![0xb1_u8; 32], "d".repeat(32)),
    ] {
        let new:i64=sqlx::query_scalar("INSERT INTO submission_private.contest_entries(campaign_id,board,ownership_hash,actor_hash,author,email,media_id,state,created_at,updated_at) VALUES($1,$2,$3,decode(repeat('ab',32),'hex'),'Private credit','private@example.org',$4,1,1,1) RETURNING id")
            .bind(campaign).bind(board).bind(ownership).bind(media).fetch_one(owner).await.unwrap();
        let mut context = request.context.clone();
        context.target = new;
        let scoped = protocol::PassWriteRequest {
            token: request.token.clone(),
            context,
        };
        let result = issue(app, &scoped).await;
        let receipt = receipt(&result);
        let hash: [u8; 32] = serde_json::from_value(result["context_hash"].clone()).unwrap();
        let proof = Proof {
            session: &session,
            receipt: &receipt,
            peer,
            context: &hash,
        };
        if board == "a" {
            assert!(contests::vote(public, campaign, new, &proof).await.unwrap());
        } else {
            sqlx::query(
                "UPDATE submission_private.contest_campaigns SET vote_capacity=votes WHERE id=$1",
            )
            .bind(campaign)
            .execute(owner)
            .await
            .unwrap();
            assert!(matches!(
                contests::vote(public, campaign, new, &proof).await,
                Err(board_store::StoreError::Invalid(
                    "Contest voting is closed."
                ))
            ));
            let mut tx = owner.begin().await.unwrap();
            sqlx::query("SET LOCAL ROLE board_anonymous_owner")
                .execute(&mut *tx)
                .await
                .unwrap();
            assert!(sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM verification_private.pass_site_receipts WHERE token_hash=$1)").bind(receipt.as_slice()).fetch_one(&mut *tx).await.unwrap());
            tx.rollback().await.unwrap();
            let score: i32 = sqlx::query_scalar(
                "SELECT score FROM submission_private.contest_entries WHERE id=$1",
            )
            .bind(new)
            .fetch_one(owner)
            .await
            .unwrap();
            assert_eq!(score, 0);
        }
    }
    let directory = contests::directory(
        public,
        board_domain::site_submissions::contests::Kind::BoardBanner,
        0,
    )
    .await
    .unwrap();
    assert!(directory.campaigns.iter().any(|c| c.id == campaign));
    assert_eq!(
        contests::campaign(public, campaign)
            .await
            .unwrap()
            .unwrap()
            .campaign
            .boards,
        ["g", "a", "b"]
    );
    sqlx::query("UPDATE submission_private.contest_campaigns SET voting_ends=floor(extract(epoch FROM clock_timestamp()))::bigint WHERE id=$1").bind(campaign).execute(owner).await.unwrap();
    let results = contests::listing(public, campaign, "g", 0)
        .await
        .unwrap()
        .unwrap();
    assert!(!results.voting);
    assert_eq!(
        results
            .entries
            .iter()
            .map(|e| e.score.unwrap())
            .sum::<i32>(),
        1
    );
    let raw: serde_json::Value =
        sqlx::query_scalar("SELECT submission_api.contest_listing($1,'a',0)")
            .bind(campaign)
            .fetch_one(public)
            .await
            .unwrap();
    let text = raw.to_string();
    for hidden in [
        "private@example.org",
        "Private credit",
        "ownership_hash",
        "voter_hash",
        "actor_hash",
    ] {
        assert!(!text.contains(hidden));
    }
    sqlx::query("UPDATE submission_private.contest_campaigns SET voting_ends=253402300799,vote_capacity=100 WHERE id=$1").bind(campaign).execute(owner).await.unwrap();
}
