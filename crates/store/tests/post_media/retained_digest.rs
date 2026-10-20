//! A normalized attachment identity survives file cleanup, not content erasure.
use super::*;
use board_store::media_assets::OutputVariants;

async fn digest(f: &Fixture, post: i64) -> Option<Vec<u8>> {
    sqlx::query_scalar("SELECT normalized_md5 FROM content.post_media WHERE post_id=$1")
        .bind(post)
        .fetch_one(&f.admin)
        .await
        .unwrap()
}

pub(super) async fn exercise(f: &Fixture) {
    let upload = f.reserve().await;
    let asset = f
        .approve_with_variants(
            &upload,
            Some(&OutputVariants {
                md5: "12".repeat(16),
                thumbnail: OutputMetadata {
                    sha256: "b".repeat(64),
                    bytes: 40,
                    width: 5,
                    height: 10,
                },
            }),
        )
        .await;
    let post = f.insert(0, &upload).await.unwrap();
    assert_eq!(digest(f, post).await, Some(vec![0x12; 16]));

    // Even the migration caller cannot silently replace captured provenance.
    let error = sqlx::query("UPDATE content.post_media SET normalized_md5=decode(repeat('34',16),'hex') WHERE post_id=$1")
        .bind(post).execute(&f.admin).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
    assert_eq!(digest(f, post).await, Some(vec![0x12; 16]));

    for variable in [
        "TEST_PUBLIC_DATABASE_URL",
        "STAFF_DATABASE_URL",
        "AUTH_DATABASE_URL",
    ] {
        let runtime = PgPool::connect(&std::env::var(variable).unwrap())
            .await
            .unwrap();
        for query in [
            "SELECT normalized_md5 FROM content.post_media WHERE false",
            "SELECT content.retain_attachment_digest()",
        ] {
            let error = sqlx::query(query).execute(&runtime).await.unwrap_err();
            assert_eq!(
                error.as_database_error().unwrap().code().as_deref(),
                Some("42501"),
                "{variable}: {query}"
            );
        }
        runtime.close().await;
    }
    let mut mutation = f.admin.begin().await.unwrap();
    sqlx::query("ALTER TABLE content.post_media DISABLE TRIGGER zz_retain_attachment_digest")
        .execute(&mut *mutation)
        .await
        .unwrap();
    sqlx::query("SET LOCAL ROLE board_thread_transfer_owner")
        .execute(&mut *mutation)
        .await
        .unwrap();
    let ready: bool = sqlx::query_scalar("SELECT content.text_thread_transfer_ready()")
        .fetch_one(&mut *mutation)
        .await
        .unwrap();
    assert!(
        !ready,
        "Disabling capture must invalidate the content catalog"
    );
    mutation.rollback().await.unwrap();

    delete_attachment(&f.public, &f.board, post).await.unwrap();
    assert_eq!(digest(f, post).await, Some(vec![0x12; 16]));
    let visible: (bool, Option<String>) =
        sqlx::query_as("SELECT available,md5 FROM content.staff_post_media WHERE post_id=$1")
            .bind(post)
            .fetch_one(&f.admin)
            .await
            .unwrap();
    assert_eq!(
        visible,
        (false, None),
        "Private history must not reopen deleted media"
    );
    assert!(f.queue.retire_output(&asset).await.unwrap());
    assert!(f.queue.forget_output(&asset).await.unwrap());
    assert_eq!(
        digest(f, post).await,
        Some(vec![0x12; 16]),
        "Asset cleanup must not erase file-only history"
    );

    board_store::delete_post(&f.public, &f.board, post)
        .await
        .unwrap();
    assert_eq!(
        digest(f, post).await,
        None,
        "Whole-post erasure removes retained attachment metadata"
    );
    let erased: bool =
        sqlx::query_scalar("SELECT metadata_erased FROM content.post_media WHERE post_id=$1")
            .bind(post)
            .fetch_one(&f.admin)
            .await
            .unwrap();
    assert!(erased);

    let legacy_post = legacy_metadata_stays_unknown(f).await;
    assert_eq!(
        digest(f, legacy_post).await,
        None,
        "Legacy approval without normalized metadata remains unknown"
    );
}
