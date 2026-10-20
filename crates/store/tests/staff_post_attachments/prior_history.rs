//! Real template issuance supplies history; request fields cannot supply it.
use super::*;
use board_store::media_assets::OutputVariants;

async fn upload(f: &Fixture, digest: Option<u8>) -> NewAttachment {
    let variants = digest.map(|byte| OutputVariants {
        md5: format!("{byte:02x}").repeat(16),
        thumbnail: OutputMetadata {
            sha256: "b".repeat(64),
            bytes: 40,
            width: 5,
            height: 10,
        },
    });
    f.approved_with_variants("Owned history.png", false, variants.as_ref())
        .await
}
async fn issue(f: &Fixture, target: i64, template: i64) -> i64 {
    sqlx::query_scalar("SELECT content.issue_post_template_restriction($1,$2,600,$3,$4,$5)")
        .bind(&f.session)
        .bind(&f.csrf)
        .bind(&f.board)
        .bind(target)
        .bind(template)
        .fetch_one(&f.staff)
        .await
        .unwrap()
}
async fn decision(f: &Fixture, upload: &NewAttachment, board: &str) -> Option<bool> {
    sqlx::query_scalar("SELECT content.prior_upload_banned($1,$2,$3,0,false,false)")
        .bind(&upload.upload.id)
        .bind(&upload.upload.capability)
        .bind(board)
        .fetch_one(&f.public)
        .await
        .unwrap()
}
async fn purge(f: &Fixture, ban: i64) {
    let mut tx = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT restrictions.gate(true)")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM restrictions.audit WHERE ban_id=$1")
        .bind(ban)
        .execute(&mut *tx)
        .await
        .unwrap();
    let bytes: i64 =
        sqlx::query_scalar("DELETE FROM restrictions.bans WHERE id=$1 RETURNING charged_bytes")
            .bind(ban)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    sqlx::query("UPDATE restrictions.capacity SET used_bytes=used_bytes-$1 WHERE singleton")
        .bind(bytes)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}

// The writer pins trigger configuration after board/thread locks and before
// waiting for a media job; it holds the INSERT lock mode without an upgrade.
async fn guard_relation_lifetime(f: &Fixture, upload: &NewAttachment, thread: i64) {
    let mut held = f.owner.begin().await.unwrap();
    sqlx::query("SELECT id FROM media.jobs WHERE id=$1 FOR UPDATE")
        .bind(&upload.upload.id)
        .execute(&mut *held)
        .await
        .unwrap();
    let mut writing = f.public.begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *writing)
        .await
        .unwrap();
    sqlx::query("SET LOCAL lock_timeout='5s'")
        .execute(&mut *writing)
        .await
        .unwrap();
    let board = f.board.clone();
    let job = upload.upload.id.clone();
    let capability = upload.upload.capability.clone();
    let task = tokio::spawn(async move {
        let result = sqlx::query("SELECT content.insert_post_attachment(NULL,$1,$2,'Anonymous','','Owned guard lifetime',$3,$4,false)")
            .bind(board).bind(thread).bind(job).bind(capability).execute(&mut *writing).await;
        writing.rollback().await.unwrap();
        result
    });
    let observed = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            let pinned: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE pid=$1 AND relation='content.post_media'::regclass AND mode='RowExclusiveLock' AND granted) AND EXISTS(SELECT 1 FROM pg_locks WHERE pid=$1 AND NOT granted)")
                .bind(pid).fetch_one(&f.owner).await.unwrap();
            if pinned { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await;
    let mut ddl = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL lock_timeout='50ms'")
        .execute(&mut *ddl)
        .await
        .unwrap();
    let blocked =
        sqlx::query("ALTER TABLE content.post_media DISABLE TRIGGER zzz_prior_upload_history")
            .execute(&mut *ddl)
            .await;
    ddl.rollback().await.unwrap();
    held.rollback().await.unwrap();
    // Drain before asserting so a failed observation cannot leak a waiting task.
    let _ = task.await.unwrap();
    observed.expect("Attachment writer pins trigger configuration before its media-job wait");
    let error = blocked.expect_err("Trigger DDL must wait for the in-flight writer");
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("55P03")
    );
}

pub(super) async fn prior_history(f: &Fixture) {
    sqlx::query("UPDATE content.boards SET worksafe=false WHERE slug=$1")
        .bind(&f.board)
        .execute(&f.owner)
        .await
        .unwrap();
    let bad_source = upload(f, Some(0x62)).await;
    let source = f
        .write(0, Some(&bad_source), true, &post(), 71, Utc::now())
        .await
        .unwrap();
    let legacy_source = upload(f, None).await;
    let legacy = f.legacy_unknown(&legacy_source, 72).await;
    let plain = f
        .write(0, None, true, &post(), 73, Utc::now())
        .await
        .unwrap();
    let good_source = upload(f, Some(0x63)).await;
    let good_target = f
        .write(0, Some(&good_source), true, &post(), 74, Utc::now())
        .await
        .unwrap();
    let other_board: String =
        sqlx::query_scalar("SELECT 'h'||substr(replace(gen_random_uuid()::text,'-',''),1,9)")
            .fetch_one(&f.owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(captcha_enabled,slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,user_thread_limit) VALUES(false,$1,'Owned history scope','Synthetic',2000,100,100,100,10,100,0,0,0,100)")
        .bind(&other_board).execute(&f.owner).await.unwrap();
    f.extra_boards.lock().unwrap().push(other_board.clone());
    let mut other = f.clone();
    other.board = other_board;
    let foreign_source = upload(&other, Some(0x62)).await;
    let foreign = other
        .write(0, Some(&foreign_source), true, &post(), 75, Utc::now())
        .await
        .unwrap();
    let candidate = upload(f, Some(0x62)).await;
    let good = upload(f, Some(0x63)).await;
    let unknown_upload = upload(f, None).await;
    guard_relation_lifetime(f, &good, source).await;
    let mut config = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *config)
        .await
        .unwrap();
    for id in [6_i64, 226] {
        sqlx::query("INSERT INTO restrictions.templates(id,name,minimum_role,global,warning,seconds,public_reason,private_reason) VALUES($1,'Owned history template','moderator',false,true,0,'Owned history','Synthetic')")
            .bind(id).execute(&mut *config).await.unwrap();
    }
    config.commit().await.unwrap();
    f.templates.lock().unwrap().extend([6, 226]);
    // No-file bans cannot contribute even when their template ID qualifies.
    issue(f, plain, 226).await;
    issue(f, plain, 226).await;
    assert_eq!(decision(f, &unknown_upload, &f.board).await, Some(false));
    let first = issue(f, source, 226).await;
    let retired: bool = sqlx::query_scalar("SELECT content.unban_restriction($1,$2,600,$3)")
        .bind(&f.session)
        .bind(&f.csrf)
        .bind(first)
        .fetch_one(&f.staff)
        .await
        .unwrap();
    assert!(retired);
    let mut config = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *config)
        .await
        .unwrap();
    sqlx::query("UPDATE restrictions.templates SET warning=false,seconds=1 WHERE id=226")
        .execute(&mut *config)
        .await
        .unwrap();
    config.commit().await.unwrap();
    let second = issue(f, source, 226).await;
    tokio::time::sleep(Duration::from_millis(1100)).await;
    let mut config = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *config)
        .await
        .unwrap();
    let expired: bool =
        sqlx::query_scalar("SELECT ends_at<clock_timestamp() FROM restrictions.bans WHERE id=$1")
            .bind(second)
            .fetch_one(&mut *config)
            .await
            .unwrap();
    assert!(expired);
    sqlx::query("UPDATE restrictions.templates SET warning=true,seconds=0 WHERE id=226")
        .execute(&mut *config)
        .await
        .unwrap();
    config.commit().await.unwrap();
    assert_eq!(decision(f, &candidate, &f.board).await, Some(false));
    assert_eq!(decision(f, &unknown_upload, &f.board).await, Some(false));
    // A decision keeps the shared global history gate until its transaction
    // ends. Use a different board/actor so no target-row lock can fake this proof.
    let mut held = f.public.begin().await.unwrap();
    let observed: Option<bool> =
        sqlx::query_scalar("SELECT content.prior_upload_banned($1,$2,$3,0,false,false)")
            .bind(&candidate.upload.id)
            .bind(&candidate.upload.capability)
            .bind(&f.board)
            .fetch_one(&mut *held)
            .await
            .unwrap();
    assert_eq!(observed, Some(false));
    let contended =
        sqlx::query("SELECT content.issue_post_template_restriction($1,$2,600,$3,$4,226)")
            .bind(&other.session)
            .bind(&other.csrf)
            .bind(&other.board)
            .bind(foreign)
            .execute(&other.staff)
            .await;
    held.rollback().await.unwrap();
    let error = contended.unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("55P03"),
        "{error}"
    );
    assert_eq!(decision(f, &candidate, &f.board).await, Some(false));
    let after_release = issue(&other, foreign, 226).await;
    assert_eq!(decision(f, &candidate, &f.board).await, Some(true));
    purge(f, after_release).await;
    assert_eq!(decision(f, &candidate, &f.board).await, Some(false));
    let uncertain = issue(f, legacy, 226).await;
    assert_eq!(
        decision(f, &candidate, &f.board).await,
        None,
        "Two exact plus one unknown event is uncertain"
    );
    assert_eq!(decision(f, &good, &f.board).await, Some(false));
    assert_eq!(decision(f, &unknown_upload, &f.board).await, None);
    let third = issue(&other, foreign, 226).await;
    assert_eq!(
        decision(f, &candidate, &f.board).await,
        Some(true),
        "Retired warnings, expired duplicate events, and other boards all count"
    );
    assert_eq!(decision(f, &good, &f.board).await, Some(false));
    let error = sqlx::query("SELECT content.prior_upload_banned($1,$2,$3,0,false,false)")
        .bind(&candidate.upload.id)
        .bind(&unknown_upload.upload.capability)
        .bind(&f.board)
        .execute(&f.public)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("P0002")
    );
    // Prove source ordering: a quiet content rule must not record a hit first.
    let rule:i64=sqlx::query_scalar("INSERT INTO admission.rules(board,pattern,quiet) VALUES($1,'Synthetic attachment body',true) RETURNING id")
        .bind(&f.board).fetch_one(&f.owner).await.unwrap();
    for (ordinary, ip) in [(true, 82), (false, 83)] {
        assert!(
            matches!(f.write(0,Some(&candidate),ordinary,&post(),ip,Utc::now()).await,
            Err(StoreError::ContentRejected(message)) if message=="Error: Upload failed.")
        );
    }
    let effects:i64=sqlx::query_scalar("SELECT (SELECT count(*) FROM admission.hits WHERE board=$1)+(SELECT count(*) FROM admission.logs WHERE board=$1)+(SELECT count(*) FROM admission.bans WHERE board=$1)")
        .bind(&f.board).fetch_one(&f.owner).await.unwrap();
    assert_eq!(effects, 0);
    sqlx::query("DELETE FROM admission.rules WHERE id=$1")
        .bind(rule)
        .execute(&f.owner)
        .await
        .unwrap();
    assert!(matches!(
        f.write(0, Some(&unknown_upload), true, &post(), 84, Utc::now())
            .await,
        Err(StoreError::Database(_))
    ));
    f.write(0, None, true, &post(), 85, Utc::now())
        .await
        .unwrap();
    assert_eq!(decision(f, &candidate, "b").await, Some(false));

    // The raw attachment API does not derive cosmetic poster IDs. Use the
    // fixture's no-ID board policy for this direct-definer control, then restore it.
    sqlx::query("UPDATE content.boards SET user_ids=false WHERE slug=$1")
        .bind(&f.board)
        .execute(&f.owner)
        .await
        .unwrap();
    let key = PosterIdKey::parse(&f.key).unwrap();
    let actor = key.public_posting_rate_identity(peer(93));
    let encoded: String = actor
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    for (receipt, blocked) in [(&good, false), (&candidate, true)] {
        let mut direct = f.public.begin().await.unwrap();
        sqlx::query("SELECT content.lock_posting_actor($1,false)")
            .bind(actor.as_bytes().as_slice())
            .execute(&mut *direct)
            .await
            .unwrap();
        sqlx::query("SELECT set_config('board.posting_actor',$1,true)")
            .bind(&encoded)
            .execute(&mut *direct)
            .await
            .unwrap();
        let id: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
            .fetch_one(&mut *direct)
            .await
            .unwrap();
        let outcome=sqlx::query("SELECT content.insert_post_attachment($1,$2,$3,'Anonymous','','Owned late guard',$4,$5,false)")
            .bind(id).bind(&f.board).bind(source).bind(&receipt.upload.id).bind(&receipt.upload.capability)
            .execute(&mut *direct).await;
        direct.rollback().await.unwrap();
        if blocked {
            let error = outcome.unwrap_err();
            assert_eq!(
                error.as_database_error().unwrap().code().as_deref(),
                Some("P196B"),
                "{error}"
            );
        } else {
            outcome.expect("The same raw attachment path accepts a nonmatching digest");
        }
    }
    sqlx::query("UPDATE content.boards SET user_ids=true WHERE slug=$1")
        .bind(&f.board)
        .execute(&f.owner)
        .await
        .unwrap();
    let used: i64 = sqlx::query_scalar("SELECT count(*) FROM content.post_media WHERE job_id=$1")
        .bind(&candidate.upload.id)
        .fetch_one(&f.owner)
        .await
        .unwrap();
    assert_eq!(used, 0);

    // File-rule provenance with the same numeric ID is never template history.
    let mut records = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *records)
        .await
        .unwrap();
    for _ in 0..3 {
        sqlx::query("SELECT restrictions.record_file_ban(decode(repeat('e1',32),'hex'),NULL,$1,226,decode(repeat('63',16),'hex'),3,false,'Owned file rule','Synthetic','UTC')")
            .bind(&f.board).execute(&mut *records).await.unwrap();
    }
    records.commit().await.unwrap();
    assert_eq!(decision(f, &good, &f.board).await, Some(false));
    for _ in 0..3 {
        issue(f, good_target, 6).await;
    }
    assert_eq!(decision(f, &good, &f.board).await, Some(false));
    sqlx::query("UPDATE content.boards SET worksafe=true WHERE slug=$1")
        .bind(&f.board)
        .execute(&f.owner)
        .await
        .unwrap();
    assert_eq!(decision(f, &good, &f.board).await, Some(true));
    sqlx::query("UPDATE content.boards SET worksafe=false WHERE slug=$1")
        .bind(&f.board)
        .execute(&f.owner)
        .await
        .unwrap();
    // The live OP duplicate rule is independent of this retained-template
    // decision. Retire that file through the real operation before the positive
    // write; its template events and captured digest must remain observable.
    board_store::post_media::delete_attachment(&f.staff, &f.board, good_target)
        .await
        .unwrap();
    assert_eq!(decision(f, &good, &f.board).await, Some(false));
    f.write(0, Some(&good), true, &post(), 86, Utc::now())
        .await
        .unwrap();
    purge(f, third).await;
    assert_eq!(decision(f, &candidate, &f.board).await, None);
    purge(f, uncertain).await;
    assert_eq!(decision(f, &candidate, &f.board).await, Some(false));
}
