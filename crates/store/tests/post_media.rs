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
    attachment_only: bool,
}

fn post() -> NewPost {
    NewPost {
        name: "😀".repeat(25),
        subject: "Attachment fixture".into(),
        comment: "A synthetic\r\nattachment\rcomment".into(),
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
        let mut post = post();
        if self.attachment_only {
            post.comment.clear();
        }
        create_post_with_attachment(&self.public, &self.board, thread, &post, Some(a)).await
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
    // Sequential cases share an otherwise idle disposable media queue.
    run(false).await;
    run(true).await;
}

async fn run(attachment_only: bool) {
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
        attachment_only,
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
        "SELECT * FROM content.media_clock",
        "UPDATE content.media_clock SET last_number=last_number WHERE false",
        "SELECT content.next_media_number()",
        "SET ROLE board_attachment_owner",
        "UPDATE content.posts SET created_at=created_at WHERE false",
        "UPDATE content.threads SET created_at=created_at WHERE false",
        "UPDATE content.threads SET http_modified_at=http_modified_at WHERE false",
    ] {
        f.denied(sql).await;
    }
    let role_safe: bool = sqlx::query_scalar("SELECT NOT rolcanlogin AND NOT rolsuper AND NOT rolcreatedb AND NOT rolcreaterole AND NOT rolreplication AND NOT rolbypassrls AND NOT EXISTS (SELECT 1 FROM pg_auth_members WHERE member=pg_roles.oid) AND NOT has_schema_privilege(oid,'content','CREATE') AND NOT has_schema_privilege(oid,'staff_identity','USAGE') AND NOT has_schema_privilege(oid,'deployment','USAGE') AND NOT has_any_column_privilege(oid,'media.assets','INSERT,UPDATE,REFERENCES') AND NOT has_column_privilege(oid,'media.jobs','lease_token','SELECT,INSERT,UPDATE') AND NOT has_table_privilege(oid,'media.jobs','DELETE,TRUNCATE,TRIGGER') FROM pg_roles WHERE rolname='board_attachment_owner'")
        .fetch_one(&f.admin).await.unwrap();
    assert!(role_safe);
    let functions_safe: bool = sqlx::query_scalar("SELECT count(*)=7 AND bool_and(p.prosecdef AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp'] AND NOT EXISTS (SELECT 1 FROM aclexplode(p.proacl) a WHERE a.grantee=0)) FROM pg_proc p JOIN pg_roles r ON r.oid=p.proowner WHERE r.rolname='board_attachment_owner'")
        .fetch_one(&f.admin).await.unwrap();
    assert!(functions_safe);

    let a = f.reserve().await;
    let thread = create_post(&f.public, &f.board, 0, &post()).await.unwrap();
    if f.attachment_only {
        reject_unattached_empty_posts(f, thread).await;
    }
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
    if f.attachment_only {
        invalid.comment.clear();
    }
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
    assert_eq!(
        board_store::find_post(&f.public, &f.board, id)
            .await
            .unwrap()
            .name,
        "😀".repeat(25)
    );
    assert_eq!(
        board_store::find_post(&f.public, &f.board, id)
            .await
            .unwrap()
            .comment,
        if f.attachment_only {
            ""
        } else {
            "A synthetic\nattachment\ncomment"
        }
    );
    assert_eq!(saved.asset_id, asset);
    assert_eq!(saved.filename, "<synthetic & file>.png");
    assert_eq!((saved.bytes, saved.width, saved.height), (123, 10, 20));
    assert!(saved.spoiler && !saved.file_deleted);
    f.reader.get(&asset).await.unwrap();
    assert_eq!(
        f.reader
            .get_post(&f.board, saved.tim, false)
            .await
            .unwrap()
            .id,
        asset
    );
    assert!(matches!(
        f.reader.get_post("wrong", saved.tim, false).await,
        Err(StoreError::NotFound)
    ));
    assert!(
        matches!(
            f.reader.get_post(&f.board, saved.tim, true).await,
            Err(StoreError::NotFound)
        ),
        "Legacy metadata must not invent a thumbnail"
    );
    assert!(
        saved.md5.is_none() && saved.thumbnail_width.is_none() && saved.thumbnail_height.is_none()
    );
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
    // Permaage suppresses the indicator but does not exempt image admission.
    sqlx::query("UPDATE content.threads SET sticky=false,permaage=true,undead=false WHERE id=$1")
        .bind(thread)
        .execute(&f.admin)
        .await
        .unwrap();
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
    sqlx::query("UPDATE content.threads SET sticky=false,permaage=false,undead=false WHERE id=$1")
        .bind(thread)
        .execute(&f.admin)
        .await
        .unwrap();
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
    assert!(matches!(
        f.reader.get_post(&f.board, saved.tim, false).await,
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
    tail_counts(f).await;
    posting_times(f).await;
    image_admission_flags(f).await;
    f.public.close().await;
    assert!(matches!(
        f.insert(0, &expired).await,
        Err(StoreError::Database(_))
    ));
}

async fn tail_counts(f: &Fixture) {
    sqlx::query("UPDATE content.boards SET image_limit=2,json_tail_size=1 WHERE slug=$1")
        .bind(&f.board)
        .execute(&f.admin)
        .await
        .unwrap();
    let thread = create_post(&f.public, &f.board, 0, &post()).await.unwrap();
    let first = f.reserve().await;
    f.approve(&first).await;
    let image = f.insert(thread, &first).await.unwrap();
    let text = create_post(&f.public, &f.board, thread, &post())
        .await
        .unwrap();
    let tail = board_store::thread_snapshot_selection(&f.public, &f.board, thread, true)
        .await
        .unwrap();
    assert_eq!(tail.replies, 2);
    assert_eq!(tail.images, 1);
    assert_eq!(tail.tail_id, Some(image));
    assert_eq!(tail.posts[1].id, text);
    assert!(tail.posts.iter().all(|post| post.attachment.is_none()));
    delete_attachment(&f.public, &f.board, image).await.unwrap();
    let tail = board_store::thread_snapshot_selection(&f.public, &f.board, thread, true)
        .await
        .unwrap();
    assert_eq!(tail.images, 0);
    let last = f.reserve().await;
    let asset = f.approve(&last).await;
    f.insert(thread, &last).await.unwrap();
    let tail = board_store::thread_snapshot_selection(&f.public, &f.board, thread, true)
        .await
        .unwrap();
    assert_eq!(tail.images, 1);
    assert!(tail.posts[1].attachment.is_some());
    sqlx::query("UPDATE media.assets SET state='deleting',approved_at=NULL WHERE id=$1")
        .bind(&asset)
        .execute(&f.admin)
        .await
        .unwrap();
    let tail = board_store::thread_snapshot_selection(&f.public, &f.board, thread, true)
        .await
        .unwrap();
    assert_eq!(tail.images, 0);
    assert!(tail.posts[1].attachment.as_ref().unwrap().file_deleted);
}

async fn image_admission_flags(f: &Fixture) {
    sqlx::query("UPDATE content.boards SET image_limit=1 WHERE slug=$1")
        .bind(&f.board)
        .execute(&f.admin)
        .await
        .unwrap();
    let thread = create_post(&f.public, &f.board, 0, &post()).await.unwrap();
    let first = f.reserve().await;
    f.approve(&first).await;
    f.insert(thread, &first).await.unwrap();
    for sticky in [false, true] {
        for undead in [false, true] {
            for permaage in [false, true] {
                for permasage in [false, true] {
                    sqlx::query("UPDATE content.threads SET sticky=$2,undead=$3,permaage=$4,permasage=$5 WHERE id=$1")
                        .bind(thread).bind(sticky).bind(undead).bind(permaage).bind(permasage).execute(&f.admin).await.unwrap();
                    let candidate = f.reserve().await;
                    f.approve(&candidate).await;
                    let before = board_store::thread(&f.public, &f.board, thread)
                        .await
                        .unwrap();
                    let result = f.insert(thread, &candidate).await;
                    if sticky || undead {
                        let id = result.unwrap();
                        assert!(attachment(&f.public, id).await.unwrap().is_some());
                        assert_eq!(
                            board_store::thread(&f.public, &f.board, thread)
                                .await
                                .unwrap()
                                .reply_count,
                            before.reply_count + 1
                        );
                        assert!(
                            matches!(
                                f.insert(thread, &candidate).await,
                                Err(StoreError::Conflict(_))
                            ),
                            "exemption cannot reuse a capability"
                        );
                        delete_attachment(&f.public, &f.board, id).await.unwrap();
                    } else {
                        assert!(matches!(result, Err(StoreError::Conflict(_))));
                        let after = board_store::thread(&f.public, &f.board, thread)
                            .await
                            .unwrap();
                        assert_eq!(
                            (after.reply_count, after.modified_at, after.http_modified_at),
                            (
                                before.reply_count,
                                before.modified_at,
                                before.http_modified_at
                            )
                        );
                    }
                }
            }
        }
    }
    // A flag change committed while a real public writer waits must decide
    // admission after the lock, both when granting and removing the exemption.
    for exempt in [true, false] {
        let candidate = f.reserve().await;
        f.approve(&candidate).await;
        let mut held = f.admin.begin().await.unwrap();
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *held)
            .await
            .unwrap();
        sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
            .bind(&f.board)
            .execute(&mut *held)
            .await
            .unwrap();
        sqlx::query("UPDATE content.threads SET sticky=$2,undead=false WHERE id=$1")
            .bind(thread)
            .bind(exempt)
            .execute(&mut *held)
            .await
            .unwrap();
        let pool = f.public.clone();
        let board = f.board.clone();
        let mut draft = post();
        if f.attachment_only {
            draft.comment.clear();
        }
        let writer = tokio::spawn(async move {
            create_post_with_attachment(&pool, &board, thread, &draft, Some(&candidate)).await
        });
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let blocked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE NOT granted AND $1=ANY(pg_blocking_pids(pid)))")
                    .bind(pid).fetch_one(&f.admin).await.unwrap();
                if blocked { break; }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }).await.expect("healthy public attachment writer reaches owned board lock");
        held.commit().await.unwrap();
        let result = writer.await.unwrap();
        if exempt {
            delete_attachment(&f.public, &f.board, result.unwrap())
                .await
                .unwrap();
        } else {
            assert!(matches!(result, Err(StoreError::Conflict(_))));
        }
    }
    sqlx::query("UPDATE content.threads SET sticky=true,undead=true WHERE id=$1")
        .bind(thread)
        .execute(&f.admin)
        .await
        .unwrap();
    let candidate = f.reserve().await;
    f.approve(&candidate).await;
    sqlx::query("UPDATE content.boards SET image_limit=0 WHERE slug=$1")
        .bind(&f.board)
        .execute(&f.admin)
        .await
        .unwrap();
    assert!(
        matches!(
            f.insert(thread, &candidate).await,
            Err(StoreError::Conflict(_))
        ),
        "disabled media is not a count exemption"
    );
    sqlx::query("UPDATE content.boards SET image_limit=1 WHERE slug=$1")
        .bind(&f.board)
        .execute(&f.admin)
        .await
        .unwrap();
    sqlx::query("UPDATE content.threads SET closed=true WHERE id=$1")
        .bind(thread)
        .execute(&f.admin)
        .await
        .unwrap();
    assert!(matches!(
        f.insert(thread, &candidate).await,
        Err(StoreError::Conflict(_))
    ));
    sqlx::query("UPDATE content.threads SET closed=false WHERE id=$1")
        .bind(thread)
        .execute(&f.admin)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE media.jobs SET created_at=clock_timestamp()-interval '3 hours' WHERE id=$1",
    )
    .bind(&candidate.upload.id)
    .execute(&f.admin)
    .await
    .unwrap();
    assert!(
        matches!(
            f.insert(thread, &candidate).await,
            Err(StoreError::NotFound)
        ),
        "exemption does not renew capabilities"
    );
}

async fn posting_times(f: &Fixture) {
    use chrono::{DateTime, Timelike, Utc};
    let mut post = post();
    if f.attachment_only {
        post.comment.clear();
    }
    let requested = DateTime::from_timestamp(1_700_000_000, 987_654_321).unwrap();
    let op = f.reserve().await;
    f.approve(&op).await;
    let id = board_store::create_post_with_attachment_at(
        &f.public,
        &f.board,
        0,
        &post,
        Some(&op),
        requested,
    )
    .await
    .unwrap();
    let snapshot = board_store::thread_snapshot(&f.public, &f.board, id)
        .await
        .unwrap();
    assert_eq!(
        snapshot.posts[0].created_at,
        requested.with_nanosecond(0).unwrap()
    );
    assert_eq!(snapshot.thread.created_at, snapshot.posts[0].created_at);
    assert_eq!(snapshot.thread.modified_at, snapshot.posts[0].created_at);
    assert!(snapshot.thread.bumped_at > requested);
    let reply = f.reserve().await;
    f.approve(&reply).await;
    let earlier = requested - chrono::Duration::seconds(100);
    let rid = board_store::create_post_with_attachment_at(
        &f.public,
        &f.board,
        id,
        &post,
        Some(&reply),
        earlier,
    )
    .await
    .unwrap();
    let snapshot = board_store::thread_snapshot(&f.public, &f.board, id)
        .await
        .unwrap();
    assert_eq!(
        snapshot
            .posts
            .iter()
            .find(|p| p.id == rid)
            .unwrap()
            .created_at,
        earlier.with_nanosecond(0).unwrap()
    );
    assert_eq!(
        snapshot.thread.modified_at,
        earlier.with_nanosecond(0).unwrap()
    );
    assert!(snapshot.thread.http_modified_at > requested);

    let legacy = f.reserve().await;
    f.approve(&legacy).await;
    for invalid in [None, Some("infinity"), Some("-infinity")] {
        let mut tx = f.public.begin().await.unwrap();
        let new_id: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        let error = sqlx::query("SELECT content.insert_post_attachment($1,$2,$3,$4,$5,$6,$7,$8,$9,$10::text::timestamptz)")
        .bind(new_id)
        .bind(&f.board)
        .bind(id)
        .bind(&post.name)
        .bind(&post.subject)
        .bind(&post.comment)
        .bind(&legacy.upload.id)
        .bind(&legacy.upload.capability)
        .bind(false)
        .bind(invalid)
        .execute(&mut *tx)
        .await
        .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("22023")
        );
        tx.rollback().await.unwrap();
    }
    let before = Utc::now().timestamp();
    let mut tx = f.public.begin().await.unwrap();
    let new_id: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT content.insert_post_attachment($1,$2,$3,$4,$5,$6,$7,$8,$9)")
        .bind(new_id)
        .bind(&f.board)
        .bind(id)
        .bind(&post.name)
        .bind(&post.subject)
        .bind(&post.comment)
        .bind(&legacy.upload.id)
        .bind(&legacy.upload.capability)
        .bind(false)
        .execute(&mut *tx)
        .await
        .unwrap();
    let saved: DateTime<Utc> =
        sqlx::query_scalar("SELECT created_at FROM content.posts WHERE id=$1")
            .bind(new_id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(saved.nanosecond(), 0);
    assert!((before..=Utc::now().timestamp()).contains(&saved.timestamp()));
    tx.rollback().await.unwrap();
    sqlx::query(
        "UPDATE media.jobs SET created_at=clock_timestamp()-interval '3 hours' WHERE id=$1",
    )
    .bind(&legacy.upload.id)
    .execute(&f.admin)
    .await
    .unwrap();
    assert!(
        matches!(
            board_store::create_post_with_attachment_at(
                &f.public,
                &f.board,
                id,
                &post,
                Some(&legacy),
                Utc::now() - chrono::Duration::hours(4)
            )
            .await,
            Err(StoreError::NotFound)
        ),
        "an old request clock cannot extend capability lifetime"
    );
}

async fn reject_unattached_empty_posts(f: &Fixture, thread: i64) {
    let safe: bool = sqlx::query_scalar("SELECT NOT has_function_privilege('board_public','content.require_attachment_for_empty_post()','EXECUTE') AND NOT has_table_privilege('board_public','content.posts','TRIGGER') AND NOT has_column_privilege('board_public','content.posts','comment','UPDATE')")
        .fetch_one(&f.admin).await.unwrap();
    assert!(safe);
    let mut empty = post();
    empty.comment.clear();
    for parent in [0, thread] {
        assert!(matches!(
            create_post(&f.public, &f.board, parent, &empty).await,
            Err(StoreError::Invalid(_))
        ));
    }
    for immediate in [false, true] {
        let mut tx = f.public.begin().await.unwrap();
        let id: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES ($1,$2,$3,'Anonymous','','')")
            .bind(id).bind(&f.board).bind(thread).execute(&mut *tx).await.unwrap();
        let error = if immediate {
            let error = tx
                .execute("SET CONSTRAINTS ALL IMMEDIATE")
                .await
                .unwrap_err();
            tx.rollback().await.unwrap();
            error
        } else {
            tx.commit().await.unwrap_err()
        };
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
        let present: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM content.posts WHERE id=$1)")
                .bind(id)
                .fetch_one(&f.public)
                .await
                .unwrap();
        assert!(
            !present,
            "The failed constraint must roll back the empty row"
        );
    }
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
    retention(f).await;
}

async fn retention(f: &Fixture) {
    let a = f.reserve().await;
    let asset = f.approve(&a).await;
    sqlx::query(
        "UPDATE media.assets SET approved_at=clock_timestamp()-interval '2 days' WHERE id=$1",
    )
    .bind(&asset)
    .execute(&f.admin)
    .await
    .unwrap();
    assert!(
        !f.queue
            .output_retention_candidates()
            .await
            .unwrap()
            .contains(&asset),
        "A usable capability must survive even when the approval timestamp is old"
    );
    assert!(!f.queue.retire_output(&asset).await.unwrap());
    let id = f.insert(0, &a).await.unwrap();
    sqlx::query(
        "UPDATE media.jobs SET created_at=clock_timestamp()-interval '3 hours' WHERE id=$1",
    )
    .bind(&a.upload.id)
    .execute(&f.admin)
    .await
    .unwrap();
    assert!(
        !f.queue.retire_output(&asset).await.unwrap(),
        "Live attachments survive the orphan deadline"
    );
    delete_attachment(&f.public, &f.board, id).await.unwrap();
    assert!(
        f.queue
            .output_retention_candidates()
            .await
            .unwrap()
            .contains(&asset)
    );
    assert!(f.queue.retire_output(&asset).await.unwrap());
    assert!(!f.queue.retire_output(&asset).await.unwrap());
    assert!(f.reader.get(&asset).await.is_err());
    let coordinator = PgPool::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let revival = sqlx::query(
        "UPDATE media.assets SET state='approved',approved_at=clock_timestamp() WHERE id=$1",
    )
    .bind(&asset)
    .execute(&coordinator)
    .await
    .unwrap_err();
    assert_eq!(
        revival.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
    for statement in [
        "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ",
        "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE",
    ] {
        let mut tx = coordinator.begin().await.unwrap();
        tx.execute(statement).await.unwrap();
        let error = sqlx::query("SELECT media.retire_output($1)")
            .bind(&asset)
            .execute(&mut *tx)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("22023")
        );
        tx.rollback().await.unwrap();
    }
    for login in [
        "TEST_PUBLIC_DATABASE_URL",
        "MEDIA_READ_DATABASE_URL",
        "INTAKE_DATABASE_URL",
        "STAFF_DATABASE_URL",
        "AUTH_DATABASE_URL",
    ] {
        let connection = PgPool::connect(&std::env::var(login).unwrap())
            .await
            .unwrap();
        let error = sqlx::query("SELECT media.retire_output($1)")
            .bind(&asset)
            .execute(&connection)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501"),
            "{login}"
        );
        connection.close().await;
    }

    // Use the actual public function and an uncommitted posting transaction.
    // Retirement waits for its job lock, then must see the committed attachment.
    let b = f.reserve().await;
    let live = f.approve(&b).await;
    let thread = create_post(&f.public, &f.board, 0, &post()).await.unwrap();
    let mut posting = f.public.begin().await.unwrap();
    let number: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&mut *posting)
        .await
        .unwrap();
    sqlx::query("SELECT content.insert_post_attachment($1,$2,$3,'Anonymous','','Retention race',$4,$5,false)")
        .bind(number).bind(&f.board).bind(thread).bind(&b.upload.id).bind(&b.upload.capability).execute(&mut *posting).await.unwrap();
    let queue = f.queue.clone();
    let waiting_asset = live.clone();
    let waiting = tokio::spawn(async move { queue.retire_output(&waiting_asset).await });
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    assert!(
        !waiting.is_finished(),
        "Retirement waits for the posting job lock"
    );
    posting.commit().await.unwrap();
    assert!(!waiting.await.unwrap().unwrap());
    f.reader.get(&live).await.unwrap();

    // Archive retention remains readable until its deadline, then eligible.
    sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp()-interval '1 second',archive_expires_at=clock_timestamp()+interval '60 seconds' WHERE id=$1")
        .bind(thread).execute(&f.admin).await.unwrap();
    assert!(!f.queue.retire_output(&live).await.unwrap());
    sqlx::query("UPDATE content.threads SET archive_expires_at=clock_timestamp()-interval '1 millisecond' WHERE id=$1")
        .bind(thread).execute(&f.admin).await.unwrap();
    assert!(f.queue.retire_output(&live).await.unwrap());
    coordinator.close().await;
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
