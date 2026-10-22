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
    exif: Option<&str>,
) -> NewAttachment {
    let upload = intake
        .reserve_image("owned.jpg", board, parent, false)
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
        exif: exif.map(|text| board_store::media_assets::ExifMetadata {
            text: text.into(),
            mode: 2,
        }),
        format: MediaFormat::Jpeg,
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

async fn post(public: &PgPool, image: &NewAttachment, body: &str) -> Result<i64, StoreError> {
    let key = board_domain::poster_id::PosterIdKey::parse(&"a1".repeat(32)).unwrap();
    create_post_with_identity_keys(
        public,
        "p",
        0,
        &NewPost {
            name: "Anonymous".into(),
            subject: String::new(),
            comment: body.into(),
            deletion_hash: "owned".into(),
            sage: false,
        },
        Some(image),
        PostingContext {
            request_start: chrono::Utc::now(),
            peer: Some("192.0.2.197".parse().unwrap()),
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
async fn isolated_exif_provenance_reaches_real_posting_and_rolls_back_refusals() {
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
    let old: (bool, i32) = sqlx::query_as(
        "SELECT captcha_enabled,posting_thread_seconds FROM content.boards WHERE slug='p'",
    )
    .fetch_one(&owner)
    .await
    .unwrap();
    sqlx::query(
        "UPDATE content.boards SET captcha_enabled=false,posting_thread_seconds=0 WHERE slug='p'",
    )
    .execute(&owner)
    .await
    .unwrap();
    let image = upload(&intake, &queue, "p", 0, 91, Some("Camera: Synthetic\n")).await;
    // Source metadata qualifies an otherwise blank OP and carries its allocated
    // media number through the saved typed payload and comparison history.
    let id = post(&public, &image, "").await.unwrap();
    let (tim,comment,payload,known): (i64,String,Vec<u8>,bool) = sqlx::query_as("SELECT m.tim,p.comment,p.wordfilter_payload,h.projection_hash IS NOT NULL FROM content.posts p JOIN content.post_media m ON m.post_id=p.id JOIN post_secrets.comment_history h ON h.post_id=p.id WHERE p.id=$1")
        .bind(id).fetch_one(&owner).await.unwrap();
    assert_eq!(&payload[..4], b"WF05");
    assert!(known);
    assert!(comment.contains(&format!("id=\"exif{tim}\"")));
    assert!(comment.contains("Synthetic"));
    let decoded = board_domain::wordfiltered_comment::PreparedComment::decode(&payload).unwrap();
    assert!(decoded.parts().iter().any(
        |part| matches!(part,board_domain::wordfiltered_comment::Part::Generated(d) if d.is_exif())
    ));
    assert!(board_store::thread_snapshot(&public, "p", id).await.is_ok());
    let tickets: i64 = sqlx::query_scalar("SELECT count(*) FROM post_secrets.exif_preparations")
        .fetch_one(&owner)
        .await
        .unwrap();
    assert_eq!(tickets, 0);
    assert!(
        sqlx::query("SELECT * FROM post_secrets.exif_preparations")
            .fetch_all(&public)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("SELECT exif_text FROM media.assets")
            .fetch_all(&public)
            .await
            .is_err()
    );

    let refused = upload(&intake, &queue, "p", 0, 92, Some("Camera: Rejected\n")).await;
    sqlx::query(
        "UPDATE content.boards SET opening_comment_pattern='/NEVER_MATCH_OWNED/' WHERE slug='p'",
    )
    .execute(&owner)
    .await
    .unwrap();
    assert!(matches!(
        post(&public, &refused, "Owned rollback").await,
        Err(StoreError::ContentRejected(_))
    ));
    let tickets: i64 = sqlx::query_scalar("SELECT count(*) FROM post_secrets.exif_preparations")
        .fetch_one(&owner)
        .await
        .unwrap();
    assert_eq!(tickets, 0);
    post_media::check_upload(&public, &refused.upload.id, &refused.upload.capability)
        .await
        .unwrap();
    sqlx::query("UPDATE content.boards SET opening_comment_pattern='' WHERE slug='p'")
        .execute(&owner)
        .await
        .unwrap();
    let id2 = post(&public, &refused, "Owned retry").await.unwrap();

    let unused = upload(&intake, &queue, "p", 0, 93, Some("Camera: Unused\n")).await;
    let mut tx = public.begin().await.unwrap();
    let prepared: (Option<String>, Option<i64>) =
        sqlx::query_as("SELECT * FROM content.prepare_attachment_exif($1,$2,'p',0,false,false)")
            .bind(&unused.upload.id)
            .bind(&unused.upload.capability)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert!(prepared.1.is_some());
    assert!(
        tx.commit().await.is_err(),
        "unconsumed tickets must not persist"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM post_secrets.exif_preparations")
            .fetch_one(&owner)
            .await
            .unwrap(),
        0
    );
    post_media::check_upload(&public, &unused.upload.id, &unused.upload.capability)
        .await
        .unwrap();
    let large = upload(
        &intake,
        &queue,
        "p",
        0,
        94,
        Some(&format!("Key: {}", "&".repeat(16_000))),
    )
    .await;
    assert!(matches!(
        post(&public, &large, "Owned bound").await,
        Err(StoreError::Invalid(
            "EXIF comment exceeds the saved-content byte limit."
        ))
    ));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM post_secrets.exif_preparations")
            .fetch_one(&owner)
            .await
            .unwrap(),
        0
    );
    let actor: Vec<u8> =
        sqlx::query_scalar("SELECT actor_hash FROM post_secrets.posting_history WHERE post_id=$1")
            .bind(id)
            .fetch_one(&owner)
            .await
            .unwrap();
    // Restore the original board policy and remove only owned fixture data.
    sqlx::query(
        "UPDATE content.boards SET captcha_enabled=$1,posting_thread_seconds=$2 WHERE slug='p'",
    )
    .bind(old.0)
    .bind(old.1)
    .execute(&owner)
    .await
    .unwrap();
    for statement in [
        "DELETE FROM content.post_media WHERE post_id=ANY($1)",
        "DELETE FROM post_secrets.deletion WHERE post_id=ANY($1)",
        "DELETE FROM content.posts WHERE id=ANY($1)",
        "DELETE FROM content.threads WHERE id=ANY($1)",
    ] {
        sqlx::query(statement)
            .bind(vec![id, id2])
            .execute(&owner)
            .await
            .unwrap();
    }
    sqlx::query(
        "DELETE FROM post_secrets.posting_thread_actions WHERE board='p' AND actor_hash=$1",
    )
    .bind(&actor)
    .execute(&owner)
    .await
    .unwrap();
    for image in [&image, &refused, &unused, &large] {
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
    assert_ne!(id, id2);
}
