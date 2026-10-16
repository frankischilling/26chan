#![cfg(feature = "database-tests")]
mod support;
use board_domain::{
    anonymous_session::Capability,
    challenge_secrets::{ReceiptToken, TokenKind, VerificationKey, VerificationToken},
};
use board_store::{
    anonymous_session::PostingSession,
    verification::{
        ChallengeCheck, ChallengePurpose, CreditRequest, NewChallenge, VerificationStore,
    },
    *,
};
use sqlx::PgPool;

#[tokio::test]
async fn report_admission_consumes_normal_or_credit_proofs_in_the_same_transaction() {
    for categorical in [false, true] {
        for credit in [false, true] {
            scenario(categorical, credit).await;
        }
    }
}
async fn scenario(categorical: bool, credit: bool) {
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
        sqlx::query_scalar("SELECT 'rc'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Report challenge','Owned fixture',2000,100,100,100,10)").bind(&board).execute(&owner).await.unwrap();
    let target: i64 =
        sqlx::query_scalar("INSERT INTO content.threads(board) VALUES($1) RETURNING id")
            .bind(&board)
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','Report target','Synthetic')").bind(target).bind(&board).execute(&owner).await.unwrap();
    let mut catalog_read = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_report_admission_owner")
        .execute(&mut *catalog_read)
        .await
        .unwrap();
    let original: Option<i64> = sqlx::query_scalar(
        "SELECT active_catalog_revision FROM post_secrets.report_admission_gate WHERE singleton",
    )
    .fetch_one(&mut *catalog_read)
    .await
    .unwrap();
    catalog_read.rollback().await.unwrap();
    let revision: Option<i64> = if categorical {
        Some(sqlx::query_scalar("SELECT content.import_report_catalog($1::text::jsonb)").bind(serde_json::json!({"version":1,"categories":[{"id":1,"board":"","op_only":false,"reply_only":false,"image_only":false,"exclude_boards":null,"title":"Owned category","weight":0.5,"filtered":0}]}).to_string()).fetch_one(&owner).await.unwrap())
    } else {
        None
    };
    sqlx::query("SELECT content.set_report_catalog_active($1)")
        .bind(revision)
        .execute(&owner)
        .await
        .unwrap();
    let peer = "192.0.2.107".parse().unwrap();
    let fingerprints = Capability::generate()
        .unwrap()
        .fingerprints(Some(peer), *b"US");
    let disabled_peer = "192.0.2.109".parse().unwrap();
    let disabled_fp = Capability::generate()
        .unwrap()
        .fingerprints(Some(disabled_peer), *b"US");
    let material = ReceiptToken::generate().unwrap().encode();
    let key = VerificationKey::parse(&material[3..]).unwrap();
    let peer_hash = key.network_identity(peer);
    let inherited_peer = "192.0.2.110".parse().unwrap();
    let inherited_fp = Capability::generate()
        .unwrap()
        .fingerprints(Some(inherited_peer), *b"US");
    let inherited_hash = key.network_identity(inherited_peer);
    let receipt = ReceiptToken::generate().unwrap();
    let receipt_hash = receipt.storage_hash();
    let poster_key = support::fresh_key();
    let history_actors = [
        peer,
        "192.0.2.108".parse().unwrap(),
        disabled_peer,
        "192.0.2.110".parse().unwrap(),
    ]
    .map(|peer| *poster_key.public_report_rate_identity(peer).as_bytes());
    let prior_epoch:(Option<Vec<u8>>,chrono::DateTime<chrono::Utc>)=sqlx::query_as("SELECT report_history_epoch,report_history_complete_after FROM verification_private.policy WHERE singleton").fetch_one(&owner).await.unwrap();
    let work = {
        let owner = owner.clone();
        let public = public.clone();
        let verifier = verifier.clone();
        let board = board.clone();
        async move {
            verifier.ensure_session(&fingerprints, true).await.unwrap();
            let context = key
                .form_context("report", Some(&board), target, &"ab".repeat(32))
                .unwrap();
            if credit {
                sqlx::query("UPDATE post_secrets.anonymous_sessions SET created_at=extract(epoch FROM statement_timestamp())::bigint-259260,network_at=extract(epoch FROM statement_timestamp())::bigint-259260,address_at=extract(epoch FROM statement_timestamp())::bigint-120,activity_at=extract(epoch FROM statement_timestamp())::bigint,posts=5 WHERE token_hash=$1").bind(fingerprints.token.as_slice()).execute(&owner).await.unwrap();
                sqlx::query("INSERT INTO verification_private.credits(identity_key,remaining,expires_at) VALUES(sha256(convert_to('26chan:captcha-credit:browser:v1','UTF8')||$1::bytea),1,statement_timestamp()+interval '1 hour')").bind(fingerprints.token.as_slice()).execute(&owner).await.unwrap();
                assert_eq!(
                    verifier
                        .credit_receipt(&CreditRequest {
                            receipt: Some(&receipt_hash),
                            peer: &peer_hash,
                            fingerprints: &fingerprints,
                            board: &board,
                            target,
                            purpose: ChallengePurpose::Report,
                            context: &context,
                            request_time: chrono::Utc::now().timestamp()
                        })
                        .await
                        .unwrap()
                        .status,
                    "ready"
                );
            } else {
                let challenge = VerificationToken::generate(TokenKind::Challenge).unwrap();
                let answer = key.challenge_answer(&challenge, 6).unwrap();
                let answer_hash = key.answer_hash(&challenge, answer.as_str()).unwrap();
                assert!(
                    verifier
                        .issue_challenge(&NewChallenge {
                            token_hash: &key.token_hash(&challenge),
                            answer_hash: &answer_hash,
                            peer_hash: &peer_hash,
                            session_hash: &fingerprints.token,
                            purpose: ChallengePurpose::Report,
                            board: Some(&board),
                            target,
                            suspicious: false,
                            context_hash: &context
                        })
                        .await
                        .unwrap()
                        .issued
                );
                assert!(
                    verifier
                        .check_challenge(&ChallengeCheck {
                            token_hash: &key.token_hash(&challenge),
                            answer_hash: Some(&answer_hash),
                            peer_hash: &peer_hash,
                            session_hash: &fingerprints.token,
                            purpose: ChallengePurpose::Report,
                            board: Some(&board),
                            target,
                            context_hash: &context,
                            receipt_hash: &receipt_hash
                        })
                        .await
                        .unwrap()
                );
            }
            let actor = poster_key.public_report_rate_identity(peer);
            let public = &public;
            let board = board.as_str();
            let receipt = &receipt;
            let actor = &actor;
            let submit = |proof: bool, stale: bool| async move {
                let protected = ProtectedReportingContext {
                    source_cookies_absent: false,
                    session: PostingSession {
                        fingerprints,
                        minted: false,
                        now: chrono::Utc::now(),
                    },
                    challenge: if proof {
                        WriteChallengeProof::from_issuer(&receipt.encode(), peer_hash, context)
                    } else {
                        None
                    },
                };
                match revision {
                    Some(revision) => {
                        report_categorical_with_challenge(
                            public,
                            board,
                            target,
                            1,
                            if stale { revision + 1 } else { revision },
                            actor,
                            protected,
                        )
                        .await
                    }
                    None => {
                        report_with_challenge(
                            public,
                            board,
                            target,
                            "Owned report",
                            actor,
                            protected,
                        )
                        .await
                    }
                }
            };
            let missing = submit(false, false).await.unwrap_err();
            assert!(
                matches!(missing,StoreError::Database(ref e) if e.as_database_error().and_then(|e|e.code()).as_deref()==Some("P0166")),
                "{missing:?}"
            );
            if categorical {
                assert!(matches!(
                    submit(true, true).await,
                    Err(StoreError::Invalid(
                        "Report categories changed. Please reload the report form."
                    ))
                ));
            }
            assert!(board.starts_with("rc") && board[2..].bytes().all(|b| b.is_ascii_hexdigit()));
            let token_hex: String = fingerprints
                .token
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect();
            sqlx::query(sqlx::AssertSqlSafe(format!("ALTER TABLE post_secrets.anonymous_reports ADD CONSTRAINT {board}_late_report CHECK(token_hash<>decode('{token_hex}','hex')) NOT VALID"))).execute(&owner).await.unwrap();
            let failed = submit(true, false).await.unwrap_err();
            assert!(
                matches!(failed,StoreError::Database(ref e) if e.as_database_error().and_then(|e|e.code()).as_deref()==Some("23514")),
                "{failed:?}"
            );
            sqlx::query(sqlx::AssertSqlSafe(format!(
                "ALTER TABLE post_secrets.anonymous_reports DROP CONSTRAINT {board}_late_report"
            )))
            .execute(&owner)
            .await
            .unwrap();
            assert!(
                sqlx::query_scalar::<_, bool>(
                    "SELECT EXISTS(SELECT 1 FROM verification_private.receipts WHERE token_hash=$1)"
                )
                .bind(receipt_hash.as_slice())
                .fetch_one(&owner)
                .await
                .unwrap()
            );
            let reports: i64 =
                sqlx::query_scalar("SELECT count(*) FROM content.reports WHERE board=$1")
                    .bind(board)
                    .fetch_one(&owner)
                    .await
                    .unwrap();
            assert_eq!(reports, 0);
            let failed_history: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM verification_private.report_history WHERE actor_hash=$1",
            )
            .bind(actor.as_bytes().as_slice())
            .fetch_one(&owner)
            .await
            .unwrap();
            assert_eq!(
                failed_history, 0,
                "A failed report must not advance its exemption history"
            );
            let failed_epoch:(Option<Vec<u8>>,chrono::DateTime<chrono::Utc>)=sqlx::query_as("SELECT report_history_epoch,report_history_complete_after FROM verification_private.policy WHERE singleton").fetch_one(&owner).await.unwrap();
            assert_eq!(
                failed_epoch, prior_epoch,
                "Failed report must roll back epoch observation too"
            );

            if credit {
                let balance:i16=sqlx::query_scalar("SELECT remaining FROM verification_private.credits WHERE identity_key=sha256(convert_to('26chan:captcha-credit:browser:v1','UTF8')||$1::bytea)").bind(fingerprints.token.as_slice()).fetch_one(&owner).await.unwrap();
                assert_eq!(balance, 1);
            }
            submit(true, false).await.unwrap();
            let accepted_epoch: Vec<u8> = sqlx::query_scalar(
                "SELECT report_history_epoch FROM verification_private.policy WHERE singleton",
            )
            .fetch_one(&owner)
            .await
            .unwrap();
            assert_eq!(accepted_epoch, actor.report_history_epoch().as_slice());
            let replay = submit(true, false).await.unwrap_err();
            assert!(
                matches!(replay,StoreError::Database(ref e) if e.as_database_error().and_then(|e|e.code()).as_deref()==Some("P0166")),
                "{replay:?}"
            );
            let reports: i64 =
                sqlx::query_scalar("SELECT count(*) FROM content.reports WHERE board=$1")
                    .bind(board)
                    .fetch_one(&owner)
                    .await
                    .unwrap();
            assert_eq!(reports, 1);
            if categorical {
                let staff = PgPool::connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
                    .await
                    .unwrap();
                let cleared: i64 = sqlx::query_scalar("SELECT content.clear_report_group($1,$2,1)")
                    .bind(board)
                    .bind(target)
                    .fetch_one(&staff)
                    .await
                    .unwrap();
                assert_eq!(cleared, 1);
                let first_clear:chrono::DateTime<chrono::Utc>=sqlx::query_scalar("SELECT last_cleared_at FROM verification_private.report_history WHERE actor_hash=$1").bind(actor.as_bytes().as_slice()).fetch_one(&owner).await.unwrap();
                let repeated: i64 =
                    sqlx::query_scalar("SELECT content.clear_report_group($1,$2,1)")
                        .bind(board)
                        .bind(target)
                        .fetch_one(&staff)
                        .await
                        .unwrap();
                assert_eq!(repeated, 0);
                let same_clear:chrono::DateTime<chrono::Utc>=sqlx::query_scalar("SELECT last_cleared_at FROM verification_private.report_history WHERE actor_hash=$1").bind(actor.as_bytes().as_slice()).fetch_one(&owner).await.unwrap();
                assert_eq!(first_clear, same_clear);
                let inherited_actor =
                    poster_key.public_report_rate_identity("192.0.2.110".parse().unwrap());
                verifier.ensure_session(&inherited_fp, true).await.unwrap();
                let (inherited_receipt, inherited_context) = normal_report_receipt(
                    &verifier,
                    &key,
                    &inherited_fp,
                    &inherited_hash,
                    board,
                    target,
                )
                .await;
                report_categorical_with_challenge(
                    public,
                    board,
                    target,
                    1,
                    revision.unwrap(),
                    &inherited_actor,
                    ProtectedReportingContext {
                        source_cookies_absent: false,
                        session: PostingSession {
                            fingerprints: inherited_fp,
                            minted: false,
                            now: chrono::Utc::now(),
                        },
                        challenge: WriteChallengeProof::from_issuer(
                            &inherited_receipt.encode(),
                            inherited_hash,
                            inherited_context,
                        ),
                    },
                )
                .await
                .unwrap();
                let inherited_clear:chrono::DateTime<chrono::Utc>=sqlx::query_scalar("SELECT last_cleared_at FROM verification_private.report_history WHERE actor_hash=$1").bind(inherited_actor.as_bytes().as_slice()).fetch_one(&owner).await.unwrap();
                assert!(
                    inherited_clear >= first_clear,
                    "A new report against a cleared group records a new clear event"
                );
                staff.close().await;
            }
            if !categorical {
                let staff = PgPool::connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
                    .await
                    .unwrap();
                let role: String = sqlx::query_scalar("SELECT current_user::text")
                    .fetch_one(&staff)
                    .await
                    .unwrap();
                assert_eq!(role, "board_staff");
                let staff_actor =
                    poster_key.public_report_rate_identity("192.0.2.108".parse().unwrap());
                report(&staff, board, target, "Owned staff report", &staff_actor)
                    .await
                    .unwrap();
                staff.close().await;
                let disabled_actor = poster_key.public_report_rate_identity(disabled_peer);
                let before_disabled = report_with_anonymous_session(
                    public,
                    board,
                    target,
                    "Owned disabled-board report",
                    &disabled_actor,
                    PostingSession {
                        fingerprints: disabled_fp,
                        minted: true,
                        now: chrono::Utc::now(),
                    },
                )
                .await
                .unwrap_err();
                assert!(
                    matches!(before_disabled,StoreError::Database(ref e) if e.as_database_error().and_then(|e|e.code()).as_deref()==Some("P0166")),
                    "{before_disabled:?}"
                );
                sqlx::query("UPDATE content.boards SET captcha_enabled=false WHERE slug=$1")
                    .bind(board)
                    .execute(&owner)
                    .await
                    .unwrap();
                report_with_anonymous_session(
                    public,
                    board,
                    target,
                    "Owned disabled-board report",
                    &disabled_actor,
                    PostingSession {
                        fingerprints: disabled_fp,
                        minted: true,
                        now: chrono::Utc::now(),
                    },
                )
                .await
                .unwrap();
            }

            if credit {
                let balance:i16=sqlx::query_scalar("SELECT remaining FROM verification_private.credits WHERE identity_key=sha256(convert_to('26chan:captcha-credit:browser:v1','UTF8')||$1::bytea)").bind(fingerprints.token.as_slice()).fetch_one(&owner).await.unwrap();
                assert_eq!(balance, 0);
            }
        }
    };
    let outcome = tokio::spawn(work).await;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "ALTER TABLE post_secrets.anonymous_reports DROP CONSTRAINT IF EXISTS {board}_late_report"
    )))
    .execute(&owner)
    .await
    .unwrap();
    sqlx::query("SELECT content.set_report_catalog_active($1)")
        .bind(original)
        .execute(&owner)
        .await
        .unwrap();
    let mut tx = support::begin_cleanup_with_sessions(
        &owner,
        std::slice::from_ref(&board),
        &[fingerprints.token, disabled_fp.token, inherited_fp.token],
    )
    .await;
    for query in [
        "DELETE FROM content.reports WHERE board=$1",
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
    sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
        .bind(inherited_fp.token.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.peers WHERE peer_hash=$1")
        .bind(inherited_hash.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
        .bind(disabled_fp.token.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
        .bind(fingerprints.token.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.credits WHERE identity_key=sha256(convert_to('26chan:captcha-credit:browser:v1','UTF8')||$1::bytea)").bind(fingerprints.token.as_slice()).execute(&owner).await.unwrap();
    sqlx::query("DELETE FROM verification_private.peers WHERE peer_hash=$1")
        .bind(peer_hash.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    let actors: Vec<&[u8]> = history_actors
        .iter()
        .map(|actor| actor.as_slice())
        .collect();
    let retained:i64=sqlx::query_scalar("SELECT count(*) FROM verification_private.report_history WHERE actor_hash=ANY($1::bytea[])").bind(&actors).fetch_one(&owner).await.unwrap();
    if outcome.is_ok() {
        assert!(
            retained > 0,
            "Deleting reports and targets must not erase the challenge windows"
        );
    }
    sqlx::query(
        "DELETE FROM verification_private.report_history WHERE actor_hash=ANY($1::bytea[])",
    )
    .bind(&actors)
    .execute(&owner)
    .await
    .unwrap();
    public.close().await;
    verifier.close().await;
    owner.close().await;
    outcome.unwrap();
}

async fn normal_report_receipt(
    store: &VerificationStore,
    key: &VerificationKey,
    fp: &board_domain::anonymous_session::Fingerprints,
    peer: &[u8; 32],
    board: &str,
    target: i64,
) -> (ReceiptToken, [u8; 32]) {
    let context = key
        .form_context("report", Some(board), target, &"ab".repeat(32))
        .unwrap();
    let challenge = VerificationToken::generate(TokenKind::Challenge).unwrap();
    let answer = key.challenge_answer(&challenge, 6).unwrap();
    let answer_hash = key.answer_hash(&challenge, answer.as_str()).unwrap();
    let receipt = ReceiptToken::generate().unwrap();
    assert!(
        store
            .issue_challenge(&NewChallenge {
                token_hash: &key.token_hash(&challenge),
                answer_hash: &answer_hash,
                peer_hash: peer,
                session_hash: &fp.token,
                purpose: ChallengePurpose::Report,
                board: Some(board),
                target,
                suspicious: false,
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
                peer_hash: peer,
                session_hash: &fp.token,
                purpose: ChallengePurpose::Report,
                board: Some(board),
                target,
                context_hash: &context,
                receipt_hash: &receipt.storage_hash()
            })
            .await
            .unwrap()
    );
    (receipt, context)
}
