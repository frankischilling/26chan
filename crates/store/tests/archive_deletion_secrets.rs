#![cfg(feature = "database-tests")]

use board_domain::{capcode::Capcode, poster_id::PosterIdKey};
use board_store::{
    NewPost, PostIdentityKeys, PostMetadata, PostingContext, StaffPostAuthority, StaffPostIdentity,
    StoreError,
};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{net::IpAddr, time::Duration};

// Prospective retirement only. Historical hashes are covered by the isolated
// pre-0091 upgrade fixture; these tests never disable triggers or alter /j/.
static TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone)]
struct Fixture {
    owner: PgPool,
    staff: PgPool,
    auth: PgPool,
    public: PgPool,
    board: String,
    account: i64,
    session: Vec<u8>,
    csrf: Vec<u8>,
    key: String,
}

async fn pool(variable: &str) -> PgPool {
    PgPool::connect(&std::env::var(variable).expect("explicit owned database URL required"))
        .await
        .unwrap()
}

impl Fixture {
    async fn new(private: bool) -> Self {
        let owner = pool("MIGRATION_DATABASE_URL").await;
        let seed: String = sqlx::query_scalar("SELECT replace(gen_random_uuid()::text,'-','')")
            .fetch_one(&owner)
            .await
            .unwrap();
        let board = format!("v{}", &seed[..9]);
        sqlx::query("INSERT INTO content.boards(slug,title,description,staff_only,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) VALUES($1,'Owned archive secrets','Synthetic fixture',$2,2000,1000,1000,1000,10,0,0,0)")
            .bind(&board).bind(private).execute(&owner).await.unwrap();
        // Source badge/name authority is real and remains valid when this
        // synthetic non-j board changes privacy while a writer is queued.
        let account = sqlx::query_scalar("INSERT INTO staff_identity.accounts(role,flags,allow_boards) VALUES('moderator',ARRAY['capcode','capcodename','developer'],ARRAY['all']) RETURNING id")
            .fetch_one(&owner).await.unwrap();
        let credential = seed.as_bytes().to_vec();
        sqlx::query(
            "INSERT INTO staff_identity.credentials(id,account_id,credential) VALUES($1,$2,'{}')",
        )
        .bind(&credential)
        .bind(account)
        .execute(&owner)
        .await
        .unwrap();
        let (session, csrf, key): (Vec<u8>, Vec<u8>, String) = sqlx::query_as("SELECT sha256(convert_to(gen_random_uuid()::text,'UTF8')),sha256(convert_to(gen_random_uuid()::text,'UTF8')),encode(sha256(convert_to(gen_random_uuid()::text,'UTF8')),'hex')")
            .fetch_one(&owner).await.unwrap();
        sqlx::query("INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id) VALUES($1,$2,$3,$4)")
            .bind(&session).bind(&csrf).bind(account).bind(credential).execute(&owner).await.unwrap();
        let staff = pool("STAFF_DATABASE_URL").await;
        let auth = pool("AUTH_DATABASE_URL").await;
        let public = pool("TEST_PUBLIC_DATABASE_URL").await;
        for (connection, expected) in [
            (&staff, "board_staff"),
            (&auth, "board_auth"),
            (&public, "board_public"),
        ] {
            let role: String = sqlx::query_scalar("SELECT current_user::text")
                .fetch_one(connection)
                .await
                .unwrap();
            assert_eq!(role, expected);
        }
        Self {
            owner,
            staff,
            auth,
            public,
            board,
            account,
            session,
            csrf,
            key,
        }
    }

    async fn write(&self, parent: i64, actor: u8, valid: bool) -> Result<i64, StoreError> {
        self.staff_write(parent, actor, valid, false).await
    }

    async fn staff_write(
        &self,
        parent: i64,
        actor: u8,
        valid: bool,
        ordinary: bool,
    ) -> Result<i64, StoreError> {
        let key = PosterIdKey::parse(&self.key).unwrap();
        let ticket: Vec<u8> =
            sqlx::query_scalar("SELECT sha256(convert_to(gen_random_uuid()::text,'UTF8'))")
                .fetch_one(&self.owner)
                .await
                .unwrap();
        let ticket: [u8; 32] = ticket.try_into().unwrap();
        let invalid = [0_u8; 32];
        let authority = StaffPostAuthority {
            auth_pool: &self.auth,
            session_hash: &self.session,
            csrf_hash: if valid { &self.csrf } else { &invalid },
            ticket_hash: &ticket,
            idle_seconds: 900,
            highlight: false,
            authorized_limits: true,
            raw_name_nonempty: true,
            identity: Some(StaffPostIdentity {
                capcode: if ordinary {
                    None
                } else {
                    Some(Capcode::Moderator)
                },
                name_allowed: true,
                administrator: false,
                tripcode_key: None,
            }),
        };
        let keys = PostIdentityKeys {
            tripcode: None,
            poster_id: Some(&key),
        };
        if ordinary {
            board_store::create_ordinary_staff_post(
                &self.staff,
                &self.board,
                parent,
                &post(),
                context(actor),
                PostMetadata {
                    keys,
                    country_database: None,
                    flag: "",
                    options: "",
                    spoiler: false,
                },
                authority,
            )
            .await
        } else {
            board_store::create_staff_post_with_context_and_keys(
                &self.staff,
                &self.board,
                parent,
                &post(),
                context(actor),
                keys,
                authority,
            )
            .await
        }
    }

    async fn rollover_write(&self, staff: bool, private: bool, parent: i64, actor: u8) -> i64 {
        if staff {
            self.staff_write(parent, actor, true, !private)
                .await
                .unwrap()
        } else {
            self.public_write(parent, actor).await
        }
    }

    async fn policy(&self, retention: i32) {
        sqlx::query("UPDATE content.boards SET thread_limit=1,archive_retention_seconds=$2,archive_limit=100 WHERE slug=$1")
            .bind(&self.board).bind(retention).execute(&self.owner).await.unwrap();
    }

    async fn cleanup(self) {
        for query in [
            "DELETE FROM content.reports WHERE board=$1",
            "DELETE FROM content.moderation_audit WHERE board=$1",
            "DELETE FROM post_secrets.staff_post_intents WHERE board=$1",
            "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
            "DELETE FROM content.post_media WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
            "DELETE FROM content.posts WHERE board=$1",
            "DELETE FROM content.threads WHERE board=$1",
            "DELETE FROM post_secrets.posting_thread_actions WHERE board=$1",
            "DELETE FROM content.boards WHERE slug=$1",
        ] {
            sqlx::query(query)
                .bind(&self.board)
                .execute(&self.owner)
                .await
                .unwrap();
        }
        for query in [
            "DELETE FROM staff_identity.sessions WHERE account_id=$1",
            "DELETE FROM staff_identity.credentials WHERE account_id=$1",
            "DELETE FROM staff_identity.accounts WHERE id=$1",
        ] {
            sqlx::query(query)
                .bind(self.account)
                .execute(&self.owner)
                .await
                .unwrap();
        }
    }
}

fn post() -> NewPost {
    NewPost {
        name: "Anonymous".into(),
        subject: "Owned private rollover".into(),
        comment: "Synthetic private active and archive fixture".into(),
        deletion_hash: "synthetic-not-a-password".into(),
        sage: false,
    }
}

fn context(actor: u8) -> PostingContext {
    PostingContext {
        request_start: chrono::Utc::now(),
        peer: Some(IpAddr::from([192, 0, 2, actor])),
        op_password_proof: None,
    }
}

fn code(error: &sqlx::Error) -> String {
    error
        .as_database_error()
        .unwrap()
        .code()
        .unwrap()
        .into_owned()
}
fn denial(error: sqlx::Error) -> (String, String) {
    (
        code(&error),
        error.as_database_error().unwrap().message().to_owned(),
    )
}
impl Fixture {
    async fn public_write(&self, parent: i64, actor: u8) -> i64 {
        let key = PosterIdKey::parse(&self.key).unwrap();
        board_store::create_post_with_identity_keys(
            &self.public,
            &self.board,
            parent,
            &post(),
            None,
            context(actor),
            PostIdentityKeys {
                tripcode: None,
                poster_id: Some(&key),
            },
        )
        .await
        .unwrap()
    }
    async fn hashes(&self, ids: &[i64]) -> Vec<(i64, String)> {
        sqlx::query_as("SELECT post_id,password_hash FROM post_secrets.deletion WHERE post_id=ANY($1) ORDER BY post_id")
            .bind(ids).fetch_all(&self.owner).await.unwrap()
    }
    async fn archive(&self, id: i64) {
        sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE board=$1 AND id=$2")
            .bind(&self.board).bind(id).execute(&self.owner).await.unwrap();
    }
    async fn retained(&self, ids: &[i64]) -> serde_json::Value {
        sqlx::query_scalar("SELECT jsonb_build_object('posts',(SELECT jsonb_agg(to_jsonb(p) ORDER BY id) FROM content.posts p WHERE id=ANY($1)),'reports',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM content.reports r WHERE post_id=ANY($1)),'audit',(SELECT jsonb_agg(to_jsonb(a) ORDER BY id) FROM content.moderation_audit a WHERE target_id=ANY($1)),'media',(SELECT jsonb_agg(to_jsonb(m) ORDER BY post_id) FROM content.post_media m WHERE post_id=ANY($1)),'anonymous',(SELECT jsonb_agg(to_jsonb(a) ORDER BY post_id) FROM post_secrets.anonymous_posts a WHERE post_id=ANY($1)),'history',(SELECT jsonb_agg(to_jsonb(h) ORDER BY post_id) FROM post_secrets.posting_history h WHERE post_id=ANY($1)))")
            .bind(ids).fetch_one(&self.owner).await.unwrap()
    }
}

// 0087 already removes per-post cooldown history on archive. Assert that
// independently, then compare every other snapshot field without omissions.
// Rollback tests intentionally compare the complete unmodified snapshot.
fn assert_archive_retention(mut before: serde_json::Value, after: serde_json::Value) {
    assert!(
        after["history"].is_null(),
        "Existing archive history cleanup remains effective"
    );
    before["history"] = serde_json::Value::Null;
    assert_eq!(
        after, before,
        "Archive preserves content, reports, audit, media and anonymous derivatives"
    );
}

#[tokio::test]
async fn public_and_staff_rollover_retire_only_victims_and_preserve_private_exemption() {
    let _serial = TEST.lock().await;
    for (staff, private) in [(false, false), (true, false), (true, true)] {
        let f = Fixture::new(private).await;
        let work = f.clone();
        let result = tokio::spawn(async move {
            let f = work;
            let mut protected = Vec::new();
            for (n, sticky, undead) in [(1, true, false), (2, false, true), (3, true, true)] {
                let op = f.rollover_write(staff, private, 0, n).await;
                let reply = f.rollover_write(staff, private, op, n+20).await;
                sqlx::query("UPDATE content.threads SET sticky=$2,undead=$3 WHERE id=$1")
                    .bind(op).bind(sticky).bind(undead).execute(&f.owner).await.unwrap();
                protected.extend([op,reply]);
            }
            let victim = f.rollover_write(staff, private, 0, 40).await;
            let reply = f.rollover_write(staff, private, victim, 41).await;
            let reader = if private { &f.staff } else { &f.public };
            if private {
                let mut private_posts = protected.clone();
                private_posts.extend([victim, reply]);
                assert!(f.hashes(&private_posts).await.is_empty(), "Real badged private posts do not create deletion passwords");
                // Model imported private deletion authority on active content.
                // This legitimate migrator insertion leaves every trigger on;
                // these hashes are fixture data, not normal private-post output.
                for id in private_posts {
                    sqlx::query("INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES($1,'owned-imported-private-hash')")
                        .bind(id).execute(&f.owner).await.unwrap();
                }
            }
            if private {
                sqlx::query("INSERT INTO content.reports(board,post_id,reason) VALUES($1,$2,'Owned private retained report')")
                    .bind(&f.board).bind(reply).execute(&f.owner).await.unwrap();
            } else {
                let report_identity = PosterIdKey::parse(&f.key).unwrap()
                    .public_report_rate_identity(IpAddr::from([192, 0, 2, 201]));
                board_store::report(reader,&f.board,reply,"Owned retained report",&report_identity).await.unwrap();
            }
            sqlx::query("INSERT INTO content.moderation_audit(account_id,board,target_id,action) VALUES($1,$2,$3,'close')")
                .bind(f.account).bind(&f.board).bind(victim).execute(&f.owner).await.unwrap();
            let before = f.retained(&[victim,reply]).await;
            let protected_hashes = f.hashes(&protected).await;
            assert_eq!(protected_hashes.len(),6);
            assert_eq!(f.hashes(&[victim,reply]).await.len(),2);
            f.policy(3600).await;
            let newest = f.rollover_write(staff, private, 0, 42).await;
            assert_eq!(f.hashes(&protected).await,protected_hashes);
            assert_eq!(f.hashes(&[newest]).await.len(),if private {0} else {1}, "Ordinary public/staff writers create hashes; badged private writer does not");
            assert_eq!(f.hashes(&[victim,reply]).await.len(),if private {2} else {0});
            let after = f.retained(&[victim,reply]).await;
            if private {
                assert_eq!(after, before, "Private exemption retains posting history too");
            } else {
                assert_archive_retention(before, after);
            }
            let archived: bool = sqlx::query_scalar("SELECT archived_at IS NOT NULL FROM content.threads WHERE id=$1")
                .bind(victim).fetch_one(&f.owner).await.unwrap();
            assert_eq!(archived,!private);
            if !private {
                assert_eq!(board_store::posts(reader,&f.board,victim).await.unwrap().len(),2);
                assert!(board_store::archive_snapshot(reader,&f.board).await.unwrap().entries.iter().any(|t|t.id==victim));
            }
        }).await;
        f.cleanup().await;
        result.unwrap();
    }
}

#[tokio::test]
async fn rollback_repeated_updates_guard_errors_and_least_privilege() {
    let _serial = TEST.lock().await;
    let f = Fixture::new(false).await;
    let private = Fixture::new(true).await;
    let work = f.clone();
    let hidden = private.clone();
    let result = tokio::spawn(async move {
        let f=work;
        let op=f.public_write(0,50).await;
        let reply=f.public_write(op,51).await;
        let active=f.public_write(0,52).await;
        let absent=f.public_write(0,53).await;
        let hidden_op=hidden.write(0,54,true).await.unwrap();
        let before=f.hashes(&[op,reply,active]).await;
        let retained_before=f.retained(&[op,reply,active]).await;
        assert_eq!(retained_before["history"].as_array().unwrap().len(),3);
        let mut tx=f.owner.begin().await.unwrap();
        sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
            .bind(op).execute(&mut *tx).await.unwrap();
        let retired:i64=sqlx::query_scalar("SELECT count(*) FROM post_secrets.deletion WHERE post_id=ANY($1)")
            .bind(&[op,reply][..]).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(retired,0);
        let history_retired:i64=sqlx::query_scalar("SELECT count(*) FROM post_secrets.posting_history WHERE post_id=ANY($1)")
            .bind(&[op,reply][..]).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(history_retired,0,"Existing 0087 archive cleanup runs in the same transaction");
        assert_eq!(code(&sqlx::query("SELECT 1/0").execute(&mut *tx).await.unwrap_err()),"22012");
        tx.rollback().await.unwrap();
        assert_eq!(f.hashes(&[op,reply,active]).await,before);
        assert_eq!(f.retained(&[op,reply,active]).await,retained_before,"Rollback restores existing posting history together with deletion hashes");
        f.archive(op).await;
        f.archive(op).await;
        assert!(f.hashes(&[op,reply]).await.is_empty());
        assert_eq!(f.hashes(&[active]).await.len(),1);
        for pool in [&f.public,&f.staff] {
            for query in ["DELETE FROM post_secrets.deletion WHERE post_id=$1", "UPDATE post_secrets.deletion SET password_hash='revival' WHERE post_id=$1"] {
                assert_eq!(code(&sqlx::query(query).bind(op).execute(pool).await.unwrap_err()),"42501");
            }
        }
        let insert="INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES($1,'revival')";
        assert_eq!(code(&sqlx::query(insert).bind(op).execute(&f.staff).await.unwrap_err()),"42501");
        let archived=denial(sqlx::query(insert).bind(op).execute(&f.public).await.unwrap_err());
        let private=denial(sqlx::query(insert).bind(hidden_op).execute(&f.public).await.unwrap_err());
        let missing=denial(sqlx::query(insert).bind(-1_i64).execute(&f.public).await.unwrap_err());
        assert_eq!(archived,("23514".into(),"Deletion authority is unavailable.".into()));
        assert_eq!(private,missing,"Private parent and absent parent must be indistinguishable");
        assert_eq!(private,archived);
        assert_eq!(code(&sqlx::query("UPDATE post_secrets.deletion SET post_id=$1 WHERE post_id=$2")
            .bind(op).bind(active).execute(&f.owner).await.unwrap_err()),"23514");
        // Active rotation and reassignment stay valid for the existing operator.
        sqlx::query("UPDATE post_secrets.deletion SET password_hash='active-rotated' WHERE post_id=$1")
            .bind(active).execute(&f.owner).await.unwrap();
        sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id=$1").bind(absent).execute(&f.owner).await.unwrap();
        sqlx::query(insert).bind(absent).execute(&f.public).await.unwrap();
        sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id=$1").bind(absent).execute(&f.owner).await.unwrap();
        sqlx::query("UPDATE post_secrets.deletion SET post_id=$1 WHERE post_id=$2")
            .bind(absent).bind(active).execute(&f.owner).await.unwrap();
        assert_eq!(f.hashes(&[absent]).await,vec![(absent,"active-rotated".into())]);
        let allowed:bool=sqlx::query_scalar("SELECT has_column_privilege('board_posting_cooldown_owner','post_secrets.deletion','post_id','SELECT') AND NOT has_column_privilege('board_posting_cooldown_owner','post_secrets.deletion','password_hash','SELECT')")
            .fetch_one(&f.owner).await.unwrap();
        assert!(allowed);
        let mut owner=f.owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_posting_cooldown_owner").execute(&mut *owner).await.unwrap();
        assert_eq!(code(&sqlx::query("SELECT password_hash FROM post_secrets.deletion LIMIT 0").execute(&mut *owner).await.unwrap_err()),"42501");
        owner.rollback().await.unwrap();
    }).await;
    f.cleanup().await;
    private.cleanup().await;
    result.unwrap();
}

async fn wait_behind(owner: &PgPool, writer: i32, blocker: i32) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT $2=ANY(pg_blocking_pids($1))")
                .bind(writer)
                .bind(blocker)
                .fetch_one(owner)
                .await
                .unwrap();
            if waiting {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("Expected concrete blocker must be observed");
}

#[tokio::test]
async fn insertion_waits_for_archive_and_stale_repeatable_read_fails_closed() {
    let _serial = TEST.lock().await;
    let f = Fixture::new(false).await;
    let work = f.clone();
    let result = tokio::spawn(async move {
        let f = work;
        for repeatable in [false, true] {
            let op = f.public_write(0, 60 + u8::from(repeatable)).await;
            sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id=$1")
                .bind(op)
                .execute(&f.owner)
                .await
                .unwrap();
            let one = PgPoolOptions::new()
                .max_connections(1)
                .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
                .await
                .unwrap();
            let mut writer = one.begin().await.unwrap();
            if repeatable {
                sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
                    .execute(&mut *writer)
                    .await
                    .unwrap();
            }
            let writer_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut *writer)
                .await
                .unwrap();
            let snapshot_active: bool =
                sqlx::query_scalar("SELECT archived_at IS NULL FROM content.threads WHERE id=$1")
                    .bind(op)
                    .fetch_one(&mut *writer)
                    .await
                    .unwrap();
            assert!(snapshot_active);
            let mut archive = f.owner.begin().await.unwrap();
            let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut *archive)
                .await
                .unwrap();
            sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
                .bind(&f.board)
                .execute(&mut *archive)
                .await
                .unwrap();
            sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
                .bind(op)
                .execute(&mut *archive)
                .await
                .unwrap();
            let queued = tokio::spawn(async move {
                let error = sqlx::query(
                    "INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES($1,'queued')",
                )
                .bind(op)
                .execute(&mut *writer)
                .await
                .unwrap_err();
                writer.rollback().await.unwrap();
                error
            });
            wait_behind(&f.owner, writer_pid, blocker).await;
            archive.commit().await.unwrap();
            let error = tokio::time::timeout(Duration::from_secs(5), queued)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(code(&error), if repeatable { "40001" } else { "23514" });
            assert!(f.hashes(&[op]).await.is_empty());
            one.close().await;
        }
    })
    .await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn archive_transitions_reject_fixed_snapshots_and_out_of_order_locks_fail_fast() {
    let _serial = TEST.lock().await;
    let f = Fixture::new(false).await;
    let work = f.clone();
    let result = tokio::spawn(async move {
        let f = work;
        let op = f.public_write(0, 70).await;
        let reply = f.public_write(op, 71).await;
        let before = f.hashes(&[op, reply]).await;
        // Fixed snapshots could miss newly inserted secrets. The migration
        // deliberately supports archive transitions at Read Committed only.
        for statement in [
            "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ",
            "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE",
        ] {
            let mut tx = f.owner.begin().await.unwrap();
            sqlx::query(statement).execute(&mut *tx).await.unwrap();
            let error =
                sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
                    .bind(op)
                    .execute(&mut *tx)
                    .await
                    .unwrap_err();
            assert_eq!(code(&error), "22023");
            tx.rollback().await.unwrap();
            assert_eq!(f.hashes(&[op, reply]).await, before);
        }
        let mut lock = f.owner.begin().await.unwrap();
        sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
            .bind(&f.board)
            .execute(&mut *lock)
            .await
            .unwrap();
        // Both paths already own a downstream tuple when their trigger tries
        // the board. NOWAIT must reject immediately, never wait into a cycle.
        for query in [
            "UPDATE post_secrets.deletion SET password_hash='out-of-order' WHERE post_id=$1",
            "UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1",
        ] {
            let result = tokio::time::timeout(
                Duration::from_secs(3),
                sqlx::query(query).bind(op).execute(&f.owner),
            )
            .await
            .expect("NOWAIT must not hang");
            assert_eq!(code(&result.unwrap_err()), "55P03");
        }
        lock.rollback().await.unwrap();
        assert_eq!(f.hashes(&[op, reply]).await, before);
        f.archive(op).await;
        assert!(f.hashes(&[op, reply]).await.is_empty());
    })
    .await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn update_already_holding_secret_cannot_deadlock_board_first_archiver() {
    let _serial = TEST.lock().await;
    let f = Fixture::new(false).await;
    let work = f.clone();
    let result = tokio::spawn(async move {
        let f = work;
        let op = f.public_write(0, 80).await;
        let mut update = f.owner.begin().await.unwrap();
        let updater: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *update)
            .await
            .unwrap();
        sqlx::query("SELECT post_id FROM post_secrets.deletion WHERE post_id=$1 FOR UPDATE")
            .bind(op)
            .execute(&mut *update)
            .await
            .unwrap();
        let mut archive = f.owner.begin().await.unwrap();
        let archiver: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *archive)
            .await
            .unwrap();
        sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
            .bind(&f.board)
            .execute(&mut *archive)
            .await
            .unwrap();
        let queued = tokio::spawn(async move {
            sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
                .bind(op)
                .execute(&mut *archive)
                .await
                .unwrap();
            archive.commit().await.unwrap();
        });
        wait_behind(&f.owner, archiver, updater).await;
        let error = tokio::time::timeout(
            Duration::from_secs(3),
            sqlx::query(
                "UPDATE post_secrets.deletion SET password_hash='out-of-order' WHERE post_id=$1",
            )
            .bind(op)
            .execute(&mut *update),
        )
        .await
        .expect("Guard must reject instead of joining a lock cycle")
        .unwrap_err();
        assert_eq!(code(&error), "55P03");
        update.rollback().await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), queued)
            .await
            .unwrap()
            .unwrap();
        assert!(f.hashes(&[op]).await.is_empty());
    })
    .await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn delete_already_holding_secret_cannot_deadlock_board_first_archiver() {
    let _serial = TEST.lock().await;
    let f = Fixture::new(false).await;
    let work = f.clone();
    let result = tokio::spawn(async move {
        let f = work;
        let op = f.public_write(0, 81).await;
        let before = f.hashes(&[op]).await;
        assert_eq!(before.len(), 1);
        let mut update = f.owner.begin().await.unwrap();
        let updater: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *update)
            .await
            .unwrap();
        sqlx::query("SELECT post_id FROM post_secrets.deletion WHERE post_id=$1 FOR UPDATE")
            .bind(op)
            .execute(&mut *update)
            .await
            .unwrap();
        let mut archive = f.owner.begin().await.unwrap();
        let archiver: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *archive)
            .await
            .unwrap();
        sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
            .bind(&f.board)
            .execute(&mut *archive)
            .await
            .unwrap();
        let queued = tokio::spawn(async move {
            sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
                .bind(op)
                .execute(&mut *archive)
                .await
                .unwrap();
            archive.commit().await.unwrap();
        });
        wait_behind(&f.owner, archiver, updater).await;
        let error = tokio::time::timeout(
            Duration::from_secs(3),
            sqlx::query(
                "DELETE FROM post_secrets.deletion WHERE post_id=$1",
            )
            .bind(op)
            .execute(&mut *update),
        )
        .await
        .expect("Guard must reject instead of joining a lock cycle")
        .unwrap_err();
        assert_eq!(code(&error), "55P03");
        assert_eq!(f.hashes(&[op]).await, before,
            "Rejected deletion leaves authority intact until the archiver can commit");
        update.rollback().await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), queued)
            .await
            .unwrap()
            .unwrap();
        assert!(f.hashes(&[op]).await.is_empty());
    })
    .await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn retirement_preserves_anonymous_derivatives_actions_and_consumed_media() {
    use board_domain::anonymous_session::Capability;
    use board_store::{AnonymousPostingContext, PostMetadata, anonymous_session::PostingSession};
    let _serial = TEST.lock().await;
    let f = Fixture::new(false).await;
    sqlx::query("UPDATE content.boards SET image_limit=100 WHERE slug=$1")
        .bind(&f.board)
        .execute(&f.owner)
        .await
        .unwrap();
    let capability = Capability::generate().unwrap();
    let token = capability.storage_hash();
    let intake = board_store::media_intake::IntakeStore::connect(
        &std::env::var("INTAKE_DATABASE_URL").unwrap(),
    )
    .await
    .unwrap();
    let queue =
        board_store::media::MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
            .await
            .unwrap();
    let upload = intake.reserve("archive-secret-fixture.png").await.unwrap();
    let job = upload.id.clone();
    let work = f.clone();
    let result=tokio::spawn(async move {
        let f=work;
        intake.begin_upload(&upload.id,&upload.capability).await.unwrap();
        intake.finish_upload(&upload.id,&upload.capability,100).await.unwrap();
        let claim=queue.claim().await.unwrap().expect("Owned fixture job must be claimable");
        assert_eq!(claim.id,upload.id,"Requires an idle owned qualification queue");
        let lease=claim.lease_token.unwrap();
        let output=queue.prepare_output(&claim.id,&lease,&board_store::media_assets::OutputMetadata {
            sha256:"b".repeat(64),bytes:123,width:10,height:20,
        }).await.unwrap();
        queue.approve_output(&claim.id,&lease,&output.id).await.unwrap();
        let attachment=board_store::post_media::NewAttachment { upload,spoiler:false };
        let key=PosterIdKey::parse(&f.key).unwrap();
        let mut ids=Vec::new();
        for (parent,minted,media) in [(0,true,Some(&attachment)),(0,false,None)] {
            let parent=if minted {parent} else {ids[0]};
            let id=board_store::create_post_with_anonymous_session(&f.public,&f.board,parent,&post(),media,
                AnonymousPostingContext {
                    posting:context(90),
                    session:PostingSession {
                        fingerprints:capability.fingerprints(Some(IpAddr::from([192,0,2,90])),*b"US"),
                        minted,now:chrono::Utc::now(),
                    },
                },PostMetadata {
                    keys:PostIdentityKeys {tripcode:None,poster_id:Some(&key)},spoiler:false,
                    country_database:None,flag:"",options:"",
                }).await.unwrap();
            ids.push(id);
        }
        let report_identity = key.public_report_rate_identity(IpAddr::from([192, 0, 2, 90]));
        board_store::report(&f.public,&f.board,ids[1],"Retained archive report",&report_identity).await.unwrap();
        let before=f.retained(&ids).await;
        assert_eq!(before["anonymous"].as_array().unwrap().len(),2);
        assert_eq!(before["media"].as_array().unwrap().len(),1);
        assert_eq!(before["history"].as_array().unwrap().len(),2);
        let actions:serde_json::Value=sqlx::query_scalar("SELECT jsonb_agg(to_jsonb(a) ORDER BY actor_hash) FROM post_secrets.posting_thread_actions a WHERE board=$1")
            .bind(&f.board).fetch_one(&f.owner).await.unwrap();
        let saved_session:serde_json::Value=sqlx::query_scalar("SELECT to_jsonb(s) FROM post_secrets.anonymous_sessions s WHERE token_hash=$1")
            .bind(token.as_slice()).fetch_one(&f.owner).await.unwrap();
        // A real public OP creates a new actor's action only; the victim
        // actor's action survives. Existing 0087 separately removes archived
        // posting_history; 0091 must preserve the other retained records.
        f.policy(3600).await;
        f.public_write(0,91).await;
        assert!(f.hashes(&ids).await.is_empty());
        assert_archive_retention(before, f.retained(&ids).await);
        let actor=key.public_posting_rate_identity(IpAddr::from([192,0,2,90]));
        let after:serde_json::Value=sqlx::query_scalar("SELECT jsonb_agg(to_jsonb(a) ORDER BY actor_hash) FROM post_secrets.posting_thread_actions a WHERE board=$1 AND actor_hash=$2")
            .bind(&f.board).bind(actor.as_bytes().as_slice()).fetch_one(&f.owner).await.unwrap();
        assert_eq!(after,actions);
        let after_session:serde_json::Value=sqlx::query_scalar("SELECT to_jsonb(s) FROM post_secrets.anonymous_sessions s WHERE token_hash=$1")
            .bind(token.as_slice()).fetch_one(&f.owner).await.unwrap();
        assert_eq!(after_session,saved_session);
        assert!(board_store::post_media::attachment(&f.public,ids[0]).await.unwrap().is_some());
        let consumed_before=board_store::post_media::check_upload(&f.public,&attachment.upload.id,&attachment.upload.capability).await;
        assert!(consumed_before.is_err(),"Archived attachment must remain consumed");
        let proof:Option<Vec<u8>>=sqlx::query_scalar("SELECT password_proof FROM post_secrets.anonymous_posts WHERE post_id=$1")
            .bind(ids[0]).fetch_optional(&f.owner).await.unwrap();
        assert_eq!(proof.unwrap().len(),32,"Proof derivative is deliberately outside retirement scope");
    }).await;
    let owner = f.owner.clone();
    f.cleanup().await;
    sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
        .bind(token.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    for query in [
        "DELETE FROM media.assets WHERE job_id=$1",
        "DELETE FROM media.jobs WHERE id=$1",
    ] {
        sqlx::query(query).bind(&job).execute(&owner).await.unwrap();
    }
    result.unwrap();
}

#[tokio::test]
async fn queued_public_secret_insert_rechecks_board_privacy_after_waiting() {
    let _serial = TEST.lock().await;
    for repeatable in [false, true] {
        let f = Fixture::new(false).await;
        let work = f.clone();
        let result = tokio::spawn(async move {
            let f = work;
            let op = f.public_write(0, 100).await;
            sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id=$1")
                .bind(op).execute(&f.owner).await.unwrap();
            let one = PgPoolOptions::new().max_connections(1)
                .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap()).await.unwrap();
            let mut writer = one.begin().await.unwrap();
            if repeatable {
                sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
                    .execute(&mut *writer).await.unwrap();
            }
            let writer_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut *writer).await.unwrap();
            let initially_public: bool = sqlx::query_scalar("SELECT NOT staff_only FROM content.boards WHERE slug=$1")
                .bind(&f.board).fetch_one(&mut *writer).await.unwrap();
            assert!(initially_public);
            let mut policy = f.owner.begin().await.unwrap();
            let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut *policy).await.unwrap();
            // The writer initially sees the committed public parent, then
            // waits for this real privacy change on the shared board lock.
            sqlx::query("UPDATE content.boards SET staff_only=true WHERE slug=$1")
                .bind(&f.board).execute(&mut *policy).await.unwrap();
            let queued = tokio::spawn(async move {
                let error = sqlx::query("INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES($1,'queued-private')")
                    .bind(op).execute(&mut *writer).await.unwrap_err();
                writer.rollback().await.unwrap();
                error
            });
            wait_behind(&f.owner, writer_pid, blocker).await;
            policy.commit().await.unwrap();
            let error = tokio::time::timeout(Duration::from_secs(5), queued)
                .await.unwrap().unwrap();
            if repeatable {
                assert_eq!(code(&error), "40001");
            } else {
                assert_eq!(denial(error), ("23514".into(), "Deletion authority is unavailable.".into()));
            }
            assert!(f.hashes(&[op]).await.is_empty());
            let private: bool = sqlx::query_scalar("SELECT staff_only FROM content.boards WHERE slug=$1")
                .bind(&f.board).fetch_one(&f.owner).await.unwrap();
            assert!(private);
            one.close().await;
        }).await;
        f.cleanup().await;
        result.unwrap();
    }
}
