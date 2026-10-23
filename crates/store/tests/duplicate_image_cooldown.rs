#![cfg(feature = "database-tests")]
use board_store::{
    media::MediaQueue,
    media_assets::{
        InputComponent, InputReceipt, MediaFormat, OutputMetadata, OutputVariants, SourceProfile,
        SourceProvenance, StillOutput,
    },
    media_intake::IntakeStore,
    post_media::NewAttachment,
    *,
};
use sqlx::PgPool;

async fn upload(
    intake: &IntakeStore,
    queue: &MediaQueue,
    board: &str,
    parent: i64,
    digest: u8,
) -> NewAttachment {
    let upload = intake
        .reserve_image("owned.png", board, parent, false)
        .await
        .unwrap();
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
    let md5 = format!("{digest:02x}").repeat(16);
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
            md5,
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
            profile: SourceProfile::StillV6,
            retained_bytes: 100,
            md5: [digest; 16],
        },
    };
    let asset = queue
        .prepare_still_output(&job.id, token, &output)
        .await
        .unwrap();
    let input = InputReceipt {
        component: InputComponent::Upload,
        sha256: "d".repeat(64),
        bytes: 100,
        md5: [9; 16],
    };
    queue
        .record_input_receipt(&job.id, token, &asset.id, &input)
        .await
        .unwrap();
    queue
        .approve_output_with_input(&job.id, token, &asset.id, &input)
        .await
        .unwrap();
    NewAttachment {
        upload,
        spoiler: false,
    }
}
async fn post(
    public: &PgPool,
    board: &str,
    parent: i64,
    at: i64,
    image: &NewAttachment,
) -> Result<i64, StoreError> {
    let key = board_domain::poster_id::PosterIdKey::parse(&"a1".repeat(32)).unwrap();
    create_post_with_identity_keys(
        public,
        board,
        parent,
        &NewPost {
            name: "Anonymous".into(),
            subject: "Owned duplicate qualification".into(),
            comment: format!("Owned image at {at}"),
            deletion_hash: "owned".into(),
            sage: false,
        },
        Some(image),
        PostingContext {
            request_start: chrono::DateTime::from_timestamp(at, 0).unwrap(),
            peer: Some("192.0.2.133".parse().unwrap()),
            op_password_proof: None,
        },
        PostIdentityKeys {
            tripcode: None,
            poster_id: Some(&key),
        },
    )
    .await
}
#[tokio::test]
async fn duplicate_images_use_bound_normalized_digest_before_ordinary_reply_timers() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let intake = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board = String::from("g");
    let original: (bool,i32,i32,i32) = sqlx::query_as("SELECT captcha_enabled,posting_reply_seconds,posting_image_seconds,posting_thread_seconds FROM content.boards WHERE slug=$1").bind(&board).fetch_one(&owner).await.unwrap();
    sqlx::query("UPDATE content.boards SET captcha_enabled=false,posting_reply_seconds=0,posting_image_seconds=0,posting_thread_seconds=0 WHERE slug=$1").bind(&board).execute(&owner).await.unwrap();
    let now = chrono::Utc::now().timestamp();
    let first = upload(&intake, &queue, &board, 0, 1).await;
    let op1 = post(&public, &board, 0, now - 1000, &first).await.unwrap();
    let second = upload(&intake, &queue, &board, 0, 2).await;
    let op2 = post(&public, &board, 0, now - 900, &second).await.unwrap();
    let image = upload(&intake, &queue, &board, op1, 7).await;
    let reply = post(&public, &board, op1, now - 100, &image).await.unwrap();
    let candidate = upload(&intake, &queue, &board, op2, 7).await;
    let actor: Vec<u8> =
        sqlx::query_scalar("SELECT actor_hash FROM post_secrets.posting_history WHERE post_id=$1")
            .bind(op1)
            .fetch_one(&owner)
            .await
            .unwrap();
    let cases: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/duplicate-image-cooldown-cases.json"
    ))
    .unwrap();
    for case in cases.as_array().unwrap() {
        let mut tx = owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_anonymous_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        let result: Option<i64> = sqlx::query_scalar(
            "SELECT remaining_seconds FROM content.pass_duplicate_cooldown_core($1,$2,$3,$4,$5,$6)",
        )
        .bind(&actor)
        .bind(&board)
        .bind(op2)
        .bind(now - 100 + case["age"].as_i64().unwrap())
        .bind(vec![7u8; 16])
        .bind(case["bypass"].as_bool().unwrap())
        .fetch_optional(&mut *tx)
        .await
        .unwrap();
        assert_eq!(
            result,
            case["remaining"].as_i64(),
            "Source timing case {case}"
        );
        tx.rollback().await.unwrap();
    }
    for (target, digest, scope, identity, at, expected) in [
        (op1, 7u8, board.as_str(), actor.clone(), now, None), // same-thread replies do not match
        (op2, 9, "g", actor.clone(), now, None), // original input MD5 is not the normalized digest
        (op2, 7, "a", actor.clone(), now, None),
        (op2, 7, "g", vec![0; 32], now, None),
        (op1, 1, "g", actor.clone(), now - 851, Some(1)), // source OP resto=0 still matches its own thread
    ] {
        let mut tx = owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_anonymous_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        let result:Option<i64>=sqlx::query_scalar("SELECT remaining_seconds FROM content.pass_duplicate_cooldown_core($1,$2,$3,$4,$5,true)")
            .bind(identity).bind(scope).bind(target).bind(at).bind(vec![digest;16]).fetch_optional(&mut *tx).await.unwrap();
        assert_eq!(result, expected);
        tx.rollback().await.unwrap();
    }
    assert!(
        sqlx::query("SELECT * FROM content.pass_duplicate_cooldown_core($1,$2,$3,$4,$5,true)")
            .bind(&actor)
            .bind(&board)
            .bind(op2)
            .bind(now)
            .bind(vec![7u8; 16])
            .execute(&public)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("SELECT * FROM content.pass_check_duplicate_image($1,$2,$3,$4,$5,$6)")
            .bind(&actor)
            .bind(&board)
            .bind(op2)
            .bind(now)
            .bind(&candidate.upload.id)
            .bind("0".repeat(64))
            .execute(&public)
            .await
            .is_err()
    );
    let error = post(&public, &board, op2, now, &candidate)
        .await
        .unwrap_err();
    assert!(
        matches!(error,StoreError::PostingCooldownRejected(ref rejection) if rejection.reason==PostingCooldownReason::DuplicateImage && rejection.remaining_seconds==50 && !rejection.pass_offer),
        "{error:?}"
    );
    sqlx::query("UPDATE content.post_media SET file_deleted=true WHERE post_id=$1")
        .bind(reply)
        .execute(&owner)
        .await
        .unwrap();
    let retained: Option<i64> = sqlx::query_scalar(
        "SELECT remaining_seconds FROM content.pass_check_duplicate_image($1,$2,$3,$4,$5,$6)",
    )
    .bind(&actor)
    .bind(&board)
    .bind(op2)
    .bind(now)
    .bind(&candidate.upload.id)
    .bind(&candidate.upload.capability)
    .fetch_optional(&public)
    .await
    .unwrap();
    assert_eq!(
        retained,
        Some(50),
        "Source file-only deletion retains duplicate timing"
    );
    sqlx::query("UPDATE content.boards SET captcha_enabled=true WHERE slug=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    let ordinary: Option<i64> = sqlx::query_scalar(
        "SELECT remaining_seconds FROM content.pass_check_duplicate_image($1,$2,$3,$4,$5,$6)",
    )
    .bind(&actor)
    .bind(&board)
    .bind(op2)
    .bind(now)
    .bind(&candidate.upload.id)
    .bind(&candidate.upload.capability)
    .fetch_optional(&public)
    .await
    .unwrap();
    assert_eq!(ordinary, Some(200));
    let ordinary_error = post(&public, &board, op2, now, &candidate)
        .await
        .unwrap_err();
    assert!(
        matches!(ordinary_error,StoreError::PostingCooldownRejected(ref rejection) if rejection.reason==PostingCooldownReason::DuplicateImage && rejection.remaining_seconds==200 && rejection.pass_offer),
        "{ordinary_error:?}"
    );
    let mut setup = owner.begin().await.unwrap();
    sqlx::raw_sql("INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,created_at,network_at,address_at,environment_at,expires_at) SELECT sha256('duplicate-anon'),sha256('network'),sha256('address'),sha256('environment'),n,n,n,n,n+3600 FROM (SELECT floor(extract(epoch FROM clock_timestamp()))::bigint n) clock; SET LOCAL ROLE board_anonymous_owner; WITH account AS (INSERT INTO verification_private.pass_entitlements(token_hash,pin_hash,status,purchased_at,expires_at) VALUES(sha256('duplicate-account'),repeat('owned-placeholder',3),0,'2020-01-01',statement_timestamp()+interval '1 year') RETURNING id) INSERT INTO verification_private.pass_sessions SELECT sha256('duplicate-member'),id,1,statement_timestamp(),statement_timestamp()+interval '1 day' FROM account;").execute(&mut *setup).await.unwrap();
    setup.commit().await.unwrap();
    let verifier = PgPool::connect(&std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let issued:String=sqlx::query_scalar("SELECT status FROM content.pass_write_receipt(sha256('duplicate-member'),sha256('duplicate-receipt'),sha256('duplicate-peer'),sha256('login'),sha256('posting'),sha256('duplicate-anon'),sha256('form'),'reply',$1,$2)").bind(&board).bind(op2).fetch_one(&verifier).await.unwrap();
    assert_eq!(issued, "ready");
    let proof = "SELECT set_config('board.challenge_context',jsonb_build_object('receipt',encode(sha256('duplicate-receipt'),'hex'),'peer',encode(sha256('duplicate-peer'),'hex'),'session',encode(sha256('duplicate-anon'),'hex'),'context',encode(sha256('form'),'hex'))::text,true)";
    let mut member = public.begin().await.unwrap();
    sqlx::query(proof).execute(&mut *member).await.unwrap();
    let discounted: Option<i64> = sqlx::query_scalar(
        "SELECT remaining_seconds FROM content.pass_check_duplicate_image($1,$2,$3,$4,$5,$6)",
    )
    .bind(&actor)
    .bind(&board)
    .bind(op2)
    .bind(now)
    .bind(&candidate.upload.id)
    .bind(&candidate.upload.capability)
    .fetch_optional(&mut *member)
    .await
    .unwrap();
    assert_eq!(discounted, Some(50));
    member.rollback().await.unwrap();
    let mut private = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *private)
        .await
        .unwrap();
    let unconsumed:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM verification_private.pass_receipts WHERE token_hash=sha256('duplicate-receipt'))").fetch_one(&mut *private).await.unwrap();
    assert!(unconsumed);
    sqlx::query("UPDATE verification_private.pass_entitlements SET status=4 WHERE token_hash=sha256('duplicate-account')").execute(&mut *private).await.unwrap();
    private.commit().await.unwrap();
    let mut member = public.begin().await.unwrap();
    sqlx::query(proof).execute(&mut *member).await.unwrap();
    let revoked =
        sqlx::query("SELECT * FROM content.pass_check_duplicate_image($1,$2,$3,$4,$5,$6)")
            .bind(&actor)
            .bind(&board)
            .bind(op2)
            .bind(now)
            .bind(&candidate.upload.id)
            .bind(&candidate.upload.capability)
            .execute(&mut *member)
            .await
            .unwrap_err();
    assert_eq!(
        revoked
            .as_database_error()
            .and_then(|e| e.code())
            .as_deref(),
        Some("P0166")
    );
    member.rollback().await.unwrap();
    let mut cleanup = owner.begin().await.unwrap();
    sqlx::raw_sql("SET LOCAL ROLE board_anonymous_owner; DELETE FROM verification_private.pass_entitlements WHERE token_hash=sha256('duplicate-account'); RESET ROLE; DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=sha256('duplicate-anon');").execute(&mut *cleanup).await.unwrap();
    cleanup.commit().await.unwrap();
    sqlx::query("UPDATE content.boards SET captcha_enabled=false WHERE slug=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    // The refused upload remains usable; the exact boundary succeeds.
    let accepted = post(&public, &board, op2, now + 50, &candidate)
        .await
        .unwrap();
    assert!(accepted > reply);
    let mut reordered = owner.begin().await.unwrap();
    sqlx::query("UPDATE post_secrets.posting_history SET request_at=$2 WHERE post_id=$1")
        .bind(accepted)
        .bind(now - 500)
        .execute(&mut *reordered)
        .await
        .unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *reordered)
        .await
        .unwrap();
    let newest: Option<i64> = sqlx::query_scalar(
        "SELECT remaining_seconds FROM content.pass_duplicate_cooldown_core($1,$2,$3,$4,$5,true)",
    )
    .bind(&actor)
    .bind(&board)
    .bind(accepted + 1000)
    .bind(now)
    .bind(vec![7u8; 16])
    .fetch_optional(&mut *reordered)
    .await
    .unwrap();
    assert_eq!(
        newest, None,
        "Source chooses newest post number, not maximum timestamp"
    );
    reordered.rollback().await.unwrap();

    // Cleanup only records owned by this fixture, including already consumed uploads.
    for statement in [
        "DELETE FROM content.post_media WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1 AND thread_id=ANY($2))",
        "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1 AND thread_id=ANY($2))",
    ] {
        sqlx::query(statement)
            .bind(&board)
            .bind(vec![op1, op2])
            .execute(&owner)
            .await
            .unwrap();
    }

    sqlx::query("DELETE FROM content.posts WHERE board=$1 AND thread_id=ANY($2)")
        .bind(&board)
        .bind(vec![op1, op2])
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.threads WHERE board=$1 AND id=ANY($2)")
        .bind(&board)
        .bind(vec![op1, op2])
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM post_secrets.posting_thread_actions WHERE board=$1 AND actor_hash=$2")
        .bind(&board)
        .bind(&actor)
        .execute(&owner)
        .await
        .unwrap();
    for image in [&first, &second, &image, &candidate] {
        for statement in [
            "DELETE FROM media.assets WHERE job_id=$1",
            "DELETE FROM media_intake.handles WHERE job_id=$1",
            "DELETE FROM media.jobs WHERE id=$1",
        ] {
            sqlx::query(statement)
                .bind(&image.upload.id)
                .execute(&owner)
                .await
                .unwrap();
        }
    }
    sqlx::query("UPDATE content.boards SET captcha_enabled=$2,posting_reply_seconds=$3,posting_image_seconds=$4,posting_thread_seconds=$5 WHERE slug=$1").bind(&board).bind(original.0).bind(original.1).bind(original.2).bind(original.3).execute(&owner).await.unwrap();
}
