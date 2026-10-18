use super::*;
use board_store::media_assets::OutputVariants;

async fn issue(f: &Fixture, target: i64, template: i64, pool: &PgPool) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar("SELECT content.issue_post_template_restriction($1,$2,600,$3,$4,$5)")
        .bind(&f.session)
        .bind(&f.csrf)
        .bind(&f.board)
        .bind(target)
        .bind(template)
        .fetch_one(pool)
        .await
}

async fn snapshot(f: &Fixture, ban: i64) -> (i64, String, Option<Vec<u8>>, i64) {
    let mut tx = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let row = sqlx::query_as("SELECT template_id,upload_observation,normalized_upload_md5,charged_bytes FROM restrictions.bans WHERE id=$1")
        .bind(ban).fetch_one(&mut *tx).await.unwrap();
    tx.rollback().await.unwrap();
    row
}

pub(super) async fn template_history(f: &Fixture) {
    let upload = f
        .approved_with_variants(
            "Owned normalized.png",
            false,
            Some(&OutputVariants {
                md5: "52".repeat(16),
                thumbnail: OutputMetadata {
                    sha256: "b".repeat(64),
                    bytes: 40,
                    width: 5,
                    height: 10,
                },
            }),
        )
        .await;
    let target = f
        .write(0, Some(&upload), true, &post(), 71, Utc::now())
        .await
        .unwrap();
    let legacy_upload = f.approved("Owned legacy.png", false).await;
    let legacy = f
        .write(0, Some(&legacy_upload), true, &post(), 72, Utc::now())
        .await
        .unwrap();
    let text = f
        .write(0, None, true, &post(), 73, Utc::now())
        .await
        .unwrap();
    let mut tx = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let template: i64 = sqlx::query_scalar("INSERT INTO restrictions.templates(id,name,minimum_role,global,warning,seconds,public_reason,private_reason) VALUES(1900000000+floor(random()*1000000)::bigint,'Owned synthetic template','moderator',true,false,60,'Owned public','Owned private') RETURNING id")
        .fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    f.templates.lock().unwrap().push(template);
    for pool in [&f.public, &f.auth] {
        let error = issue(f, target, template, pool).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501")
        );
    }
    let first = issue(f, target, template, &f.staff).await.unwrap();
    let charge = (512 + "Owned public".len() + "Owned private".len() + 80) as i64;
    assert_eq!(
        snapshot(f, first).await,
        (template, "known".into(), Some(vec![0x52; 16]), charge)
    );
    let unknown = issue(f, legacy, template, &f.staff).await.unwrap();
    assert_eq!(
        snapshot(f, unknown).await,
        (template, "unknown".into(), None, charge - 16)
    );
    let absent = issue(f, text, template, &f.staff).await.unwrap();
    assert_eq!(
        snapshot(f, absent).await,
        (template, "absent".into(), None, charge - 16)
    );
    // Source file-only deletion retains MD5 even for a later template ban.
    board_store::post_media::delete_attachment(&f.staff, &f.board, target)
        .await
        .unwrap();
    let asset: String =
        sqlx::query_scalar("SELECT asset_id FROM content.post_media WHERE post_id=$1")
            .bind(target)
            .fetch_one(&f.owner)
            .await
            .unwrap();
    assert!(f.queue.retire_output(&asset).await.unwrap());
    assert!(f.queue.forget_output(&asset).await.unwrap());
    let second = issue(f, target, template, &f.staff).await.unwrap();
    assert_eq!(
        snapshot(f, second).await,
        (template, "known".into(), Some(vec![0x52; 16]), charge)
    );

    // File-required presets reject the tombstone and roll back the base issuer.
    let mut tx = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE restrictions.templates SET requires_file=true WHERE id=$1")
        .bind(template)
        .execute(&mut *tx)
        .await
        .unwrap();
    let before: (i64,i64,i64) = sqlx::query_as("SELECT used_bytes,byte_limit,(SELECT count(*) FROM restrictions.bans WHERE template_id=$1),(SELECT count(*) FROM restrictions.audit a JOIN restrictions.bans b ON b.id=a.ban_id WHERE b.template_id=$1) FROM restrictions.capacity WHERE singleton")
        .bind(template).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let error = issue(f, target, template, &f.staff).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("23514")
    );
    let mut tx = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let after: (i64,i64,i64) = sqlx::query_as("SELECT used_bytes,byte_limit,(SELECT count(*) FROM restrictions.bans WHERE template_id=$1),(SELECT count(*) FROM restrictions.audit a JOIN restrictions.bans b ON b.id=a.ban_id WHERE b.template_id=$1) FROM restrictions.capacity WHERE singleton")
        .bind(template).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(before, after);
    sqlx::query("UPDATE restrictions.templates SET requires_file=false WHERE id=$1")
        .bind(template)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE restrictions.capacity SET byte_limit=used_bytes+$1-1 WHERE singleton")
        .bind(charge)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let refused = issue(f, target, template, &f.staff).await;
    let mut tx = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("UPDATE restrictions.capacity SET byte_limit=$1 WHERE singleton")
        .bind(before.1)
        .execute(&mut *tx)
        .await
        .unwrap();
    let after: (i64,i64,i64) = sqlx::query_as("SELECT used_bytes,byte_limit,(SELECT count(*) FROM restrictions.bans WHERE template_id=$1),(SELECT count(*) FROM restrictions.audit a JOIN restrictions.bans b ON b.id=a.ban_id WHERE b.template_id=$1) FROM restrictions.capacity WHERE singleton")
        .bind(template).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    assert_eq!(
        refused
            .unwrap_err()
            .as_database_error()
            .unwrap()
            .code()
            .as_deref(),
        Some("P134C")
    );
    assert_eq!(
        before, after,
        "Snapshot capacity refusal must roll back base ban and audit"
    );
    board_store::delete_post(&f.staff, &f.board, target)
        .await
        .unwrap();
    assert_eq!(
        snapshot(f, first).await,
        (template, "known".into(), Some(vec![0x52; 16]), charge)
    );
    assert_eq!(
        snapshot(f, second).await,
        (template, "known".into(), Some(vec![0x52; 16]), charge)
    );
    let error = issue(f, target, template, &f.staff).await.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("P134N")
    );
}
