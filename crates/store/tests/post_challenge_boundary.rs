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

#[tokio::test]
async fn public_post_requires_a_receipt_and_grants_credits_only_with_registered_activity() {
    run_scenario(4, 0, false).await;
    run_scenario(5, 1, false).await;
    run_scenario(5, 1, true).await;
}

async fn run_scenario(initial_posts: i16, expected_credits: i16, with_attachment: bool) {
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
            .prepare_output(
                &upload.id,
                &lease,
                &OutputMetadata {
                    sha256: "a".repeat(64),
                    bytes: 123,
                    width: 10,
                    height: 20,
                },
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
    let work = {
        let owner = owner.clone();
        let public = public.clone();
        let verifier = verifier.clone();
        let board = board.clone();
        async move {
            verifier.ensure_session(&fingerprints, true).await.unwrap();
            sqlx::query("UPDATE post_secrets.anonymous_sessions SET created_at=extract(epoch FROM statement_timestamp())::bigint-259260,network_at=extract(epoch FROM statement_timestamp())::bigint-259260,address_at=extract(epoch FROM statement_timestamp())::bigint-120,activity_at=extract(epoch FROM statement_timestamp())::bigint,posts=$2 WHERE token_hash=$1").bind(fingerprints.token.as_slice()).bind(initial_posts).execute(&owner).await.unwrap();
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
                    sqlx::query(sqlx::AssertSqlSafe(format!("ALTER TABLE content.post_media ADD CONSTRAINT {board}_late_failure CHECK(job_id<>'{job}')"))).execute(&owner).await.unwrap();
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
                        },
                    },
                    PostMetadata {
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
                    sqlx::query(sqlx::AssertSqlSafe(format!(
                        "ALTER TABLE content.post_media DROP CONSTRAINT {board}_late_failure"
                    )))
                    .execute(&owner)
                    .await
                    .unwrap();
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
        }
    };
    let outcome = tokio::spawn(work).await;
    if with_attachment {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "ALTER TABLE content.post_media DROP CONSTRAINT IF EXISTS {board}_late_failure"
        )))
        .execute(&owner)
        .await
        .unwrap();
    }
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
