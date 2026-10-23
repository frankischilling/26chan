#![cfg(feature = "database-tests")]
mod support;
use board_store::{media::MediaQueue, media_assets::*, media_intake::IntakeStore, *};
use sqlx::PgPool;
use std::sync::Arc;

#[tokio::test]
async fn active_image_scopes_repost_limits_and_concurrent_attempts() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board = "qb".to_owned();
    let alias_board = "qa".to_owned();
    let before=support::borrow_empty_source_board(&owner,&board,serde_json::json!({"title":"Owned duplicates","text_only":true,"captcha_enabled":false,"posting_reply_seconds":0,"posting_image_seconds":0,"posting_thread_seconds":0,"user_thread_limit":100,"image_limit":100,"archive_retention_seconds":3600})).await;
    let alias_before=support::borrow_empty_source_board(&owner,&alias_board,serde_json::json!({"title":"Owned shared history","text_only":false,"captcha_enabled":false,"posting_reply_seconds":0,"posting_image_seconds":0,"posting_thread_seconds":0,"user_thread_limit":100,"image_limit":100,"upload_board":true})).await;
    let mut uploads = Vec::new();
    for digest in [7, 7, 7, 7, 7, 8, 8] {
        uploads.push(Arc::new(owned_upload(Some(digest)).await));
    }
    uploads.push(Arc::new(owned_upload(None).await));
    // Reserve the later history fixtures with real upload-board provenance.
    sqlx::query("UPDATE content.boards SET upload_board=true,text_only=false WHERE slug=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();

    for (offset, digest) in [9, 9, 10, 11, 12, 12].into_iter().enumerate() {
        let file = if digest == 11 {
            owned_upload(Some(digest)).await
        } else {
            support::owned_swf(
                if offset == 5 { &alias_board } else { &board },
                digest,
                &format!("owned-history-{offset}.swf"),
            )
            .await
        };
        uploads.push(Arc::new(file));
    }
    sqlx::query("UPDATE content.boards SET upload_board=false,text_only=true WHERE slug=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    let jobs: Vec<String> = uploads.iter().map(|file| file.upload.id.clone()).collect();
    let work = {
        let public = public.clone();
        let owner = owner.clone();
        let board = board.clone();
        let alias_board = alias_board.clone();
        async move {
            let submit = |parent, index: Option<usize>, peer: u8| {
                let public = public.clone();
                let board = board.clone();
                let attachment = index.map(|index| uploads[index].clone());
                async move {
                    support::create_post_with_context(
                        &public,
                        &board,
                        parent,
                        &NewPost {
                            name: "Anonymous".into(),
                            subject: "Owned duplicate image".into(),
                            comment: "Owned independent post".into(),
                            deletion_hash: "fixture".into(),
                            sage: false,
                        },
                        attachment.as_deref(),
                        PostingContext {
                            request_start: chrono::Utc::now(),
                            peer: Some(std::net::Ipv4Addr::new(192, 0, 2, peer).into()),
                            op_password_proof: None,
                        },
                    )
                    .await
                }
            };
            let mut threads = Vec::new();
            for peer in 1..=3 {
                threads.push(submit(0, None, peer).await.unwrap());
            }
            sqlx::query("UPDATE content.boards SET text_only=false WHERE slug=$1")
                .bind(&board)
                .execute(&owner)
                .await
                .unwrap();
            let unknown = submit(threads[0], Some(7), 9).await.unwrap_err();
            assert!(
                matches!(unknown,StoreError::Database(ref error) if error.as_database_error().and_then(|e|e.code()).as_deref()==Some("P0241")),
                "{unknown:?}"
            );
            post_media::check_upload(
                &public,
                &uploads[7].upload.id,
                &uploads[7].upload.capability,
            )
            .await
            .unwrap();
            let first = submit(threads[0], Some(0), 10).await.unwrap();
            let duplicate = submit(threads[0], Some(1), 11).await.unwrap_err();
            assert!(
                matches!(duplicate,StoreError::ImageDuplicate(ref r) if r.existing()==Some((first,threads[0]))),
                "{duplicate:?}"
            );
            let second = submit(threads[1], Some(1), 11).await.unwrap();
            sqlx::query("UPDATE content.boards SET image_repost_limit=2 WHERE slug=$1")
                .bind(&board)
                .execute(&owner)
                .await
                .unwrap();
            let limited = submit(threads[2], Some(2), 12).await.unwrap_err();
            assert!(
                matches!(limited,StoreError::ImageDuplicate(ref r) if r.existing().is_none()),
                "{limited:?}"
            );
            sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE board=$1 AND id=$2")
            .bind(&board).bind(threads[0]).execute(&owner).await.unwrap();
            let third = submit(threads[2], Some(2), 12).await.unwrap();
            assert!(
                third > second,
                "A refused upload remains usable after the live count drops"
            );
            sqlx::query("UPDATE content.boards SET image_repost_limit=0 WHERE slug=$1")
                .bind(&board)
                .execute(&owner)
                .await
                .unwrap();
            let op = submit(0, Some(3), 13).await.unwrap();
            let duplicate_op = submit(0, Some(4), 14).await.unwrap_err();
            assert!(
                matches!(duplicate_op,StoreError::ImageDuplicate(ref r) if r.existing()==Some((op,op))),
                "{duplicate_op:?}"
            );
            // The fixture owns the target; exercise the existing file-only
            // deletion operation rather than changing private digest data.
            sqlx::query("SELECT content.delete_post_attachment($1,$2)")
                .bind(&board)
                .bind(op)
                .execute(&public)
                .await
                .unwrap();
            let replacement = submit(0, Some(4), 14).await.unwrap();
            assert!(
                replacement > op,
                "file-only deletion releases the duplicate scope"
            );
            let (left, right) = tokio::join!(submit(0, Some(5), 15), submit(0, Some(6), 16));
            assert_eq!(
                usize::from(left.is_ok()) + usize::from(right.is_ok()),
                1,
                "one board-serialized insert wins"
            );
            let refusal = left.err().or_else(|| right.err()).unwrap();
            assert!(
                matches!(refusal, StoreError::ImageDuplicate(_)),
                "{refusal:?}"
            );
            // Upload-board history survives live-post retirement. This fixture
            // owns a private namespace rather than borrowing production /f/ rows.
            history_sql(
                &owner,
                "INSERT INTO post_secrets.image_history_scopes(scope,row_limit) VALUES($1,2)",
                &board,
            )
            .await;
            sqlx::query(
                "UPDATE content.boards SET image_history_scope=$1,upload_board=true WHERE slug=$1",
            )
            .bind(&board)
            .execute(&owner)
            .await
            .unwrap();
            let history_first = submit(0, Some(8), 30).await.unwrap();
            sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
                .bind(history_first).execute(&owner).await.unwrap();
            let recent = submit(0, Some(9), 31).await.unwrap_err();
            assert!(
                matches!(
                    recent,
                    StoreError::ImageDuplicate(ImageDuplicateRefusal::RecentUpload)
                ),
                "{recent:?}"
            );
            post_media::check_upload(
                &public,
                &uploads[9].upload.id,
                &uploads[9].upload.capability,
            )
            .await
            .unwrap();
            history_sql(&owner, "UPDATE post_secrets.image_upload_history SET accepted_at=date_trunc('second',clock_timestamp())-interval '24 hours' WHERE scope=$1", &board).await;
            let history_second = submit(0, Some(9), 31).await.unwrap();
            assert!(history_second > history_first);
            // Both recorded rows still exist, so capacity refuses a distinct
            // image and rolls back its post and one-use attachment consumption.
            let full = submit(0, Some(10), 32).await.unwrap_err();
            assert!(
                matches!(full,StoreError::Database(ref e) if e.as_database_error().and_then(|e|e.code()).as_deref()==Some("P0242")),
                "{full:?}"
            );
            post_media::check_upload(
                &public,
                &uploads[10].upload.id,
                &uploads[10].upload.capability,
            )
            .await
            .unwrap();
            history_sql(&owner, "UPDATE post_secrets.image_upload_history SET accepted_at=date_trunc('second',clock_timestamp())-interval '49 hours' WHERE scope=$1 AND post_id=(SELECT min(post_id) FROM post_secrets.image_upload_history WHERE scope=$1)", &board).await;
            submit(0, Some(10), 32).await.unwrap();
            sqlx::query("UPDATE content.boards SET upload_board=false WHERE slug=$1")
                .bind(&board)
                .execute(&owner)
                .await
                .unwrap();
            // Read-enabled, non-upload boards do not add a history row, even
            // when the shared namespace's accepted-upload capacity is full.
            submit(0, Some(11), 33).await.unwrap();
            let mut read = owner.begin().await.unwrap();
            sqlx::query("SET LOCAL ROLE board_attachment_owner")
                .execute(&mut *read)
                .await
                .unwrap();
            let retained: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM post_secrets.image_upload_history WHERE scope=$1",
            )
            .bind(&board)
            .fetch_one(&mut *read)
            .await
            .unwrap();
            assert_eq!(retained, 2);
            read.rollback().await.unwrap();
            history_sql(
                &owner,
                "UPDATE post_secrets.image_history_scopes SET row_limit=10 WHERE scope=$1",
                &board,
            )
            .await;
            sqlx::query("UPDATE content.boards SET upload_board=true WHERE slug=$1")
                .bind(&board)
                .execute(&owner)
                .await
                .unwrap();
            sqlx::query("UPDATE content.boards SET image_history_scope=$2 WHERE slug=$1")
                .bind(&alias_board)
                .bind(&board)
                .execute(&owner)
                .await
                .unwrap();
            let independent = NewPost {
                name: "Anonymous".into(),
                subject: "Owned shared history".into(),
                comment: "Owned independent namespace contender".into(),
                deletion_hash: "fixture".into(),
                sage: false,
            };
            let other = support::create_post_with_context(
                &public,
                &alias_board,
                0,
                &independent,
                Some(uploads[13].as_ref()),
                PostingContext {
                    request_start: chrono::Utc::now(),
                    peer: Some(std::net::Ipv4Addr::new(192, 0, 2, 35).into()),
                    op_password_proof: None,
                },
            );
            let (left, right) = tokio::join!(submit(0, Some(12), 34), other);
            assert_eq!(
                usize::from(left.is_ok()) + usize::from(right.is_ok()),
                1,
                "Shared history serializes independent boards"
            );
            let refusal = left.err().or_else(|| right.err()).unwrap();
            assert!(
                matches!(
                    refusal,
                    StoreError::ImageDuplicate(ImageDuplicateRefusal::RecentUpload)
                ),
                "{refusal:?}"
            );
        }
    };
    let result = tokio::spawn(work).await;
    history_sql(
        &owner,
        "DELETE FROM post_secrets.image_upload_history WHERE scope=$1",
        &board,
    )
    .await;
    history_sql(
        &owner,
        "DELETE FROM post_secrets.image_history_scopes WHERE scope=$1",
        &board,
    )
    .await;
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
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(&alias_board)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM content.threads WHERE board=$1 ORDER BY id FOR UPDATE")
        .bind(&alias_board)
        .execute(&mut *tx)
        .await
        .unwrap();
    for statement in [
        "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.post_media WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM post_secrets.posting_thread_actions WHERE board=$1",
    ] {
        for owned_board in [&board, &alias_board] {
            sqlx::query(statement)
                .bind(owned_board)
                .execute(&mut *tx)
                .await
                .unwrap();
        }
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
    tx.commit().await.unwrap();
    support::restore_empty_source_board(&owner, &board, &before).await;
    support::restore_empty_source_board(&owner, &alias_board, &alias_before).await;
    public.close().await;
    owner.close().await;
    result.unwrap();
}

// The private queue uses synthetic metadata; image decoding is qualified by the
// media worker suite. Every capability still passes production intake checks.
async fn owned_upload(digest: Option<u8>) -> post_media::NewAttachment {
    let intake = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let upload = intake.reserve("owned-duplicate.png").await.unwrap();
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
    let primary = OutputMetadata {
        sha256: "a".repeat(64),
        bytes: 100,
        width: 200,
        height: 100,
    };
    let Some(digest) = digest else {
        let asset = queue
            .prepare_output(&job.id, token, &primary)
            .await
            .unwrap();
        queue
            .approve_output(&job.id, token, &asset.id)
            .await
            .unwrap();
        return post_media::NewAttachment {
            upload,
            spoiler: false,
        };
    };
    let variants = OutputVariants {
        md5: format!("{digest:02x}").repeat(16),
        thumbnail: OutputMetadata {
            sha256: "b".repeat(64),
            bytes: 50,
            width: 100,
            height: 50,
        },
    };
    let source = SourceProvenance {
        input_sha256: "d".repeat(64),
        input_bytes: 100,
        profile: SourceProfile::PngV1,
        retained_bytes: 100,
        md5: [digest; 16],
    };
    let asset = queue
        .prepare_output_with_provenance(&job.id, token, &primary, Some(&variants), Some(&source))
        .await
        .unwrap();
    let input = InputReceipt {
        component: InputComponent::Upload,
        sha256: "d".repeat(64),
        bytes: 100,
        md5: [digest; 16],
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

async fn history_sql(owner: &PgPool, statement: &'static str, scope: &str) {
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_attachment_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query(statement)
        .bind(scope)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}
