#![cfg(feature = "database-tests")]
use board_store::{
    media::MediaQueue,
    media_assets::{InputComponent, InputReceipt, OutputMetadata},
    media_intake::IntakeStore,
};

#[tokio::test]
async fn source_upload_board_consumes_swf_and_serializes_filename_claims() {
    let admin = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = sqlx::PgPool::connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let intake = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let identity = board_domain::poster_id::PosterIdKey::parse(&"8".repeat(64)).unwrap();
    let before: String = sqlx::query_scalar("SELECT to_jsonb(b)::text FROM content.boards b WHERE slug='f' AND captcha_enabled AND NOT captcha_twister").fetch_one(&admin).await.unwrap();
    assert_eq!(sqlx::query("UPDATE content.boards b SET captcha_enabled=false WHERE slug='f' AND to_jsonb(b)=$1::jsonb").bind(&before).execute(&admin).await.unwrap().rows_affected(),1);
    let restore = admin.clone();
    let result = tokio::spawn(async move {
        let board = board_store::board(&public, "f").await.unwrap();
        assert!(board.check_attachment_allowed(0, false).is_err());
        assert!(board.check_attachment_allowed(1, true).is_err());
        assert!(board.check_attachment_allowed(1, false).is_ok());
        let mut assets = Vec::new();
        let mut jobs = Vec::new();
        let mut posts = Vec::new();
        for number in [1, 2, 3] {
            let upload = intake
                .reserve_image("owned-shared.swf", "f", 0, false)
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
            assert_eq!(job.id, upload.id, "Use an idle disposable queue");
            let token = job.lease_token.as_deref().unwrap();
            let sha = number.to_string().repeat(64);
            let input = InputReceipt {
                component: InputComponent::Upload,
                sha256: sha.clone(),
                bytes: 100,
                md5: [number as u8; 16],
            };
            let metadata = OutputMetadata {
                sha256: sha,
                bytes: 100,
                width: 640,
                height: 480,
            };
            let asset = queue
                .prepare_swf_output(&job.id, token, &metadata, &input)
                .await
                .unwrap();
            queue
                .record_input_receipt(&job.id, token, &asset.id, &input)
                .await
                .unwrap();
            queue
                .approve_output_with_input(&job.id, token, &asset.id, &input)
                .await
                .unwrap();
            assets.push(asset.id);
            jobs.push(job.id);
            let result = board_store::create_post_with_metadata(
                &public,
                "f",
                0,
                &board_store::NewPost {
                    name: String::new(),
                    subject: format!("4|Owned movie {number}"),
                    comment: format!("Owned movie comment {number}"),
                    deletion_hash: "owned-hash".into(),
                    sage: false,
                },
                Some(&board_store::post_media::NewAttachment {
                    upload,
                    spoiler: false,
                }),
                board_store::PostingContext {
                    request_start: sqlx::query_scalar("SELECT clock_timestamp()")
                        .fetch_one(&admin)
                        .await
                        .unwrap(),
                    peer: Some(format!("192.0.2.{}", 220 + number).parse().unwrap()),
                    op_password_proof: None,
                },
                board_store::PostMetadata {
                    keys: board_store::PostIdentityKeys {
                        tripcode: None,
                        poster_id: Some(&identity),
                    },
                    spoiler: false,
                    country_database: None,
                    flag: "",
                    options: "",
                    drawing: None,
                },
            )
            .await;
            if number != 2 {
                let id = result.unwrap();
                posts.push(id);
                assert_eq!(
                    claimed_name(&admin, id).await.as_deref(),
                    Some("owned-shared.swf")
                );
                assert!(matches!(
                    board_store::post_media::delete_attachment(&public, "f", id).await,
                    Err(board_store::StoreError::Invalid(
                        board_domain::upload_board::FILE_ONLY_ERROR
                    ))
                ));
                assert!(claimed_name(&admin, id).await.is_some());
                assert!(
                    sqlx::query("SELECT name FROM content.upload_board_names")
                        .execute(&public)
                        .await
                        .is_err()
                );
            } else {
                assert!(
                    matches!(
                        result,
                        Err(board_store::StoreError::Invalid("Filename already exists."))
                    ),
                    "{result:?}"
                );
                let retired = posts[0];
                sqlx::query("UPDATE content.threads SET deleted=true WHERE id=$1")
                    .bind(retired)
                    .execute(&admin)
                    .await
                    .unwrap();
                assert!(
                    claimed_name(&admin, retired).await.is_none(),
                    "Capacity retirement must release the filename"
                );
            }
        }
        for id in posts {
            sqlx::query("DELETE FROM content.post_media WHERE post_id=$1")
                .bind(id)
                .execute(&admin)
                .await
                .unwrap();
            assert!(claimed_name(&admin, id).await.is_none());
            sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id=$1")
                .bind(id)
                .execute(&admin)
                .await
                .unwrap();
            sqlx::query("DELETE FROM content.posts WHERE thread_id=$1")
                .bind(id)
                .execute(&admin)
                .await
                .unwrap();
            sqlx::query("DELETE FROM content.threads WHERE id=$1")
                .bind(id)
                .execute(&admin)
                .await
                .unwrap();
        }
        for id in assets {
            sqlx::query("DELETE FROM media.assets WHERE id=$1")
                .bind(id)
                .execute(&admin)
                .await
                .unwrap();
        }
        for id in jobs {
            sqlx::query("DELETE FROM media.jobs WHERE id=$1")
                .bind(id)
                .execute(&admin)
                .await
                .unwrap();
        }
    })
    .await;
    assert_eq!(sqlx::query("UPDATE content.boards b SET captcha_enabled=true WHERE slug='f' AND to_jsonb(b)=($1::jsonb || '{\"captcha_enabled\":false}'::jsonb)").bind(&before).execute(&restore).await.unwrap().rows_affected(),1,"Do not overwrite unrelated policy changes");
    result.unwrap();
}

async fn claimed_name(admin: &sqlx::PgPool, id: i64) -> Option<String> {
    let mut tx = admin.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_attachment_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let value = sqlx::query_scalar("SELECT name FROM content.upload_board_names WHERE post_id=$1")
        .bind(id)
        .fetch_optional(&mut *tx)
        .await
        .unwrap();
    tx.rollback().await.unwrap();
    value
}
