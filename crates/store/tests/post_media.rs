#![cfg(feature = "database-tests")]

use board_store::{
    NewPost, StoreError, create_post, create_post_with_attachment,
    media::MediaQueue,
    media_assets::{MediaReader, OutputMetadata},
    media_intake::IntakeStore,
    post_media::{NewAttachment, attachment, delete_attachment},
};
use sqlx::{Executor, PgPool};
use std::sync::{Arc, Mutex};

struct Fixture {
    admin: PgPool,
    public: PgPool,
    intake: IntakeStore,
    queue: MediaQueue,
    reader: MediaReader,
    board: String,
    jobs: Arc<Mutex<Vec<String>>>,
}

fn post() -> NewPost {
    NewPost {
        name: "Anonymous".into(),
        subject: "Attachment fixture".into(),
        comment: "A synthetic attachment".into(),
        deletion_hash: "fixture-not-a-password".into(),
        sage: false,
    }
}

impl Fixture {
    async fn reserve(&self) -> NewAttachment {
        let upload = self.intake.reserve("<synthetic & file>.png").await.unwrap();
        self.jobs.lock().unwrap().push(upload.id.clone());
        NewAttachment {
            upload,
            spoiler: true,
        }
    }

    async fn approve(&self, a: &NewAttachment) -> String {
        self.intake
            .begin_upload(&a.upload.id, &a.upload.capability)
            .await
            .unwrap();
        self.intake
            .finish_upload(&a.upload.id, &a.upload.capability, 100)
            .await
            .unwrap();
        assert!(
            matches!(self.insert(0, a).await, Err(StoreError::Conflict(_))),
            "Queued input cannot attach"
        );
        let claim = self.queue.claim().await.unwrap().unwrap();
        assert_eq!(claim.id, a.upload.id, "Requires an idle disposable queue");
        let token = claim.lease_token.unwrap();
        assert!(
            matches!(self.insert(0, a).await, Err(StoreError::Conflict(_))),
            "Processing input cannot attach"
        );
        let output = self
            .queue
            .prepare_output(
                &claim.id,
                &token,
                &OutputMetadata {
                    sha256: "a".repeat(64),
                    bytes: 123,
                    width: 10,
                    height: 20,
                },
            )
            .await
            .unwrap();
        assert!(
            matches!(self.insert(0, a).await, Err(StoreError::Conflict(_))),
            "Pending output cannot attach"
        );
        self.queue
            .approve_output(&claim.id, &token, &output.id)
            .await
            .unwrap();
        output.id
    }

    async fn insert(&self, thread: i64, a: &NewAttachment) -> Result<i64, StoreError> {
        create_post_with_attachment(&self.public, &self.board, thread, &post(), Some(a)).await
    }

    async fn denied(&self, sql: &'static str) {
        // A successful same-statement administrator control distinguishes denied
        // authority from a missing relation, missing role, or malformed query.
        let mut healthy = self.admin.begin().await.unwrap();
        healthy.execute(sql).await.unwrap();
        healthy.rollback().await.unwrap();
        let error = self.public.execute(sql).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501"),
            "{sql}"
        );
    }
}

#[tokio::test]
async fn attachment_authorization_is_atomic_one_use_and_visible_only_while_live() {
    let admin = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT substr(replace(gen_random_uuid()::text, '-', ''),1,10)")
            .fetch_one(&admin)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES ($1,'Attachment test','Synthetic',2000,100,100,100,10)")
        .bind(&board).execute(&admin).await.unwrap();
    let jobs = Arc::new(Mutex::new(Vec::new()));
    let f = Fixture {
        admin: admin.clone(),
        public,
        intake: IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap())
            .await
            .unwrap(),
        queue: MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
            .await
            .unwrap(),
        reader: MediaReader::connect(&std::env::var("MEDIA_READ_DATABASE_URL").unwrap())
            .await
            .unwrap(),
        board: board.clone(),
        jobs: jobs.clone(),
    };
    let outcome = tokio::spawn(async move { exercise(&f).await }).await;
    // Cleanup runs even when an assertion in the spawned test fails.
    sqlx::query("DELETE FROM content.post_media WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)").bind(&board).execute(&admin).await.unwrap();
    sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)").bind(&board).execute(&admin).await.unwrap();
    sqlx::query("DELETE FROM content.posts WHERE board=$1")
        .bind(&board)
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.threads WHERE board=$1")
        .bind(&board)
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.boards WHERE slug=$1")
        .bind(&board)
        .execute(&admin)
        .await
        .unwrap();
    let ids = jobs.lock().unwrap().clone();
    sqlx::query("DELETE FROM media.assets WHERE job_id=ANY($1)")
        .bind(&ids)
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("DELETE FROM media.jobs WHERE id=ANY($1)")
        .bind(&ids)
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;
    outcome.unwrap();
}

async fn exercise(f: &Fixture) {
    for sql in [
        "SELECT * FROM media.jobs",
        "SELECT * FROM media.assets",
        "SELECT * FROM media_intake.handles",
        "SELECT * FROM content.post_media",
        "SELECT * FROM staff_identity.credentials",
        "SELECT * FROM deployment.settings",
        "UPDATE media.assets SET state=state WHERE false",
        "UPDATE content.post_media SET file_deleted=false WHERE false",
        "DELETE FROM content.post_media WHERE false",
        "UPDATE content.boards SET image_limit=100 WHERE false",
        "SET ROLE board_attachment_owner",
    ] {
        f.denied(sql).await;
    }
    let role_safe: bool = sqlx::query_scalar("SELECT NOT rolcanlogin AND NOT rolsuper AND NOT rolcreatedb AND NOT rolcreaterole AND NOT rolreplication AND NOT rolbypassrls AND NOT EXISTS (SELECT 1 FROM pg_auth_members WHERE member=pg_roles.oid) AND NOT has_schema_privilege(oid,'content','CREATE') AND NOT has_schema_privilege(oid,'staff_identity','USAGE') AND NOT has_schema_privilege(oid,'deployment','USAGE') AND NOT has_any_column_privilege(oid,'media.assets','INSERT,UPDATE,REFERENCES') AND NOT has_column_privilege(oid,'media.jobs','lease_token','SELECT,INSERT,UPDATE') AND NOT has_table_privilege(oid,'media.jobs','DELETE,TRUNCATE,TRIGGER') FROM pg_roles WHERE rolname='board_attachment_owner'")
        .fetch_one(&f.admin).await.unwrap();
    assert!(role_safe);
    let functions_safe: bool = sqlx::query_scalar("SELECT count(*)=4 AND bool_and(p.prosecdef AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp'] AND NOT EXISTS (SELECT 1 FROM aclexplode(p.proacl) a WHERE a.grantee=0)) FROM pg_proc p JOIN pg_roles r ON r.oid=p.proowner WHERE r.rolname='board_attachment_owner'")
        .fetch_one(&f.admin).await.unwrap();
    assert!(functions_safe);

    let a = f.reserve().await;
    let thread = create_post(&f.public, &f.board, 0, &post()).await.unwrap();
    assert!(matches!(
        f.insert(thread, &a).await,
        Err(StoreError::Conflict(_))
    ));
    let asset = f.approve(&a).await;
    // Existing boards remain text-only until explicitly configured.
    assert!(matches!(
        f.insert(thread, &a).await,
        Err(StoreError::Conflict(_))
    ));
    sqlx::query("UPDATE content.boards SET image_limit=2 WHERE slug=$1")
        .bind(&f.board)
        .execute(&f.admin)
        .await
        .unwrap();
    let before = board_store::thread(&f.public, &f.board, thread)
        .await
        .unwrap();
    for capability in [
        String::new(),
        "0".repeat(64),
        "A".repeat(64),
        a.upload.capability.clone() + "0",
    ] {
        let wrong = NewAttachment {
            upload: board_store::media_intake::IntakeReservation {
                id: a.upload.id.clone(),
                capability,
            },
            spoiler: false,
        };
        assert!(matches!(
            f.insert(thread, &wrong).await,
            Err(StoreError::NotFound)
        ));
    }
    let after = board_store::thread(&f.public, &f.board, thread)
        .await
        .unwrap();
    assert_eq!(before.reply_count, after.reply_count);
    assert_eq!(before.modified_at, after.modified_at);

    // A later statement failure must roll back both the insertion and one-use claim.
    let mut invalid = post();
    invalid.deletion_hash = "x".repeat(257);
    assert!(matches!(
        create_post_with_attachment(&f.public, &f.board, thread, &invalid, Some(&a)).await,
        Err(StoreError::Database(_))
    ));
    let (left, right) = tokio::join!(f.insert(thread, &a), f.insert(thread, &a));
    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
    let id = match (left, right) {
        (Ok(id), Err(StoreError::Conflict(_))) | (Err(StoreError::Conflict(_)), Ok(id)) => id,
        _ => panic!("Expected exactly one capability consumer"),
    };
    let saved = attachment(&f.public, id).await.unwrap().unwrap();
    assert_eq!(saved.asset_id, asset);
    assert_eq!(saved.filename, "<synthetic & file>.png");
    assert_eq!((saved.bytes, saved.width, saved.height), (123, 10, 20));
    assert!(saved.spoiler && !saved.file_deleted);
    f.reader.get(&asset).await.unwrap();
    assert_eq!(
        board_store::thread(&f.public, &f.board, thread)
            .await
            .unwrap()
            .reply_count,
        1
    );

    // Separate valid capabilities race for the final image slot.
    let b = f.reserve().await;
    let b_asset = f.approve(&b).await;
    // A caller cannot use a new valid capability to replace another post's file.
    let substituted = sqlx::query(
        "SELECT content.insert_post_attachment($1,$2,$3,'Anonymous','','replacement',$4,$5,false)",
    )
    .bind(id)
    .bind(&f.board)
    .bind(thread)
    .bind(&b.upload.id)
    .bind(&b.upload.capability)
    .execute(&f.public)
    .await
    .unwrap_err();
    assert_eq!(
        substituted.as_database_error().unwrap().code().as_deref(),
        Some("23505")
    );
    assert_eq!(
        attachment(&f.public, id).await.unwrap().unwrap().asset_id,
        asset
    );
    for isolation in [
        "BEGIN ISOLATION LEVEL REPEATABLE READ",
        "BEGIN ISOLATION LEVEL SERIALIZABLE",
    ] {
        let mut connection = f.public.acquire().await.unwrap();
        connection.execute(isolation).await.unwrap();
        let rejected = sqlx::query("SELECT content.insert_post_attachment($1,$2,$3,'Anonymous','','isolation',$4,$5,false)")
            .bind(id).bind(&f.board).bind(thread).bind(&b.upload.id).bind(&b.upload.capability)
            .execute(&mut *connection).await;
        connection.execute("ROLLBACK").await.unwrap();
        assert_eq!(
            rejected
                .unwrap_err()
                .as_database_error()
                .unwrap()
                .code()
                .as_deref(),
            Some("22023")
        );
    }
    let c = f.reserve().await;
    let c_asset = f.approve(&c).await;
    let (left, right) = tokio::join!(f.insert(thread, &b), f.insert(thread, &c));
    assert_eq!(usize::from(left.is_ok()) + usize::from(right.is_ok()), 1);
    let (winner, remaining, remaining_asset) = match (left, right) {
        (Ok(id), Err(StoreError::Conflict(_))) => (id, &c, &c_asset),
        (Err(StoreError::Conflict(_)), Ok(id)) => (id, &b, &b_asset),
        _ => panic!("Expected exactly one image-limit winner"),
    };
    assert!(matches!(
        f.insert(thread, remaining).await,
        Err(StoreError::Conflict(_))
    ));
    let revision = board_store::thread(&f.public, &f.board, thread)
        .await
        .unwrap()
        .modified_at;
    assert!(matches!(
        delete_attachment(&f.public, "wrong", id).await,
        Err(StoreError::NotFound)
    ));
    delete_attachment(&f.public, &f.board, id).await.unwrap();
    assert!(
        attachment(&f.public, id)
            .await
            .unwrap()
            .unwrap()
            .file_deleted
    );
    assert!(matches!(
        f.reader.get(&asset).await,
        Err(StoreError::NotFound)
    ));
    assert!(
        board_store::thread(&f.public, &f.board, thread)
            .await
            .unwrap()
            .modified_at
            > revision
    );
    assert!(matches!(
        delete_attachment(&f.public, &f.board, id).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        f.insert(thread, &a).await,
        Err(StoreError::Conflict(_))
    ));
    let last = f.insert(thread, remaining).await.unwrap();
    f.reader.get(remaining_asset).await.unwrap();
    board_store::delete_post(&f.public, &f.board, last)
        .await
        .unwrap();
    assert!(matches!(
        f.reader.get(remaining_asset).await,
        Err(StoreError::NotFound)
    ));
    assert!(attachment(&f.public, last).await.unwrap().is_none());
    assert!(matches!(
        f.insert(thread, remaining).await,
        Err(StoreError::Conflict(_))
    ));
    assert!(attachment(&f.public, winner).await.unwrap().is_some());

    // Queue metadata can be retired without making the durable attachment vanish.
    sqlx::query(
        "UPDATE media.jobs SET updated_at=clock_timestamp()-interval '2 days' WHERE id=ANY($1)",
    )
    .bind(vec![&a.upload.id, &b.upload.id, &c.upload.id])
    .execute(&f.admin)
    .await
    .unwrap();
    for cap in [&a, &b, &c] {
        assert!(f.queue.forget_terminal(&cap.upload.id).await.unwrap());
    }
    assert!(attachment(&f.public, winner).await.unwrap().is_some());
    assert!(matches!(
        f.insert(thread, &a).await,
        Err(StoreError::NotFound)
    ));
    board_store::delete_post(&f.public, &f.board, thread)
        .await
        .unwrap();
    for asset in [&asset, &b_asset, &c_asset] {
        assert!(matches!(
            f.reader.get(asset).await,
            Err(StoreError::NotFound)
        ));
    }

    // A valid but expired capability cannot publish a new post.
    let expired = f.reserve().await;
    f.approve(&expired).await;
    sqlx::query(
        "UPDATE media.jobs SET created_at=clock_timestamp()-interval '3 hours' WHERE id=$1",
    )
    .bind(&expired.upload.id)
    .execute(&f.admin)
    .await
    .unwrap();
    assert!(matches!(
        f.insert(0, &expired).await,
        Err(StoreError::NotFound)
    ));
    let live_threads: i64 =
        sqlx::query_scalar("SELECT count(*) FROM content.visible_threads WHERE board=$1")
            .bind(&f.board)
            .fetch_one(&f.public)
            .await
            .unwrap();
    assert_eq!(
        live_threads, 0,
        "Failed OP attachment rolled back the new thread"
    );
    exercise_waits_and_moderation(f).await;
    f.public.close().await;
    assert!(matches!(
        f.insert(0, &expired).await,
        Err(StoreError::Database(_))
    ));
}

async fn exercise_waits_and_moderation(f: &Fixture) {
    let failed = f.reserve().await;
    f.intake
        .abort_upload(&failed.upload.id, &failed.upload.capability)
        .await
        .unwrap();
    assert!(matches!(
        f.insert(0, &failed).await,
        Err(StoreError::Conflict(_))
    ));
    let receipt = f.reserve().await;
    f.intake
        .begin_upload(&receipt.upload.id, &receipt.upload.capability)
        .await
        .unwrap();
    f.intake
        .finish_upload(&receipt.upload.id, &receipt.upload.capability, 10)
        .await
        .unwrap();
    let job = f.queue.claim().await.unwrap().unwrap();
    assert_eq!(job.id, receipt.upload.id);
    f.queue
        .complete(&job.id, &job.lease_token.unwrap(), &"b".repeat(64), 10)
        .await
        .unwrap();
    assert!(
        matches!(f.insert(0, &receipt).await, Err(StoreError::Conflict(_))),
        "A legacy completion receipt is not an approval"
    );

    let revoked = f.reserve().await;
    f.approve(&revoked).await;
    sqlx::query("UPDATE media_intake.handles SET capability_hash=sha256(convert_to(repeat('0',64),'UTF8')) WHERE job_id=$1")
        .bind(&revoked.upload.id).execute(&f.admin).await.unwrap();
    assert!(matches!(
        f.insert(0, &revoked).await,
        Err(StoreError::NotFound)
    ));

    // The capability expires while an actual job-row lock is held.
    let a = f.reserve().await;
    f.approve(&a).await;
    let thread = create_post(&f.public, &f.board, 0, &post()).await.unwrap();
    let mut lock = f.admin.begin().await.unwrap();
    sqlx::query("UPDATE media.jobs SET created_at=clock_timestamp()-interval '2 hours'+interval '300 milliseconds' WHERE id=$1")
        .bind(&a.upload.id).execute(&mut *lock).await.unwrap();
    let pool = f.public.clone();
    let board = f.board.clone();
    let waiting = tokio::spawn(async move {
        create_post_with_attachment(&pool, &board, thread, &post(), Some(&a)).await
    });
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    assert!(
        !waiting.is_finished(),
        "Attachment must wait for the job lock"
    );
    lock.commit().await.unwrap();
    assert!(matches!(waiting.await.unwrap(), Err(StoreError::NotFound)));

    // A staff mutation wins the board lock before a waiting attachment post.
    let a = f.reserve().await;
    let asset = f.approve(&a).await;
    let staff = PgPool::connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut moderation = staff.begin().await.unwrap();
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(&f.board)
        .execute(&mut *moderation)
        .await
        .unwrap();
    let pool = f.public.clone();
    let board = f.board.clone();
    let waiting_attachment = NewAttachment {
        upload: board_store::media_intake::IntakeReservation {
            id: a.upload.id.clone(),
            capability: a.upload.capability.clone(),
        },
        spoiler: a.spoiler,
    };
    let waiting = tokio::spawn(async move {
        create_post_with_attachment(&pool, &board, thread, &post(), Some(&waiting_attachment)).await
    });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(
        !waiting.is_finished(),
        "Posting must wait for staff's board lock"
    );
    sqlx::query("UPDATE content.threads SET deleted=true,modified_at=clock_timestamp() WHERE board=$1 AND id=$2")
        .bind(&f.board).bind(thread).execute(&mut *moderation).await.unwrap();
    moderation.commit().await.unwrap();
    assert!(matches!(waiting.await.unwrap(), Err(StoreError::NotFound)));
    let other_thread = f.insert(0, &a).await.unwrap();
    f.reader.get(&asset).await.unwrap();

    // Archival preserves the image until the archive deadline; read authorization
    // changes without waiting for a cleanup job to touch the thread.
    sqlx::query("UPDATE content.boards SET archive_retention_seconds=60 WHERE slug=$1")
        .bind(&f.board)
        .execute(&f.admin)
        .await
        .unwrap();
    sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp()-interval '1 second',archive_expires_at=clock_timestamp()+interval '60 seconds' WHERE id=$1")
        .bind(other_thread).execute(&f.admin).await.unwrap();
    f.reader.get(&asset).await.unwrap();
    sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp()-interval '2 seconds',archive_expires_at=clock_timestamp()-interval '1 second' WHERE id=$1")
        .bind(other_thread).execute(&f.admin).await.unwrap();
    assert!(matches!(
        f.reader.get(&asset).await,
        Err(StoreError::NotFound)
    ));
    assert!(attachment(&f.public, other_thread).await.unwrap().is_none());

    let a = f.reserve().await;
    let asset = f.approve(&a).await;
    let id = f.insert(0, &a).await.unwrap();
    delete_attachment(&staff, &f.board, id).await.unwrap();
    assert!(attachment(&staff, id).await.unwrap().unwrap().file_deleted);
    assert!(matches!(
        f.reader.get(&asset).await,
        Err(StoreError::NotFound)
    ));
    let b = f.reserve().await;
    let b_asset = f.approve(&b).await;
    let reply = f.insert(id, &b).await.unwrap();
    sqlx::query("UPDATE content.posts SET deleted=true WHERE board=$1 AND id=$2")
        .bind(&f.board)
        .bind(reply)
        .execute(&staff)
        .await
        .unwrap();
    assert!(matches!(
        f.reader.get(&b_asset).await,
        Err(StoreError::NotFound)
    ));
    let c = f.reserve().await;
    let c_asset = f.approve(&c).await;
    f.insert(id, &c).await.unwrap();
    sqlx::query("UPDATE content.threads SET deleted=true,modified_at=clock_timestamp() WHERE board=$1 AND id=$2")
        .bind(&f.board).bind(id).execute(&staff).await.unwrap();
    assert!(matches!(
        f.reader.get(&c_asset).await,
        Err(StoreError::NotFound)
    ));
    staff.close().await;
    cancellation(f).await;
}

async fn cancellation(f: &Fixture) {
    use board_store::post_media::{cancel_upload, check_upload};
    let a = f.reserve().await;
    f.approve(&a).await;
    check_upload(&f.public, &a.upload.id, &a.upload.capability)
        .await
        .unwrap();
    assert!(matches!(
        check_upload(&f.public, &a.upload.id, &"0".repeat(64)).await,
        Err(StoreError::NotFound)
    ));
    for statement in [
        "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ",
        "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE",
    ] {
        let mut tx = f.public.begin().await.unwrap();
        tx.execute(statement).await.unwrap();
        let error = sqlx::query("SELECT content.cancel_attachment_upload($1,$2)")
            .bind(&a.upload.id)
            .bind(&a.upload.capability)
            .execute(&mut *tx)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("22023")
        );
        tx.rollback().await.unwrap();
    }
    // Either attach or cancel wins; the job lock prevents both from succeeding.
    let (inserted, cancelled) = tokio::join!(
        f.insert(0, &a),
        cancel_upload(&f.public, &a.upload.id, &a.upload.capability)
    );
    assert_eq!(
        usize::from(inserted.is_ok()) + usize::from(cancelled.is_ok()),
        1
    );
    assert!(
        check_upload(&f.public, &a.upload.id, &a.upload.capability)
            .await
            .is_err()
    );
    assert!(f.insert(0, &a).await.is_err());
    if inserted.is_ok() {
        assert!(matches!(cancelled, Err(StoreError::Conflict(_))));
    } else {
        assert!(matches!(inserted, Err(StoreError::NotFound)));
    }
    let expired = f.reserve().await;
    f.approve(&expired).await;
    sqlx::query(
        "UPDATE media.jobs SET created_at=clock_timestamp()-interval '3 hours' WHERE id=$1",
    )
    .bind(&expired.upload.id)
    .execute(&f.admin)
    .await
    .unwrap();
    assert!(matches!(
        check_upload(&f.public, &expired.upload.id, &expired.upload.capability).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        cancel_upload(&f.public, &expired.upload.id, &expired.upload.capability).await,
        Err(StoreError::NotFound)
    ));
}
