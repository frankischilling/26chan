#![cfg(feature = "database-tests")]
use board_domain::{
    anonymous_session::Capability,
    challenge_secrets::{PassSessionToken, ReceiptToken, VerificationKey},
};
use board_store::{
    anonymous_session::PostingSession,
    verification::{VerificationStore, vip::LoginRequest},
    *,
};
use sqlx::PgPool;

fn refused(error: &StoreError) -> bool {
    matches!(error, StoreError::Database(error)
        if error.as_database_error().and_then(|e| e.code()).as_deref()==Some("P0239"))
}

#[tokio::test]
async fn verified_text_posts_bind_identity_rollback_replay_and_revocation() {
    qualify_posting(false, false).await;
}
#[tokio::test]
async fn verified_posts_require_and_consume_actual_pass_bypass() {
    qualify_posting(true, false).await;
}
#[tokio::test]
async fn verified_image_posts_keep_media_capabilities_and_pass_admission() {
    qualify_posting(false, true).await;
    qualify_posting(true, true).await;
}
static TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
async fn qualify_posting(enabled: bool, images: bool) {
    let _guard = TEST.lock().await;
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
    let material = ReceiptToken::generate().unwrap().encode();
    let key = VerificationKey::parse(&material[3..]).unwrap();
    let poster_key = board_domain::poster_id::PosterIdKey::parse(&material[3..]).unwrap();
    let identifier = key.vip_identifier("owned-vip").unwrap();
    let board = format!("v{}", &material[3..12]);
    let peer = "192.0.2.131".parse().unwrap();
    let peer_hash = key.network_identity(peer);
    let fingerprints = Capability::generate()
        .unwrap()
        .fingerprints(Some(peer), *b"US");
    verifier.ensure_session(&fingerprints, true).await.unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,text_only,captcha_enabled,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,user_thread_limit,forced_anon,user_ids,robot9000,require_subject) VALUES($1,'Owned VIP posting','Synthetic',2000,100,100,100,10,true,false,0,0,0,100,false,true,true,false)")
        .bind(&board).execute(&owner).await.unwrap();
    let pass_session = PassSessionToken::generate().unwrap();
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let pass_account: i64 = sqlx::query_scalar("INSERT INTO verification_private.pass_entitlements(token_hash,pin_hash,status,purchased_at,expires_at) VALUES($1,repeat('owned-placeholder',3),0,'2020-01-02',clock_timestamp()+interval '1 year') RETURNING id")
        .bind(pass_session.storage_hash().as_slice()).fetch_one(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO verification_private.pass_sessions VALUES($1,$2,1,clock_timestamp(),clock_timestamp()+interval '1 day')")
        .bind(pass_session.storage_hash().as_slice()).bind(pass_account).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let account: i64 = sqlx::query_scalar("SELECT content.vip_manage($1,'Owned #VIP','$argon2id$v=19$m=19456,t=2,p=1$'||repeat('a',22)||'$'||repeat('b',43),true,true)")
        .bind(identifier.as_slice()).fetch_one(&owner).await.unwrap();
    let work = {
        let owner = owner.clone();
        let public = public.clone();
        let verifier = verifier.clone();
        let board = board.clone();
        async move {
            let issue = |target| {
                let pass_session = pass_session.storage_hash();
                let key = &key;
                let verifier = verifier.clone();
                let board = board.clone();
                async move {
                    let ticket = ReceiptToken::generate().unwrap();
                    let receipt = ReceiptToken::generate().unwrap();
                    let context = ReceiptToken::generate().unwrap().storage_hash();
                    let pass_receipt = ReceiptToken::generate().unwrap();
                    let purpose = if target == 0 { "thread" } else { "reply" };
                    let pass_context = key
                        .form_context(purpose, Some(&board), target, &"ab".repeat(32))
                        .unwrap();
                    if enabled {
                        let networks = key.pass_networks(peer).unwrap();
                        assert_eq!(
                            verifier
                                .pass_write_receipt(
                                    &board_store::verification::pass::WriteRequest {
                                        pass_session: &pass_session,
                                        receipt: &pass_receipt.storage_hash(),
                                        networks: &networks,
                                        anonymous: &fingerprints.token,
                                        context: &pass_context,
                                        purpose,
                                        board: &board,
                                        target,
                                    }
                                )
                                .await
                                .unwrap()
                                .status,
                            "ready"
                        );
                    }
                    let lookup = verifier
                        .begin_vip_login(&LoginRequest {
                            identifier: &identifier,
                            peer: &peer_hash,
                            ticket: &ticket.storage_hash(),
                            anonymous: &fingerprints.token,
                            context: &context,
                            board: &board,
                            target,
                            pass_proof: enabled
                                .then_some((&pass_receipt.storage_hash(), &pass_context)),
                        })
                        .await
                        .unwrap();
                    assert_eq!(lookup.status, "verify");
                    assert_eq!(
                        verifier
                            .finish_vip_login(
                                &ticket.storage_hash(),
                                &peer_hash,
                                true,
                                &receipt.storage_hash()
                            )
                            .await
                            .unwrap(),
                        "ready"
                    );
                    (receipt, context, pass_receipt, pass_context)
                }
            };
            let submit = |parent,
                          comment: &'static str,
                          proof,
                          challenge,
                          attachment: Option<std::sync::Arc<post_media::NewAttachment>>,
                          deletion_hash: String| {
                let public = public.clone();
                let board = board.clone();
                let poster_key = &poster_key;
                async move {
                    let now = chrono::Utc::now();
                    create_verified_post_with_network(
                        &public,
                        &board,
                        parent,
                        &NewPost {
                            name: "User #not-vip".into(),
                            subject: "Forced subject".into(),
                            comment: comment.into(),
                            deletion_hash,
                            sage: false,
                        },
                        attachment.as_deref(),
                        VerifiedPostingContext {
                            network: NetworkPostingContext {
                                hints: None,
                                asn_database: None,
                                posting: ProtectedPostingContext {
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
                                    challenge,
                                },
                            },
                            vip: proof,
                        },
                        PostMetadata {
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
                }
            };
            let op = submit(
                0,
                "Owned duplicate originality control",
                None,
                None,
                None,
                "fixture".into(),
            )
            .await
            .unwrap();
            // Text-only opening posts require a subject. Enable forced-anonymous
            // identity only after creating the owned target thread.
            sqlx::query("UPDATE content.boards SET forced_anon=true,captcha_enabled=$2,captcha_allow_bypass=true,image_limit=100,text_only=$3 WHERE slug=$1")
                .bind(&board)
                .bind(enabled)
                .bind(!images)
                .execute(&owner)
                .await
                .unwrap();
            let upload = if images {
                let mut history = owner.begin().await.unwrap();
                sqlx::query("SET LOCAL ROLE board_attachment_owner")
                    .execute(&mut *history)
                    .await
                    .unwrap();
                sqlx::query("INSERT INTO post_secrets.image_history_scopes(scope) VALUES($1)")
                    .bind(&board)
                    .execute(&mut *history)
                    .await
                    .unwrap();
                sqlx::query("INSERT INTO post_secrets.image_upload_history(post_id,scope,digest,accepted_at) VALUES($1,$2,$3,clock_timestamp())")
                    .bind(op).bind(&board).bind(vec![7_u8;16]).execute(&mut *history).await.unwrap();
                history.commit().await.unwrap();
                sqlx::query("UPDATE content.boards SET image_history_scope=$1 WHERE slug=$1")
                    .bind(&board)
                    .execute(&owner)
                    .await
                    .unwrap();
                Some(std::sync::Arc::new(owned_upload(&board, op).await))
            } else {
                None
            };
            let (receipt, context, pass_receipt, pass_context) = issue(op).await;
            let challenge = || {
                enabled.then(|| {
                    WriteChallengeProof::from_issuer(
                        &pass_receipt.encode(),
                        peer_hash,
                        pass_context,
                    )
                    .unwrap()
                })
            };
            let bad_peer = submit(
                op,
                "Owned duplicate originality control",
                WriteVipProof::from_issuer(&receipt.encode(), [42; 32], context),
                challenge(),
                upload.clone(),
                "fixture".into(),
            )
            .await
            .unwrap_err();
            assert!(refused(&bad_peer), "{bad_peer:?}");
            let proof = || WriteVipProof::from_issuer(&receipt.encode(), peer_hash, context);
            let rule: i64 = sqlx::query_scalar(
                "INSERT INTO admission.rules(board,pattern) VALUES($1,'forbidden') RETURNING id",
            )
            .bind(&board)
            .fetch_one(&owner)
            .await
            .unwrap();
            let blocked = submit(
                op,
                "owned vip forbidden content",
                proof(),
                challenge(),
                upload.clone(),
                "fixture".into(),
            )
            .await
            .unwrap_err();
            assert!(
                matches!(blocked, StoreError::ContentRejected(_)),
                "{blocked:?}"
            );
            sqlx::query("DELETE FROM admission.hits WHERE rule_id=$1")
                .bind(rule)
                .execute(&owner)
                .await
                .unwrap();
            sqlx::query("DELETE FROM admission.rules WHERE id=$1")
                .bind(rule)
                .execute(&owner)
                .await
                .unwrap();
            if images {
                // Exact-file blacklist exemption must not become staff authority.
                sqlx::query("INSERT INTO admission.file_rules(board,digest,active,mode,ban_days,public_reason,description) VALUES($1,$2,true,0,0,'Owned rejection','Owned VIP exemption')")
                    .bind(&board).bind(vec![7_u8;16]).execute(&owner).await.unwrap();
            }
            // VIP and Pass do not exempt the earlier five-minute comment gate.
            // The refused attempt must leave both proofs and any upload usable.
            let duplicate = submit(
                op,
                "Owned duplicate originality control",
                proof(),
                challenge(),
                upload.clone(),
                "fixture".into(),
            )
            .await
            .unwrap_err();
            assert!(
                matches!(duplicate, StoreError::ContentRejected(ref message)
                if message.ends_with("before posting a duplicate reply.")),
                "{duplicate:?}"
            );
            // Keep the original R9K originality record, but age the source
            // duplicate-comment candidate beyond its separate finite window.
            sqlx::query("UPDATE content.posts SET created_at=clock_timestamp()-interval '301 seconds' WHERE id=$1 AND board=$2")
                .bind(op).bind(&board).execute(&owner).await.unwrap();
            // The post INSERT consumes its receipt before deletion storage. This
            // deliberately oversized owned hash causes a later SQL failure.
            let late = submit(
                op,
                "Owned duplicate originality control",
                proof(),
                challenge(),
                upload.clone(),
                "x".repeat(257),
            )
            .await
            .unwrap_err();
            assert!(
                matches!(late, StoreError::Database(ref error)
                if error.as_database_error().and_then(|e|e.code()).as_deref()==Some("23514")),
                "{late:?}"
            );
            let posted = submit(
                op,
                "Owned duplicate originality control",
                proof(),
                challenge(),
                upload.clone(),
                "fixture".into(),
            )
            .await
            .unwrap();
            let row: (String, Option<String>, Option<String>, Option<String>, bool, String) =
                sqlx::query_as("SELECT name,trip,capcode,poster_id,staff_authorized_limits,subject FROM content.posts WHERE id=$1")
                .bind(posted).fetch_one(&public).await.unwrap();
            assert_eq!(
                row,
                (
                    "Owned #VIP".into(),
                    None,
                    Some("verified".into()),
                    Some("Verified".into()),
                    false,
                    String::new()
                )
            );
            let replay = submit(
                op,
                "Owned later replay",
                proof(),
                challenge(),
                upload.clone(),
                "fixture".into(),
            )
            .await
            .unwrap_err();
            assert!(
                refused(&replay)
                    || matches!(&replay, StoreError::Database(e) if enabled && e.as_database_error().and_then(|e|e.code()).as_deref()==Some("P0166")),
                "{replay:?}"
            );
            let (_, _, ordinary_receipt, ordinary_context) = issue(op).await;
            let ordinary_challenge = enabled.then(|| {
                WriteChallengeProof::from_issuer(
                    &ordinary_receipt.encode(),
                    peer_hash,
                    ordinary_context,
                )
                .unwrap()
            });
            let ordinary = submit(
                op,
                "Different ordinary context after a VIP post",
                None,
                ordinary_challenge,
                None,
                "fixture".into(),
            )
            .await
            .unwrap();
            let ordinary_row: (String, Option<String>) =
                sqlx::query_as("SELECT name,capcode FROM content.posts WHERE id=$1")
                    .bind(ordinary)
                    .fetch_one(&public)
                    .await
                    .unwrap();
            assert_eq!(ordinary_row, ("Anonymous".into(), None));
            let (revoked, context, revoked_pass, revoked_context) = issue(op).await;
            sqlx::query("SELECT content.vip_manage($1,NULL,NULL,false,false)")
                .bind(identifier.as_slice())
                .execute(&owner)
                .await
                .unwrap();
            let revoked = submit(
                op,
                "Owned revoked attempt",
                WriteVipProof::from_issuer(&revoked.encode(), peer_hash, context),
                enabled.then(|| {
                    WriteChallengeProof::from_issuer(
                        &revoked_pass.encode(),
                        peer_hash,
                        revoked_context,
                    )
                    .unwrap()
                }),
                None,
                "fixture".into(),
            )
            .await
            .unwrap_err();
            assert!(refused(&revoked), "{revoked:?}");
            sqlx::query("SELECT content.vip_manage($1,NULL,NULL,true,false)")
                .bind(identifier.as_slice())
                .execute(&owner)
                .await
                .unwrap();
            sqlx::query("UPDATE content.boards SET reply_limit=2,bump_limit=2 WHERE slug=$1")
                .bind(&board)
                .execute(&owner)
                .await
                .unwrap();
            sqlx::query(
                "UPDATE content.threads SET sticky=true,undead=true WHERE board=$1 AND id=$2",
            )
            .bind(&board)
            .bind(op)
            .execute(&owner)
            .await
            .unwrap();
            let (prune, context, prune_pass, prune_context) = issue(op).await;
            let pruned_post = submit(
                op,
                "Owned verified reply prunes the oldest sticky reply",
                WriteVipProof::from_issuer(&prune.encode(), peer_hash, context),
                enabled.then(|| {
                    WriteChallengeProof::from_issuer(&prune_pass.encode(), peer_hash, prune_context)
                        .unwrap()
                }),
                None,
                "fixture".into(),
            )
            .await
            .unwrap();
            let old: bool = sqlx::query_scalar("SELECT deleted FROM content.posts WHERE id=$1")
                .bind(posted)
                .fetch_one(&owner)
                .await
                .unwrap();
            assert!(old);
            let registered: Vec<i64> = sqlx::query_scalar("SELECT post_id FROM post_secrets.anonymous_posts WHERE post_id=ANY($1) ORDER BY post_id")
                .bind(vec![posted,pruned_post]).fetch_all(&owner).await.unwrap();
            assert_eq!(registered, vec![pruned_post]);
            sqlx::query(
                "UPDATE content.threads SET sticky=false,undead=false WHERE board=$1 AND id=$2",
            )
            .bind(&board)
            .bind(op)
            .execute(&owner)
            .await
            .unwrap();
            sqlx::query("UPDATE content.boards SET reply_limit=100,bump_limit=100,archive_retention_seconds=3600,autoarchive_cap=3 WHERE slug=$1")
                .bind(&board).execute(&owner).await.unwrap();
            let (last, context, last_pass, last_context) = issue(op).await;
            let final_post = submit(
                op,
                "Owned verified reply archives this thread",
                WriteVipProof::from_issuer(&last.encode(), peer_hash, context),
                enabled.then(|| {
                    WriteChallengeProof::from_issuer(&last_pass.encode(), peer_hash, last_context)
                        .unwrap()
                }),
                None,
                "fixture".into(),
            )
            .await
            .unwrap();
            let snapshot = thread_snapshot(&public, &board, op).await.unwrap();
            assert!(snapshot.thread.archived_at.is_some());
            assert_eq!(snapshot.posts.last().unwrap().id, final_post);
            assert_eq!(
                snapshot.posts.last().unwrap().capcode.as_deref(),
                Some("verified")
            );
            let mut tx = owner.begin().await.unwrap();
            sqlx::query("SET LOCAL ROLE board_anonymous_owner")
                .execute(&mut *tx)
                .await
                .unwrap();
            let retained: i64 =
                sqlx::query_scalar("SELECT count(*) FROM verification_private.vip_post_handoffs")
                    .fetch_one(&mut *tx)
                    .await
                    .unwrap();
            assert_eq!(retained, 0);
            tx.rollback().await.unwrap();
            if images {
                sqlx::query("UPDATE content.boards SET upload_board=true WHERE slug=$1")
                    .bind(&board)
                    .execute(&owner)
                    .await
                    .unwrap();
                let file = std::sync::Arc::new(owned_upload(&board, 0).await);
                let (receipt, context, pass, pass_context) = issue(0).await;
                let proof = || WriteVipProof::from_issuer(&receipt.encode(), peer_hash, context);
                let challenge = || {
                    enabled.then(|| {
                        WriteChallengeProof::from_issuer(&pass.encode(), peer_hash, pass_context)
                            .unwrap()
                    })
                };
                let late = submit(
                    0,
                    "Owned upload history rollback",
                    proof(),
                    challenge(),
                    Some(file.clone()),
                    "x".repeat(257),
                )
                .await
                .unwrap_err();
                assert!(
                    matches!(late, StoreError::Database(ref e) if e.as_database_error().and_then(|e|e.code()).as_deref()==Some("23514")),
                    "{late:?}"
                );
                let mut check = owner.begin().await.unwrap();
                sqlx::query("SET LOCAL ROLE board_attachment_owner")
                    .execute(&mut *check)
                    .await
                    .unwrap();
                let count: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM post_secrets.image_upload_history WHERE scope=$1",
                )
                .bind(&board)
                .fetch_one(&mut *check)
                .await
                .unwrap();
                assert_eq!(count, 1, "Late failure must not retain upload history");
                check.rollback().await.unwrap();
                let posted = submit(
                    0,
                    "Owned upload history rollback",
                    proof(),
                    challenge(),
                    Some(file),
                    "fixture".into(),
                )
                .await
                .unwrap();
                let mut check = owner.begin().await.unwrap();
                sqlx::query("SET LOCAL ROLE board_attachment_owner")
                    .execute(&mut *check)
                    .await
                    .unwrap();
                let count: i64 = sqlx::query_scalar("SELECT count(*) FROM post_secrets.image_upload_history WHERE scope=$1 AND post_id=$2")
                    .bind(&board).bind(posted).fetch_one(&mut *check).await.unwrap();
                assert_eq!(
                    count, 1,
                    "Checked badges bypass lookup but accepted uploads still record history"
                );
                check.rollback().await.unwrap();
            }
        }
    };
    let result = tokio::spawn(work).await;
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
    if images {
        sqlx::query("SET LOCAL ROLE board_attachment_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("DELETE FROM post_secrets.image_upload_history WHERE scope=$1")
            .bind(&board)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("DELETE FROM post_secrets.image_history_scopes WHERE scope=$1")
            .bind(&board)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("RESET ROLE").execute(&mut *tx).await.unwrap();
    }
    sqlx::query("DELETE FROM admission.file_rules WHERE board=$1")
        .bind(&board)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM admission.hits WHERE rule_id IN(SELECT id FROM admission.rules WHERE board=$1)").bind(&board).execute(&mut *tx).await.unwrap();
    sqlx::query("DELETE FROM admission.rules WHERE board=$1")
        .bind(&board)
        .execute(&mut *tx)
        .await
        .unwrap();
    let jobs: Vec<String> = sqlx::query_scalar("SELECT job_id FROM content.post_media WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)")
        .bind(&board).fetch_all(&mut *tx).await.unwrap();
    for statement in [
        "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.post_media WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
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
    for job in jobs {
        for statement in [
            "DELETE FROM media.assets WHERE job_id=$1",
            "DELETE FROM media_intake.handles WHERE job_id=$1",
            "DELETE FROM media.jobs WHERE id=$1",
        ] {
            sqlx::query(statement)
                .bind(&job)
                .execute(&mut *tx)
                .await
                .unwrap();
        }
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
    sqlx::query("DELETE FROM verification_private.vip_attempts WHERE peer=$1")
        .bind(peer_hash.as_slice())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.pass_entitlements WHERE id=$1")
        .bind(pass_account)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.vip_accounts WHERE id=$1")
        .bind(account)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    public.close().await;
    verifier.close().await;
    owner.close().await;
    result.unwrap();
}

// Synthetic metadata qualifies admission and one-use attachment binding; it is
// not a claim that this fixture decodes or processes a real image.
async fn owned_upload(_board: &str, _target: i64) -> post_media::NewAttachment {
    use board_store::{media::MediaQueue, media_assets::*, media_intake::IntakeStore};
    let intake = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let upload = intake.reserve("owned-vip.png").await.unwrap();
    intake
        .begin_upload(&upload.id, &upload.capability)
        .await
        .unwrap();
    intake
        .finish_upload(&upload.id, &upload.capability, 100)
        .await
        .unwrap();
    let job = queue.claim().await.unwrap().unwrap();
    assert_eq!(job.id, upload.id);
    let token = job.lease_token.as_deref().unwrap();
    let output = StillOutput {
        exif: None,
        format: MediaFormat::Png,
        primary: OutputMetadata {
            sha256: "a".repeat(64),
            bytes: 100,
            width: 200,
            height: 100,
        },
        variants: OutputVariants {
            md5: "07".repeat(16),
            thumbnail: OutputMetadata {
                sha256: "b".repeat(64),
                bytes: 50,
                width: 100,
                height: 50,
            },
        },
        mobile: None,
        source: SourceProvenance {
            input_sha256: "d".repeat(64),
            input_bytes: 100,
            profile: SourceProfile::PngV1,
            retained_bytes: 100,
            md5: [7; 16],
        },
    };
    let asset = queue
        .prepare_output_with_provenance(
            &job.id,
            token,
            &output.primary,
            Some(&output.variants),
            Some(&output.source),
        )
        .await
        .unwrap();
    let input = InputReceipt {
        component: InputComponent::Upload,
        sha256: "d".repeat(64),
        bytes: 100,
        md5: [8; 16],
    };
    queue
        .record_input_receipt(&job.id, token, &asset.id, &input)
        .await
        .unwrap();
    queue
        .approve_output_with_input(&job.id, token, &asset.id, &input)
        .await
        .unwrap();
    post_media::NewAttachment {
        upload,
        spoiler: false,
    }
}
