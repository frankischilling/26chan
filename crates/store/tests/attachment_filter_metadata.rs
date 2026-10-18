#![cfg(feature = "database-tests")]
use board_store::{
    media::MediaQueue,
    media_assets::{
        InputComponent, InputReceipt, MediaFormat, OutputMetadata, OutputVariants, SourceProfile,
        SourceProvenance, StillOutput,
    },
    media_intake::IntakeStore,
};
use sqlx::PgPool;

#[tokio::test]
async fn preflight_binds_capability_context_and_original_digest_eligibility() {
    let admin = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = PgPool::connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let staff = PgPool::connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let intake = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    // Metadata fixtures exercise the queue and policy, not image parsing.
    for board in ["p", "g"] {
        for badged in [false, true] {
            for format in [MediaFormat::Jpeg, MediaFormat::Png] {
                let upload = intake
                    .reserve_image("synthetic.image", board, 0, badged)
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
                let output = StillOutput {
                    format,
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
                        profile: SourceProfile::StillV6,
                        retained_bytes: 100,
                        md5: [7; 16],
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
                let pool = if badged { &staff } else { &public };
                let metadata: (String, Option<String>, Option<Vec<u8>>, Option<bool>) = sqlx::query_as(
                    "SELECT * FROM content.attachment_upload_filter_metadata($1,$2,$3,0,false,$4)"
                ).bind(&job.id).bind(&upload.capability).bind(board).bind(badged).fetch_one(pool).await.unwrap();
                let eligible = format != MediaFormat::Jpeg || board != "p";
                assert_eq!(
                    metadata,
                    (
                        "synthetic.image".into(),
                        Some("07".repeat(16)),
                        eligible.then(|| vec![8; 16]),
                        Some(eligible)
                    )
                );
                for capability in ["0".repeat(64), "invalid".into()] {
                    assert!(sqlx::query("SELECT * FROM content.attachment_upload_filter_metadata($1,$2,$3,0,false,$4)")
                        .bind(&job.id).bind(capability).bind(board).bind(badged).execute(pool).await.is_err());
                }
                if board == "p" && format == MediaFormat::Jpeg {
                    assert!(sqlx::query("SELECT * FROM content.attachment_upload_filter_metadata($1,$2,$3,0,false,$4)")
                        .bind(&job.id).bind(&upload.capability).bind(board).bind(!badged).execute(&staff).await.is_err());
                }
                assert!(sqlx::query("SELECT * FROM content.attachment_upload_filter_metadata($1,$2,$3,0,false,true)")
                    .bind(&job.id).bind(&upload.capability).bind(board).execute(&public).await.is_err());
                assert!(sqlx::query("SELECT * FROM content.attachment_upload_filter_metadata($1,$2,$3,0,false,$4)")
                    .bind(&job.id).bind(&upload.capability).bind(if board=="p" {"g"} else {"p"}).bind(badged).execute(pool).await.is_err());
                let attached: i64 =
                    sqlx::query_scalar("SELECT count(*) FROM content.post_media WHERE job_id=$1")
                        .bind(&job.id)
                        .fetch_one(&admin)
                        .await
                        .unwrap();
                assert_eq!(attached, 0, "Preflight must not consume the attachment");
                sqlx::query("UPDATE media.jobs SET created_at=clock_timestamp()-interval '2 hours' WHERE id=$1")
                    .bind(&job.id).execute(&admin).await.unwrap();
                assert!(sqlx::query("SELECT * FROM content.attachment_upload_filter_metadata($1,$2,$3,0,false,$4)")
                    .bind(&job.id).bind(&upload.capability).bind(board).bind(badged).execute(pool).await.is_err());
                for statement in [
                    "DELETE FROM media.assets WHERE job_id=$1",
                    "DELETE FROM media_intake.handles WHERE job_id=$1",
                    "DELETE FROM media.jobs WHERE id=$1",
                ] {
                    sqlx::query(statement)
                        .bind(&job.id)
                        .execute(&admin)
                        .await
                        .unwrap();
                }
            }
        }
    }
    for pool in [&public, &staff] {
        assert!(
            sqlx::query("SELECT md5 FROM media.input_receipts")
                .execute(pool)
                .await
                .is_err()
        );
        assert!(
            sqlx::query("SELECT md5 FROM media.assets")
                .execute(pool)
                .await
                .is_err()
        );
    }
}
