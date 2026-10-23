#![cfg(feature = "database-tests")]
mod support;
use board_domain::{
    anonymous_session::Capability,
    challenge_secrets::{ReceiptToken, TokenKind, VerificationKey, VerificationToken},
};
use board_store::{
    anonymous_session::PostingSession,
    media::MediaQueue,
    media_assets::OutputMetadata,
    media_intake::IntakeStore,
    post_media::NewAttachment,
    verification::{ChallengeCheck, ChallengePurpose, NewChallenge, VerificationStore},
    *,
};
use sqlx::PgPool;

// These deliberate late-failure constraints are fixture-owned schema. Attest
// their exact temporary shape, and require the original catalog on removal.
// Without this, admission correctly rejects the DDL before consuming a receipt.
async fn transfer_catalog(owner: &PgPool) -> (Vec<u8>, Vec<u8>) {
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_thread_transfer_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let ready: bool = sqlx::query_scalar("SELECT content.text_thread_transfer_ready()")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert!(ready, "Fixture starts with intact content authority");
    let hash = sqlx::query_scalar(
        "SELECT definition_hash FROM post_secrets.thread_transfer_catalog WHERE singleton",
    )
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let ready: bool = sqlx::query_scalar("SELECT content.restrictions_ready()")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert!(ready, "Fixture starts with intact restriction authority");
    let restriction_hash =
        sqlx::query_scalar("SELECT definition_hash FROM restrictions.catalog WHERE singleton")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    tx.rollback().await.unwrap();
    (hash, restriction_hash)
}
async fn qualified_constraint(
    owner: &PgPool,
    statements: &[String],
    restore: Option<&(Vec<u8>, Vec<u8>)>,
) {
    let mut tx = owner.begin().await.unwrap();
    if restore.is_none() {
        sqlx::query("SET LOCAL ROLE board_thread_transfer_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        let ready: bool = sqlx::query_scalar("SELECT content.text_thread_transfer_ready()")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert!(
            ready,
            "Do not attest pre-existing catalog drift as a fixture change"
        );
        sqlx::query("RESET ROLE").execute(&mut *tx).await.unwrap();
    }
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    if restore.is_none() {
        let ready: bool = sqlx::query_scalar("SELECT content.restrictions_ready()")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert!(ready, "Do not attest pre-existing restriction drift");
    }
    let previous_restriction_hash: Vec<u8> =
        sqlx::query_scalar("SELECT restrictions.catalog_hash()")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    sqlx::query("RESET ROLE").execute(&mut *tx).await.unwrap();
    for statement in statements {
        sqlx::query(sqlx::AssertSqlSafe(statement.clone()))
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    sqlx::query("SET LOCAL ROLE board_thread_transfer_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let hash: Vec<u8> = sqlx::query_scalar("SELECT post_secrets.transfer_catalog_hash()")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    if let Some(expected) = restore {
        assert_eq!(
            hash, expected.0,
            "Removing owned fault constraints restores the exact original catalog"
        );
    } else {
        let ready: bool = sqlx::query_scalar("SELECT content.text_thread_transfer_ready()")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert!(
            !ready,
            "Unattested fault injection must fail the production readiness guard"
        );
    }
    sqlx::query(
        "UPDATE post_secrets.thread_transfer_catalog SET definition_hash=$1 WHERE singleton",
    )
    .bind(&hash)
    .execute(&mut *tx)
    .await
    .unwrap();
    let ready: bool = sqlx::query_scalar("SELECT content.text_thread_transfer_ready()")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert!(
        ready,
        "Only the owned temporary schema is attested for the late-failure test"
    );
    // The private-media authority also seals post_media. Preserve the production
    // guard while admitting only this test's explicit late-failure constraint.
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let restriction_hash: Vec<u8> = sqlx::query_scalar("SELECT restrictions.catalog_hash()")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    if let Some(expected) = restore {
        assert_eq!(
            restriction_hash, expected.1,
            "Owned constraint removal restores restriction authority exactly"
        );
    } else if restriction_hash != previous_restriction_hash {
        let ready: bool = sqlx::query_scalar("SELECT content.restrictions_ready()")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert!(
            !ready,
            "Unattested media constraint must fail restriction readiness"
        );
    }
    sqlx::query("UPDATE restrictions.catalog SET definition_hash=$1 WHERE singleton")
        .bind(&restriction_hash)
        .execute(&mut *tx)
        .await
        .unwrap();
    let ready: bool = sqlx::query_scalar("SELECT content.restrictions_ready()")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert!(ready, "Only the owned fixture constraint is attested");
    tx.commit().await.unwrap();
}

#[tokio::test]
async fn public_post_requires_a_receipt_and_grants_credits_only_with_registered_activity() {
    run_scenario(4, 0, false, Some(true)).await;
    run_scenario(5, 1, false, Some(true)).await;
    run_scenario(5, 1, true, Some(true)).await;
    run_scenario(5, 0, false, Some(false)).await;
    run_scenario(5, 0, false, None).await;
}

async fn run_scenario(
    initial_posts: i16,
    expected_credits: i16,
    with_attachment: bool,
    request_eligible: Option<bool>,
) {
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
        sqlx::query_scalar("SELECT 'pc'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,text_only,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) VALUES($1,'Challenge test','Synthetic',2000,100,100,100,10,true,0,0,0)").bind(&board).execute(&owner).await.unwrap();
    assert!(board.starts_with("pc") && board[2..].bytes().all(|b| b.is_ascii_hexdigit()));
    let attachment = if with_attachment {
        sqlx::query("UPDATE content.boards SET text_only=false,image_limit=100 WHERE slug=$1")
            .bind(&board)
            .execute(&owner)
            .await
            .unwrap();
        let intake = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let upload = intake.reserve("owned-challenge.png").await.unwrap();
        intake
            .begin_upload(&upload.id, &upload.capability)
            .await
            .unwrap();
        intake
            .finish_upload(&upload.id, &upload.capability, 100)
            .await
            .unwrap();
        let claim = queue
            .claim()
            .await
            .unwrap()
            .expect("Owned disposable queue must contain the upload");
        assert_eq!(claim.id, upload.id);
        let lease = claim.lease_token.unwrap();
        let output = queue
            .prepare_output_with_variants(
                &upload.id,
                &lease,
                &OutputMetadata {
                    sha256: "a".repeat(64),
                    bytes: 123,
                    width: 10,
                    height: 20,
                },
                Some(&support::synthetic_upload_variants(&upload.id)),
            )
            .await
            .unwrap();
        queue
            .approve_output(&upload.id, &lease, &output.id)
            .await
            .unwrap();
        Some(NewAttachment {
            upload,
            spoiler: false,
        })
    } else {
        None
    };
    let owned_job = attachment.as_ref().map(|a| a.upload.id.clone());
    let peer = "192.0.2.102".parse().unwrap();
    let fingerprints = Capability::generate()
        .unwrap()
        .fingerprints(Some(peer), *b"US");
    let key = VerificationKey::parse(&ReceiptToken::generate().unwrap().encode()[3..]).unwrap();
    let peer_hash = key.network_identity(peer);
    let context_hash = key
        .form_context("thread", Some(&board), 0, &"ab".repeat(32))
        .unwrap();
    let receipt = ReceiptToken::generate().unwrap();
    let receipt_text = receipt.encode();
    let original_catalog = transfer_catalog(&owner).await;
    let work = {
        let original_catalog = original_catalog.clone();
        let owner = owner.clone();
        let public = public.clone();
        let verifier = verifier.clone();
        let board = board.clone();
        async move {
            verifier.ensure_session(&fingerprints, true).await.unwrap();
            sqlx::query("UPDATE post_secrets.anonymous_sessions SET created_at=extract(epoch FROM statement_timestamp())::bigint-259260,network_at=extract(epoch FROM statement_timestamp())::bigint-259260,environment_at=extract(epoch FROM statement_timestamp())::bigint-259260,address_at=extract(epoch FROM statement_timestamp())::bigint-120,activity_at=extract(epoch FROM statement_timestamp())::bigint,posts=$2 WHERE token_hash=$1").bind(fingerprints.token.as_slice()).bind(initial_posts).execute(&owner).await.unwrap();
            let direct=sqlx::query("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,1,'Anonymous','','Missing proof')").bind(&board).execute(&public).await.unwrap_err();
            assert_eq!(
                direct.as_database_error().and_then(|e| e.code()).as_deref(),
                Some("P0166")
            );
            let challenge = VerificationToken::generate(TokenKind::Challenge).unwrap();
            let answer = key.challenge_answer(&challenge, 6).unwrap();
            let answer_hash = key.answer_hash(&challenge, answer.as_str()).unwrap();
            let issued = verifier
                .issue_challenge(&NewChallenge {
                    token_hash: &key.token_hash(&challenge),
                    answer_hash: &answer_hash,
                    peer_hash: &peer_hash,
                    session_hash: &fingerprints.token,
                    purpose: ChallengePurpose::Thread,
                    board: Some(&board),
                    target: 0,
                    suspicious: false,
                    context_hash: &context_hash,
                })
                .await
                .unwrap();
            assert!(issued.issued);
            assert!(
                verifier
                    .check_challenge(&ChallengeCheck {
                        token_hash: &key.token_hash(&challenge),
                        answer_hash: Some(&answer_hash),
                        peer_hash: &peer_hash,
                        session_hash: &fingerprints.token,
                        purpose: ChallengePurpose::Thread,
                        board: Some(&board),
                        target: 0,
                        context_hash: &context_hash,
                        receipt_hash: &receipt.storage_hash()
                    })
                    .await
                    .unwrap()
            );
            let poster_key = support::fresh_key();
            let success_attempt = if with_attachment { 2 } else { 1 };
            for attempt in 0..=success_attempt + 1 {
                if with_attachment && attempt == 1 {
                    // Fail after the nested post INSERT consumed its receipt.
                    // This constraint names only our owned media job; unrelated
                    // uploads remain unaffected and teardown removes it on panic.
                    let job = &attachment.as_ref().unwrap().upload.id;
                    assert!(job.len() == 32 && job.bytes().all(|b| b.is_ascii_hexdigit()));
                    qualified_constraint(&owner,&[format!("ALTER TABLE content.post_media ADD CONSTRAINT {board}_late_failure CHECK(job_id<>'{job}')")],None).await;
                }
                let now = chrono::Utc::now();
                let result = create_post_with_challenge(
                    &public,
                    &board,
                    0,
                    &NewPost {
                        name: "Anonymous".into(),
                        subject: format!("Fixture subject {attempt}"),
                        comment: format!("Owned challenge post {attempt}"),
                        deletion_hash: "fixture".into(),
                        sage: false,
                    },
                    attachment.as_ref(),
                    ProtectedPostingContext {
                        request_facts: None,
                        anonymous: AnonymousPostingContext {
                            posting: PostingContext {
                                request_start: now,
                                peer: Some(peer),
                                op_password_proof: None,
                            },
                            session: PostingSession {
                                fingerprints,
                                minted: false,
                                now,
                            },
                        },
                        challenge: if attempt == 0 {
                            None
                        } else {
                            WriteChallengeProof::from_issuer(&receipt_text, peer_hash, context_hash)
                                .map(|proof| {
                                    use board_domain::content_admission::{
                                        classifiers::CreditRequest,
                                        network::ProviderValue::{
                                            Available as A, Unavailable as U,
                                        },
                                    };
                                    match request_eligible {
                                        Some(true) => proof.with_credit_request(CreditRequest {
                                            accept_language: A(Some(b"en")),
                                            accept: A(Some(b"text/html")),
                                            user_agent: A(None),
                                            referer: A(None),
                                        }),
                                        Some(false) => proof.with_credit_request(CreditRequest {
                                            accept_language: A(None),
                                            accept: A(None),
                                            user_agent: A(None),
                                            referer: A(None),
                                        }),
                                        None => proof.with_credit_request(CreditRequest {
                                            accept_language: U,
                                            accept: U,
                                            user_agent: U,
                                            referer: U,
                                        }),
                                    }
                                })
                        },
                    },
                    PostMetadata {
                        event_scores: false,
                        keys: PostIdentityKeys {
                            tripcode: None,
                            poster_id: Some(&poster_key),
                        },
                        country_database: None,
                        flag: "",
                        options: "",
                        spoiler: false,
                        drawing: None,
                    },
                )
                .await;
                if with_attachment && attempt == 1 {
                    let error = result.unwrap_err();
                    assert!(
                        matches!(error,StoreError::Database(ref e) if e.as_database_error().and_then(|d|d.code()).as_deref()==Some("23514")),
                        "{error:?}"
                    );
                    qualified_constraint(
                        &owner,
                        &[format!(
                            "ALTER TABLE content.post_media DROP CONSTRAINT {board}_late_failure"
                        )],
                        Some(&original_catalog),
                    )
                    .await;
                    let retained:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM verification_private.receipts WHERE token_hash=$1)").bind(receipt.storage_hash().as_slice()).fetch_one(&owner).await.unwrap();
                    assert!(retained, "Late attachment failure must restore the receipt");
                    let posts: i64 =
                        sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1")
                            .bind(&board)
                            .fetch_one(&owner)
                            .await
                            .unwrap();
                    assert_eq!(posts, 0, "Late attachment failure must roll back the post");
                    continue;
                }
                if attempt == success_attempt {
                    assert!(result.is_ok(), "valid proof must commit: {result:?}");
                } else if with_attachment && attempt > success_attempt {
                    assert!(
                        matches!(result, Err(StoreError::Conflict(_))),
                        "Consumed attachment must reject replay before receipt admission"
                    );
                    let retained:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM verification_private.receipts WHERE token_hash=$1)").bind(receipt.storage_hash().as_slice()).fetch_one(&owner).await.unwrap();
                    assert!(!retained, "Successful attachment must consume its receipt");
                } else {
                    let error = result.unwrap_err();
                    assert!(
                        matches!(error,StoreError::Database(ref e) if e.as_database_error().and_then(|d|d.code()).as_deref()==Some("P0166")),
                        "{error:?}"
                    );
                }
            }
            // An unfinished private handoff must prevent transaction commit,
            // even when all other post insertion work has succeeded.
            let mut pending = owner.begin().await.unwrap();
            let parent: i64 =
                sqlx::query_scalar("SELECT id FROM content.threads WHERE board=$1 LIMIT 1")
                    .bind(&board)
                    .fetch_one(&mut *pending)
                    .await
                    .unwrap();
            let pending_post:i64=sqlx::query_scalar("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,$2,'Anonymous','','Owned completion probe') RETURNING id").bind(&board).bind(parent).fetch_one(&mut *pending).await.unwrap();
            sqlx::query("INSERT INTO verification_private.post_handoffs(post_id,session_hash,peer_hash,may_grant_credit,registration_xid) VALUES($1,$2,$3,false,pg_current_xact_id())").bind(pending_post).bind(fingerprints.token.as_slice()).bind(peer_hash.as_slice()).execute(&mut *pending).await.unwrap();
            let incomplete =
                sqlx::query("SET CONSTRAINTS content.post_challenge_completion IMMEDIATE")
                    .execute(&mut *pending)
                    .await
                    .unwrap_err();
            pending.rollback().await.unwrap();
            assert_eq!(
                incomplete
                    .as_database_error()
                    .and_then(|e| e.code())
                    .as_deref(),
                Some("P0166")
            );
            let mut drift = owner.begin().await.unwrap();
            sqlx::query("ALTER TABLE content.posts DISABLE TRIGGER a00_public_post_challenge")
                .execute(&mut *drift)
                .await
                .unwrap();
            sqlx::query("SET LOCAL ROLE board_anonymous_owner")
                .execute(&mut *drift)
                .await
                .unwrap();
            let ready: bool = sqlx::query_scalar("SELECT content.public_challenge_ready()")
                .fetch_one(&mut *drift)
                .await
                .unwrap();
            drift.rollback().await.unwrap();
            assert!(!ready, "readiness must reject a disabled insertion guard");
            assert!(
                sqlx::query_scalar::<_, bool>("SELECT content.public_challenge_ready()")
                    .fetch_one(&public)
                    .await
                    .unwrap()
            );
            let posts: i64 =
                sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1")
                    .bind(&board)
                    .fetch_one(&owner)
                    .await
                    .unwrap();
            assert_eq!(posts, 1);
            let handoffs: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM verification_private.post_handoffs WHERE session_hash=$1",
            )
            .bind(fingerprints.token.as_slice())
            .fetch_one(&owner)
            .await
            .unwrap();
            assert_eq!(handoffs, 0);
            let remaining:Option<i16>=sqlx::query_scalar("SELECT remaining FROM verification_private.credits WHERE identity_key=sha256(convert_to('26chan:captcha-credit:browser:v1','UTF8')||$1::bytea)").bind(fingerprints.token.as_slice()).fetch_optional(&owner).await.unwrap();
            assert_eq!(remaining.unwrap_or(0), expected_credits);
            if expected_credits > 0 && !with_attachment {
                let parent: i64 =
                    sqlx::query_scalar("SELECT id FROM content.threads WHERE board=$1 LIMIT 1")
                        .bind(&board)
                        .fetch_one(&owner)
                        .await
                        .unwrap();
                let context = key
                    .form_context("reply", Some(&board), parent, &"cd".repeat(32))
                    .unwrap();
                let claims = [
                    ReceiptToken::generate().unwrap(),
                    ReceiptToken::generate().unwrap(),
                ];
                for receipt in &claims {
                    let issued = verifier
                        .credit_receipt(&board_store::verification::CreditRequest {
                            receipt: Some(&receipt.storage_hash()),
                            peer: &peer_hash,
                            fingerprints: &fingerprints,
                            board: &board,
                            target: parent,
                            purpose: ChallengePurpose::Reply,
                            context: &context,
                            request_time: chrono::Utc::now().timestamp(),
                        })
                        .await
                        .unwrap();
                    assert_eq!(issued.status, "ready");
                    assert_eq!(
                        issued.remaining, 1,
                        "Claims must not spend a credit before a protected write"
                    );
                }
                let token_hex: String = fingerprints
                    .token
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect();
                // This owned constraint fails after finish_post_challenge has
                // spent the credit and before anonymous registration commits.
                qualified_constraint(&owner,&[format!("ALTER TABLE post_secrets.anonymous_posts ADD CONSTRAINT {board}_credit_failure CHECK(token_hash<>decode('{token_hex}','hex')) NOT VALID")],None).await;
                let public = &public;
                let board = board.as_str();
                let poster_key = &poster_key;
                let claims = &claims;
                let submit = |i: usize| async move {
                    let now = chrono::Utc::now();
                    create_post_with_challenge(
                        public,
                        board,
                        parent,
                        &NewPost {
                            name: "Anonymous".into(),
                            subject: String::new(),
                            comment: format!("Owned credit reply {i}"),
                            deletion_hash: "fixture".into(),
                            sage: false,
                        },
                        None,
                        ProtectedPostingContext {
                            request_facts: None,
                            anonymous: AnonymousPostingContext {
                                posting: PostingContext {
                                    request_start: now,
                                    peer: Some(peer),
                                    op_password_proof: None,
                                },
                                session: PostingSession {
                                    fingerprints,
                                    minted: false,
                                    now,
                                },
                            },
                            challenge: WriteChallengeProof::from_issuer(
                                &claims[i].encode(),
                                peer_hash,
                                context,
                            ),
                        },
                        PostMetadata {
                            event_scores: false,
                            keys: PostIdentityKeys {
                                tripcode: None,
                                poster_id: Some(poster_key),
                            },
                            country_database: None,
                            flag: "",
                            options: "",
                            spoiler: false,
                            drawing: None,
                        },
                    )
                    .await
                };
                // Entitlements are checked again at the protected write, not
                // retained just because both claims were previously issued.
                sqlx::query("UPDATE content.boards SET captcha_allow_bypass=false WHERE slug=$1")
                    .bind(board)
                    .execute(&owner)
                    .await
                    .unwrap();
                let denied = submit(0).await.unwrap_err();
                assert!(
                    matches!(denied,StoreError::Database(ref e) if e.as_database_error().and_then(|e|e.code()).as_deref()==Some("P0166")),
                    "{denied:?}"
                );
                sqlx::query("UPDATE content.boards SET captcha_allow_bypass=true WHERE slug=$1")
                    .bind(board)
                    .execute(&owner)
                    .await
                    .unwrap();
                sqlx::query("INSERT INTO verification_private.peer_risk(peer_hash,score,expires_at) VALUES($1,79,statement_timestamp()+interval '1 hour')").bind(peer_hash.as_slice()).execute(&owner).await.unwrap();
                let denied = submit(0).await.unwrap_err();
                assert!(
                    matches!(denied,StoreError::Database(ref e) if e.as_database_error().and_then(|e|e.code()).as_deref()==Some("P0166")),
                    "{denied:?}"
                );
                sqlx::query("DELETE FROM verification_private.peer_risk WHERE peer_hash=$1")
                    .bind(peer_hash.as_slice())
                    .execute(&owner)
                    .await
                    .unwrap();
                let credit_expiry:chrono::DateTime<chrono::Utc>=sqlx::query_scalar("UPDATE verification_private.credits SET expires_at=statement_timestamp()-interval '1 second' WHERE identity_key=sha256(convert_to('26chan:captcha-credit:browser:v1','UTF8')||$1::bytea) RETURNING expires_at").bind(fingerprints.token.as_slice()).fetch_one(&owner).await.unwrap();
                let denied = submit(0).await.unwrap_err();
                assert!(
                    matches!(denied,StoreError::Database(ref e) if e.as_database_error().and_then(|e|e.code()).as_deref()==Some("P0166")),
                    "{denied:?}"
                );
                sqlx::query("UPDATE verification_private.credits SET expires_at=$2 WHERE identity_key=sha256(convert_to('26chan:captcha-credit:browser:v1','UTF8')||$1::bytea)").bind(fingerprints.token.as_slice()).bind(credit_expiry+chrono::Duration::hours(1)).execute(&owner).await.unwrap();
                sqlx::query("UPDATE verification_private.receipts SET credit_identity=decode(repeat('a7',32),'hex') WHERE token_hash=$1").bind(claims[0].storage_hash().as_slice()).execute(&owner).await.unwrap();
                let denied = submit(0).await.unwrap_err();
                assert!(
                    matches!(denied,StoreError::Database(ref e) if e.as_database_error().and_then(|e|e.code()).as_deref()==Some("P0166")),
                    "{denied:?}"
                );
                sqlx::query("UPDATE verification_private.receipts SET credit_identity=sha256(convert_to('26chan:captcha-credit:browser:v1','UTF8')||$2::bytea) WHERE token_hash=$1").bind(claims[0].storage_hash().as_slice()).bind(fingerprints.token.as_slice()).execute(&owner).await.unwrap();
                let failed = submit(0).await.unwrap_err();
                assert!(
                    matches!(failed,StoreError::Database(ref e) if e.as_database_error().and_then(|e|e.code()).as_deref()==Some("23514")),
                    "{failed:?}"
                );
                qualified_constraint(&owner,&[format!("ALTER TABLE post_secrets.anonymous_posts DROP CONSTRAINT {board}_credit_failure")],Some(&original_catalog)).await;
                let restored:i16=sqlx::query_scalar("SELECT remaining FROM verification_private.credits WHERE identity_key=sha256(convert_to('26chan:captcha-credit:browser:v1','UTF8')||$1::bytea)").bind(fingerprints.token.as_slice()).fetch_one(&owner).await.unwrap();
                assert_eq!(
                    restored, 1,
                    "Late registration failure must restore the spent credit"
                );
                let retained:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM verification_private.receipts WHERE token_hash=$1)").bind(claims[0].storage_hash().as_slice()).fetch_one(&owner).await.unwrap();
                assert!(retained, "Late registration failure must restore the claim");
                let (left, right) = tokio::join!(submit(0), submit(1));
                assert_eq!(
                    usize::from(left.is_ok()) + usize::from(right.is_ok()),
                    1,
                    "Only one concurrent write can spend the last credit"
                );
                let denied = if let Err(error) = left {
                    error
                } else {
                    right.unwrap_err()
                };
                assert!(
                    matches!(denied,StoreError::Database(ref e) if e.as_database_error().and_then(|e|e.code()).as_deref()==Some("P0166")),
                    "{denied:?}"
                );
                let final_credit:i16=sqlx::query_scalar("SELECT remaining FROM verification_private.credits WHERE identity_key=sha256(convert_to('26chan:captcha-credit:browser:v1','UTF8')||$1::bytea)").bind(fingerprints.token.as_slice()).fetch_one(&owner).await.unwrap();
                assert_eq!(
                    final_credit, 0,
                    "Credit-backed posts must not grant new credits"
                );
                let final_posts: i64 =
                    sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1")
                        .bind(board)
                        .fetch_one(&owner)
                        .await
                        .unwrap();
                assert_eq!(final_posts, 2);
            }
        }
    };
    let outcome = tokio::spawn(work).await;
    let mut cleanup = vec![format!(
        "ALTER TABLE post_secrets.anonymous_posts DROP CONSTRAINT IF EXISTS {board}_credit_failure"
    )];
    if with_attachment {
        cleanup.push(format!(
            "ALTER TABLE content.post_media DROP CONSTRAINT IF EXISTS {board}_late_failure"
        ));
    }
    qualified_constraint(&owner, &cleanup, Some(&original_catalog)).await;
    let mut tx = support::begin_cleanup_with_sessions(
        &owner,
        std::slice::from_ref(&board),
        &[fingerprints.token],
    )
    .await;
    for query in [
        "DELETE FROM content.post_media WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM post_secrets.posting_thread_actions WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(query)
            .bind(&board)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();
    sqlx::query("DELETE FROM verification_private.credits WHERE identity_key=sha256(convert_to('26chan:captcha-credit:browser:v1','UTF8')||$1::bytea)").bind(fingerprints.token.as_slice()).execute(&owner).await.unwrap();
    sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
        .bind(fingerprints.token.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.peer_risk WHERE peer_hash=$1")
        .bind(peer_hash.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.peers WHERE peer_hash=$1")
        .bind(peer_hash.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    if let Some(job) = owned_job {
        sqlx::query("DELETE FROM media.assets WHERE job_id=$1")
            .bind(&job)
            .execute(&owner)
            .await
            .unwrap();
        sqlx::query("DELETE FROM media.jobs WHERE id=$1")
            .bind(&job)
            .execute(&owner)
            .await
            .unwrap();
    }
    public.close().await;
    verifier.close().await;
    owner.close().await;
    outcome.unwrap();
}
