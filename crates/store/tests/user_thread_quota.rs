#![cfg(feature = "database-tests")]
mod support;

use board_domain::{capcode::Capcode, poster_id::PosterIdKey};
use board_store::{
    NewPost, PostIdentityKeys, PostMetadata, PostingContext, StaffPostAuthority, StaffPostIdentity,
    StoreError,
};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{
    net::IpAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

// Source validate_user_thread_limit, IP-only slice. No password or Pass claims.
// Every admitted post uses a real public or authenticated staff writer. Only
// owned fixture board policy and lifecycle metadata are changed. Counted
// history is never cleared to make a later assertion pass.
static TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone)]
struct Fixture {
    owner: PgPool,
    staff: PgPool,
    auth: PgPool,
    public: PgPool,
    boards: [String; 2],
    account: i64,
    session: Vec<u8>,
    csrf: Vec<u8>,
    key: String,
    jobs: Arc<Mutex<Vec<String>>>,
}

async fn pool(name: &str) -> PgPool {
    PgPool::connect(&std::env::var(name).expect("explicit owned database URL required"))
        .await
        .unwrap()
}

impl Fixture {
    async fn new() -> Self {
        let owner = pool("MIGRATION_DATABASE_URL").await;
        let seed: String = sqlx::query_scalar("SELECT replace(gen_random_uuid()::text,'-','')")
            .fetch_one(&owner)
            .await
            .unwrap();
        let boards = [format!("s{}", &seed[..9]), format!("t{}", &seed[..9])];
        for board in &boards {
            sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) VALUES($1,'Owned user thread quota','Synthetic fixture',2000,1000,1000,1000,10,0,0,0)")
                .bind(board).execute(&owner).await.unwrap();
        }
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
            boards,
            account,
            session,
            csrf,
            key,
            jobs: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn actor(&self, peer: IpAddr) -> [u8; 32] {
        *PosterIdKey::parse(&self.key)
            .unwrap()
            .public_posting_rate_identity(peer)
            .as_bytes()
    }

    async fn now(&self) -> i64 {
        sqlx::query_scalar("SELECT floor(extract(epoch FROM clock_timestamp()))::bigint")
            .fetch_one(&self.owner)
            .await
            .unwrap()
    }

    async fn write(
        &self,
        board: usize,
        parent: i64,
        peer: IpAddr,
        epoch: i64,
        ordinary: bool,
        valid: bool,
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
        let post = post();
        let context = context(peer, epoch);
        let keys = PostIdentityKeys {
            tripcode: None,
            poster_id: Some(&key),
        };
        if ordinary {
            board_store::create_ordinary_staff_post(
                &self.staff,
                &self.boards[board],
                parent,
                &post,
                context,
                PostMetadata {
                    drawing: None,
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
                &self.boards[board],
                parent,
                &post,
                context,
                keys,
                authority,
            )
            .await
        }
    }

    async fn public_write(
        &self,
        board: usize,
        parent: i64,
        peer: IpAddr,
        epoch: i64,
    ) -> Result<i64, StoreError> {
        self.public_attachment(board, parent, peer, epoch, None)
            .await
    }

    async fn public_attachment(
        &self,
        board: usize,
        parent: i64,
        peer: IpAddr,
        epoch: i64,
        attachment: Option<&board_store::post_media::NewAttachment>,
    ) -> Result<i64, StoreError> {
        let key = PosterIdKey::parse(&self.key).unwrap();
        board_store::create_post_with_metadata(
            &self.public,
            &self.boards[board],
            parent,
            &post(),
            attachment,
            context(peer, epoch),
            PostMetadata {
                drawing: None,
                keys: PostIdentityKeys {
                    tripcode: None,
                    poster_id: Some(&key),
                },
                country_database: None,
                flag: "",
                options: "",
                spoiler: false,
            },
        )
        .await
    }

    async fn policy(&self, board: usize, limit: i32, hours: i32) {
        sqlx::query("UPDATE content.boards SET user_thread_limit=$2,user_thread_period_hours=$3 WHERE slug=$1")
            .bind(&self.boards[board]).bind(limit).bind(hours).execute(&self.owner).await.unwrap();
    }

    async fn snapshot(&self) -> serde_json::Value {
        sqlx::query_scalar("SELECT jsonb_build_object('posts',(SELECT coalesce(jsonb_agg(to_jsonb(p) ORDER BY id),'[]') FROM content.posts p WHERE board=ANY($1)),'threads',(SELECT coalesce(jsonb_agg(to_jsonb(t) ORDER BY id),'[]') FROM content.threads t WHERE board=ANY($1)),'history',(SELECT coalesce(jsonb_agg(to_jsonb(h) ORDER BY post_id),'[]') FROM post_secrets.posting_history h WHERE board=ANY($1)),'actions',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY board,actor_hash),'[]') FROM post_secrets.posting_thread_actions a WHERE board=ANY($1)),'deletion',(SELECT coalesce(jsonb_agg(to_jsonb(d) ORDER BY post_id),'[]') FROM post_secrets.deletion d WHERE post_id IN(SELECT id FROM content.posts WHERE board=ANY($1))),'media',(SELECT coalesce(jsonb_agg(to_jsonb(m) ORDER BY post_id),'[]') FROM content.post_media m WHERE post_id IN(SELECT id FROM content.posts WHERE board=ANY($1))),'intents',(SELECT coalesce(jsonb_agg(to_jsonb(i) ORDER BY post_id),'[]') FROM post_secrets.staff_post_intents i WHERE board=ANY($1)),'audit',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY id),'[]') FROM content.moderation_audit a WHERE board=ANY($1)))")
            .bind(&self.boards[..]).fetch_one(&self.owner).await.unwrap()
    }

    async fn cleanup(self) {
        let mut tx = support::begin_cleanup(&self.owner, &self.boards).await;
        for query in [
            "DELETE FROM content.moderation_audit WHERE board=ANY($1)",
            "DELETE FROM post_secrets.staff_post_intents WHERE board=ANY($1)",
            "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=ANY($1))",
            "DELETE FROM content.post_media WHERE post_id IN(SELECT id FROM content.posts WHERE board=ANY($1))",
            "DELETE FROM content.posts WHERE board=ANY($1)",
            "DELETE FROM content.threads WHERE board=ANY($1)",
            "DELETE FROM post_secrets.posting_thread_actions WHERE board=ANY($1)",
            "DELETE FROM content.boards WHERE slug=ANY($1)",
        ] {
            sqlx::query(query)
                .bind(&self.boards[..])
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        let jobs = self.jobs.lock().unwrap().clone();
        for query in [
            "DELETE FROM media.assets WHERE job_id=ANY($1)",
            "DELETE FROM media.jobs WHERE id=ANY($1)",
        ] {
            sqlx::query(query)
                .bind(&jobs)
                .execute(&mut *tx)
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
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        tx.commit().await.unwrap();
    }
}

fn peer(n: u8) -> IpAddr {
    IpAddr::from([192, 0, 2, n])
}
fn post() -> NewPost {
    NewPost {
        name: "Anonymous".into(),
        subject: "Owned user thread quota".into(),
        comment: "Owned IP-only active OP quota fixture".into(),
        deletion_hash: "synthetic-not-a-password".into(),
        sage: false,
    }
}
fn context(peer: IpAddr, epoch: i64) -> PostingContext {
    PostingContext {
        request_start: chrono::DateTime::from_timestamp(epoch, 0).unwrap(),
        peer: Some(peer),
        op_password_proof: None,
    }
}
fn rejected(result: Result<i64, StoreError>, limit: i32) {
    let expected = format!(
        "Error: You may not post more than {limit} active thread{} at a time.",
        if limit > 1 { "s" } else { "" }
    );
    match result {
        Err(StoreError::ContentRejected(message)) => assert_eq!(message, expected),
        other => panic!("Expected IP-only active OP quota rejection: {other:?}"),
    }
}

async fn run<F, Fut>(test: F)
where
    F: FnOnce(Fixture) -> Fut,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let outcome = tokio::spawn(test(f.clone())).await;
    f.cleanup().await;
    outcome.unwrap();
}

#[tokio::test]
async fn cleanup_waits_for_parent_locks_before_deleting_private_rows() {
    let _serial = TEST.lock().await;
    for parent in ["board", "thread"] {
        let f = Fixture::new().await;
        let now = f.now().await;
        let op = f.public_write(0, 0, peer(91), now).await.unwrap();
        let before = f.snapshot().await;
        let mut blocker = f.owner.begin().await.unwrap();
        let blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *blocker)
            .await
            .unwrap();
        let expected_query = if parent == "board" {
            sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
                .bind(&f.boards[0])
                .fetch_one(&mut *blocker)
                .await
                .unwrap();
            "SELECT slug FROM content.boards WHERE slug=ANY($1) ORDER BY slug FOR UPDATE"
        } else {
            sqlx::query("SELECT id FROM content.threads WHERE id=$1 FOR UPDATE")
                .bind(op)
                .fetch_one(&mut *blocker)
                .await
                .unwrap();
            "SELECT id FROM content.threads WHERE board=ANY($1) ORDER BY board,id FOR UPDATE"
        };
        // A dedicated connection makes the observed waiter unambiguous.
        let cleanup_pool = PgPoolOptions::new()
            .max_connections(1)
            .connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let cleanup_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&cleanup_pool)
            .await
            .unwrap();
        let mut cleanup_fixture = f.clone();
        cleanup_fixture.owner = cleanup_pool.clone();
        let mut cleanup = tokio::spawn(cleanup_fixture.cleanup());
        // Observe the actual dependency and statement, not elapsed time. The
        // cleanup must wait at its parent SELECT, before any protected DELETE.
        let waiting = tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let query: Option<String> = sqlx::query_scalar(
                    "SELECT query FROM pg_stat_activity WHERE pid=$1 AND $2=ANY(pg_blocking_pids(pid))",
                )
                .bind(cleanup_pid)
                .bind(blocker_pid)
                .fetch_optional(&f.owner)
                .await
                .unwrap();
                if query.is_some() || cleanup.is_finished() {
                    break query;
                }
                tokio::task::yield_now().await;
            }
        })
        .await;
        let while_blocked = f.snapshot().await;
        // Release the holder and drain teardown even if lock observation
        // failed, so a regression cannot leave a waiting cleanup task behind.
        blocker.rollback().await.unwrap();
        let completed = tokio::time::timeout(Duration::from_secs(10), &mut cleanup).await;
        if completed.is_err() {
            cleanup.abort();
            let _ = cleanup.await;
        }
        cleanup_pool.close().await;
        assert_eq!(
            waiting.expect("cleanup parent-lock wait must be observable"),
            Some(expected_query.into()),
            "cleanup must acquire the {parent} lock before deleting private rows"
        );
        assert_eq!(while_blocked, before);
        completed
            .expect("cleanup must finish after its parent lock is released")
            .unwrap();
        let remaining: (i64, i64, i64, i64) = sqlx::query_as(
            "SELECT (SELECT count(*) FROM content.boards WHERE slug=ANY($1)),(SELECT count(*) FROM content.posts WHERE board=ANY($1)),(SELECT count(*) FROM post_secrets.deletion WHERE post_id=$2),(SELECT count(*) FROM staff_identity.accounts WHERE id=$3)",
        )
        .bind(&f.boards[..])
        .bind(op)
        .bind(f.account)
        .fetch_one(&f.owner)
        .await
        .unwrap();
        assert_eq!(remaining, (0, 0, 0, 0));
    }
}

#[tokio::test]
async fn source_defaults_imported_overrides_and_zero_one_plural_messages() {
    run(|f| async move {
        let default: (i32, i32) = sqlx::query_as("SELECT user_thread_limit,user_thread_period_hours FROM content.boards WHERE slug=$1")
            .bind(&f.boards[0]).fetch_one(&f.owner).await.unwrap();
        assert_eq!(default, (5, 24));
        for (board, limit, hours) in [
            ("g", 5, 24), ("a", 3, 24), ("bant", 3, 24), ("i", 3, 168),
            ("news", 5, 120), ("pol", 3, 6), ("qa", 3, 48), ("qst", 5, 72),
            ("test", 50, 24), ("v", 3, 24), ("vm", 3, 24), ("vmg", 3, 24),
            ("vrpg", 3, 24), ("vst", 3, 24),
        ] {
            let actual: (i32, i32) = sqlx::query_as("SELECT user_thread_limit,user_thread_period_hours FROM content.boards WHERE slug=$1")
                .bind(board).fetch_one(&f.owner).await.unwrap();
            assert_eq!(actual, (limit, hours), "Imported /{board}/ policy");
        }
        let now = f.now().await;
        for limit in [0, 1, 3] {
            let ip = peer(10 + limit as u8);
            f.policy(0, limit, 24).await;
            for n in 0..limit {
                f.public_write(0, 0, ip, now + i64::from(n) * 10).await.unwrap();
            }
            let before = f.snapshot().await;
            rejected(f.public_write(0, 0, ip, now + 100).await, limit);
            assert_eq!(f.snapshot().await, before);
            // Lower the quota below the already committed count without
            // deleting any history: above-limit is just as terminal as equal.
            if limit > 1 {
                f.policy(0, 1, 24).await;
                rejected(f.public_write(0, 0, ip, now + 110).await, 1);
            }
        }
    }).await;
}

#[tokio::test]
async fn public_ordinary_staff_and_badged_staff_share_ip_count_and_replies_do_not() {
    run(|f| async move {
        f.policy(0, 2, 24).await;
        let now = f.now().await;
        let ip = peer(20);
        let op = f.public_write(0, 0, ip, now).await.unwrap();
        f.write(0, op, ip, now + 10, true, true).await.unwrap();
        f.write(0, op, ip, now + 20, false, true).await.unwrap();
        f.public_write(0, op, ip, now + 30).await.unwrap();
        // Three replies did not consume the second OP slot.
        f.write(0, 0, ip, now + 40, true, true).await.unwrap();
        for ordinary in [false, true] {
            let before = f.snapshot().await;
            rejected(f.write(0, 0, ip, now + 50, ordinary, true).await, 2);
            assert_eq!(f.snapshot().await, before, "staff rejection is atomic");
        }
        rejected(f.public_write(0, 0, ip, now + 50).await, 2);
        f.public_write(0, op, ip, now + 60).await.unwrap();
        // Even a zero OP allowance leaves all three reply paths available.
        f.policy(0, 0, 24).await;
        f.public_write(0, op, ip, now + 70).await.unwrap();
        f.write(0, op, ip, now + 80, true, true).await.unwrap();
        f.write(0, op, ip, now + 90, false, true).await.unwrap();
        // The same peer on another board and another peer on this board have
        // independent quotas. Staff avoids the separate cross-board timer.
        f.policy(0, 2, 24).await;
        f.write(1, 0, ip, now + 100, false, true).await.unwrap();
        f.public_write(0, 0, peer(21), now + 100).await.unwrap();
    })
    .await;
}

#[tokio::test]
async fn strict_post_timestamp_cutoff_future_rows_and_reversed_request_clocks() {
    run(|f| async move {
        f.policy(0, 1, 1).await;
        let now = f.now().await;
        // Exactly at the lower cutoff is excluded; one second after counts.
        f.public_write(0, 0, peer(30), now - 3600).await.unwrap();
        f.public_write(0, 0, peer(30), now).await.unwrap();
        f.public_write(0, 0, peer(31), now - 3599).await.unwrap();
        rejected(f.public_write(0, 0, peer(31), now).await, 1);
        // No upper bound: a future OP counts even with a reversed request clock.
        f.public_write(0, 0, peer(32), now + 100).await.unwrap();
        rejected(f.public_write(0, 0, peer(32), now).await, 1);
        rejected(f.public_write(0, 0, peer(32), now - 100).await, 1);
        // Deliberately diverge private cooldown metadata from the actual post:
        // quota must use OP created_at, not h.request_at or the thread timestamp.
        let op = f.public_write(0, 0, peer(33), now - 4000).await.unwrap();
        sqlx::query(
            "UPDATE content.posts SET created_at=to_timestamp($2::double precision) WHERE id=$1",
        )
        .bind(op)
        .bind(now - 1)
        .execute(&f.owner)
        .await
        .unwrap();
        rejected(f.public_write(0, 0, peer(33), now).await, 1);
        // A zero-hour window still has the strict lower edge, not a disable switch.
        f.policy(0, 1, 0).await;
        f.public_write(0, 0, peer(34), now).await.unwrap();
        f.public_write(0, 0, peer(34), now).await.unwrap();
        f.public_write(0, 0, peer(35), now + 1).await.unwrap();
        rejected(f.public_write(0, 0, peer(35), now).await, 1);
    })
    .await;
}

#[tokio::test]
async fn sticky_locked_archive_deleted_and_file_only_lifecycle_semantics() {
    run(|f| async move {
        f.policy(0, 1, 24).await;
        let now = f.now().await;
        for (n, change, counted) in [
            (40, "UPDATE content.threads SET sticky=true WHERE id=$1", true),
            (41, "UPDATE content.threads SET closed=true WHERE id=$1", true),
            (42, "UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 day' WHERE id=$1", false),
            (43, "UPDATE content.posts SET deleted=true WHERE id=$1", false),
            (44, "UPDATE content.threads SET deleted=true WHERE id=$1", false),
        ] {
            let ip = peer(n);
            let op = f.public_write(0, 0, ip, now).await.unwrap();
            sqlx::query(change).bind(op).execute(&f.owner).await.unwrap();
            if counted {
                rejected(f.public_write(0, 0, ip, now + 10).await, 1);
            } else {
                f.public_write(0, 0, ip, now + 10).await.unwrap();
            }
        }
        // Synthetic media metadata is owned fixture data; the file-only action
        // itself goes through the real public attachment deletion function.
        let ip = peer(45);
        let op = f.public_write(0, 0, ip, now).await.unwrap();
        sqlx::query("INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler) SELECT $1,replace(gen_random_uuid()::text,'-',''),replace(gen_random_uuid()::text,'-',''),'owned.png',1,1,1,false")
            .bind(op).execute(&f.owner).await.unwrap();
        board_store::post_media::delete_attachment(&f.public, &f.boards[0], op).await.unwrap();
        let deleted: bool = sqlx::query_scalar("SELECT file_deleted FROM content.post_media WHERE post_id=$1")
            .bind(op).fetch_one(&f.owner).await.unwrap();
        assert!(deleted);
        rejected(f.public_write(0, 0, ip, now + 10).await, 1);
    }).await;
}

#[tokio::test]
async fn rollover_cannot_erase_the_count_before_rejection_and_all_mutations_roll_back() {
    run(|f| async move {
        f.policy(0, 1, 24).await;
        let now = f.now().await;
        let op = f.public_write(0, 0, peer(50), now).await.unwrap();
        let expired = f.public_write(0, 0, peer(51), now).await.unwrap();
        sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp()-interval '2 days',archive_expires_at=clock_timestamp()-interval '1 day' WHERE id=$1")
            .bind(expired).execute(&f.owner).await.unwrap();
        sqlx::query("UPDATE content.boards SET thread_limit=1,archive_retention_seconds=0,archive_limit=1 WHERE slug=$1")
            .bind(&f.boards[0]).execute(&f.owner).await.unwrap();
        let before = f.snapshot().await;
        rejected(f.public_write(0, 0, peer(50), now + 10).await, 1);
        assert_eq!(f.snapshot().await, before, "failed OP must not roll over its counted predecessor or trim an expired archive");
        for ordinary in [false, true] {
            rejected(f.write(0, 0, peer(50), now + 10, ordinary, true).await, 1);
            assert_eq!(f.snapshot().await, before);
        }
        let live: bool = sqlx::query_scalar("SELECT NOT deleted AND archived_at IS NULL FROM content.threads WHERE id=$1")
            .bind(op).fetch_one(&f.owner).await.unwrap();
        assert!(live);
    }).await;
}

#[tokio::test]
async fn concurrent_public_and_authenticated_staff_compete_for_exactly_one_final_slot() {
    run(|f| async move {
        f.policy(0, 2, 24).await;
        let now = f.now().await;
        f.public_write(0, 0, peer(60), now - 10).await.unwrap();
        let public = PgPoolOptions::new().max_connections(1)
            .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap()).await.unwrap();
        let staff = PgPoolOptions::new().max_connections(1)
            .connect(&std::env::var("STAFF_DATABASE_URL").unwrap()).await.unwrap();
        let public_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&public).await.unwrap();
        let staff_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&staff).await.unwrap();
        // Hold the actor gate with the actual authorized runtime role.
        let mut blocker = f.public.begin().await.unwrap();
        sqlx::query("SELECT content.lock_posting_actor($1,true)")
            .bind(f.actor(peer(60)).as_slice()).execute(&mut *blocker).await.unwrap();
        let mut public_fixture = f.clone();
        public_fixture.public = public.clone();
        let mut staff_fixture = f.clone();
        staff_fixture.staff = staff.clone();
        let one = tokio::spawn(async move { public_fixture.public_write(0, 0, peer(60), now).await });
        let two = tokio::spawn(async move { staff_fixture.write(0, 0, peer(60), now, false, true).await });
        // Both real writer backends must be waiting concurrently before either
        // can consume the final slot. The second may wait on the first waiter.
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                let waiting: bool = sqlx::query_scalar("SELECT cardinality(pg_blocking_pids($1))>0 AND cardinality(pg_blocking_pids($2))>0")
                    .bind(public_pid).bind(staff_pid).fetch_one(&f.owner).await.unwrap();
                if waiting { break; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await.expect("both final-slot writers must actually overlap");
        blocker.commit().await.unwrap();
        let one = tokio::time::timeout(Duration::from_secs(10), one).await.unwrap().unwrap();
        let two = tokio::time::timeout(Duration::from_secs(10), two).await.unwrap().unwrap();
        public.close().await;
        staff.close().await;
        match (one, two) {
            (Ok(_), error @ Err(_)) | (error @ Err(_), Ok(_)) => rejected(error, 2),
            other => panic!("Exactly one final-slot writer must commit: {other:?}"),
        }
        let counts: (i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM content.posts WHERE board=$1),(SELECT count(*) FROM post_secrets.posting_history WHERE board=$1)")
            .bind(&f.boards[0]).fetch_one(&f.owner).await.unwrap();
        assert_eq!(counts, (2, 2));
        rejected(f.public_write(0, 0, peer(60), now + 10).await, 2);
    }).await;
}

#[tokio::test]
async fn private_boards_and_badges_do_not_bypass_quota_or_authentication() {
    run(|f| async move {
        f.policy(0, 1, 24).await;
        sqlx::query("UPDATE content.boards SET staff_only=true WHERE slug=$1")
            .bind(&f.boards[0])
            .execute(&f.owner)
            .await
            .unwrap();
        let now = f.now().await;
        let op = f.write(0, 0, peer(70), now, false, true).await.unwrap();
        let before = f.snapshot().await;
        rejected(f.write(0, 0, peer(70), now + 10, false, true).await, 1);
        assert_eq!(f.snapshot().await, before);
        assert!(matches!(
            f.write(0, 0, peer(71), now + 10, false, false).await,
            Err(StoreError::AuthorizationChanged)
        ));
        assert_eq!(f.snapshot().await, before);
        // Private discussion uses the dedicated staff writer. Ordinary staff
        // admission retains the existing public-board visibility boundary,
        // regardless of whether the later staff proof would be valid.
        for valid in [false, true] {
            match f.write(0, 0, peer(70), now + 10, true, valid).await {
                Err(StoreError::Database(error)) => assert_eq!(
                    error.as_database_error().unwrap().code().as_deref(),
                    Some("P0002")
                ),
                other => panic!("ordinary private posting must remain unavailable: {other:?}"),
            }
            assert_eq!(f.snapshot().await, before);
        }
        for parent in [0, op] {
            assert!(matches!(
                f.public_write(0, parent, peer(70), now + 10).await,
                Err(StoreError::NotFound)
            ));
        }
        let error = sqlx::query("SELECT * FROM content.check_user_thread_quota($1,$2,$3)")
            .bind(f.actor(peer(70)).as_slice())
            .bind(&f.boards[0])
            .bind(now)
            .fetch_all(&f.public)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("P0002")
        );
        assert_eq!(f.snapshot().await, before);
        f.write(0, op, peer(70), now + 10, false, true)
            .await
            .unwrap();
    })
    .await;
}

#[tokio::test]
async fn missing_identity_invalid_sql_context_and_real_role_denials_fail_closed() {
    run(|f| async move {
        let now = f.now().await;
        let key = PosterIdKey::parse(&f.key).unwrap();
        let before = f.snapshot().await;
        for (have_key, have_peer) in [(false, true), (true, false), (false, false)] {
            let mut ctx = context(peer(80), now);
            ctx.peer = have_peer.then_some(peer(80));
            let result = board_store::create_post_with_identity_keys(
                &f.public, &f.boards[0], 0, &post(), None, ctx,
                PostIdentityKeys { tripcode: None, poster_id: have_key.then_some(&key) },
            ).await;
            assert!(matches!(result, Err(StoreError::Database(_))), "missing identity must not manufacture an actor");
            assert_eq!(f.snapshot().await, before);
        }
        for actor in [None, Some(Vec::new()), Some(vec![1_u8; 31]), Some(vec![1_u8; 33])] {
            let error = sqlx::query("SELECT * FROM content.check_user_thread_quota($1,$2,$3)")
                .bind(actor).bind(&f.boards[0]).bind(now).fetch_all(&f.public).await.unwrap_err();
            assert_eq!(error.as_database_error().unwrap().code().as_deref(), Some("23514"));
        }
        for epoch in [None, Some(-1_i64), Some(i64::MAX)] {
            let error = sqlx::query("SELECT * FROM content.check_user_thread_quota($1,$2,$3)")
                .bind(f.actor(peer(80)).as_slice()).bind(&f.boards[0]).bind(epoch)
                .fetch_all(&f.public).await.unwrap_err();
            assert_eq!(error.as_database_error().unwrap().code().as_deref(), Some("23514"));
        }
        let signature = "content.check_user_thread_quota(bytea,text,bigint)";
        let secure: bool = sqlx::query_scalar("SELECT p.prosecdef AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp'] AND r.rolname='board_posting_cooldown_owner' AND NOT(r.rolcanlogin OR r.rolsuper OR r.rolcreatedb OR r.rolcreaterole OR r.rolreplication OR r.rolbypassrls) AND NOT EXISTS(SELECT 1 FROM aclexplode(p.proacl) a WHERE a.grantee=0 AND a.privilege_type='EXECUTE') FROM pg_proc p JOIN pg_roles r ON r.oid=p.proowner WHERE p.oid=$1::regprocedure")
            .bind(signature).fetch_one(&f.owner).await.unwrap();
        assert!(secure);
        for (env, role) in [
            ("AUTH_DATABASE_URL", "board_auth"),
            ("MEDIA_DATABASE_URL", "board_media"),
            ("MEDIA_READ_DATABASE_URL", "board_media_read"),
            ("INTAKE_DATABASE_URL", "board_media_intake"),
            ("MONITOR_DATABASE_URL", "board_monitor"),
        ] {
            let connection = pool(env).await;
            let actual: String = sqlx::query_scalar("SELECT current_user::text").fetch_one(&connection).await.unwrap();
            assert_eq!(actual, role);
            let error = sqlx::query("SELECT * FROM content.check_user_thread_quota($1,$2,$3)")
                .bind(f.actor(peer(80)).as_slice()).bind(&f.boards[0]).bind(now)
                .fetch_all(&connection).await.unwrap_err();
            assert_eq!(error.as_database_error().unwrap().code().as_deref(), Some("42501"), "{role}");
            connection.close().await;
        }
        for connection in [&f.public, &f.staff, &f.auth] {
            let error = sqlx::query("SELECT actor_hash FROM post_secrets.posting_history")
                .fetch_all(connection).await.unwrap_err();
            assert_eq!(error.as_database_error().unwrap().code().as_deref(), Some("42501"));
        }
        let mut repeatable = f.public.begin().await.unwrap();
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *repeatable).await.unwrap();
        let error = sqlx::query("SELECT * FROM content.check_user_thread_quota($1,$2,$3)")
            .bind(f.actor(peer(80)).as_slice()).bind(&f.boards[0]).bind(now)
            .fetch_all(&mut *repeatable).await.unwrap_err();
        assert_eq!(error.as_database_error().unwrap().code().as_deref(), Some("22023"));
        repeatable.rollback().await.unwrap();
        assert_eq!(f.snapshot().await, before);
    }).await;
}

#[tokio::test]
async fn rejected_attachment_op_leaves_capability_media_clock_and_content_untouched() {
    run(|f| async move {
        use board_store::{media::MediaQueue, media_assets::OutputMetadata, media_intake::IntakeStore, post_media::NewAttachment};
        f.policy(0, 1, 24).await;
        // The source default image_limit=0 disallows image replies. Only this
        // owned media fixture enables image replies so capability reuse is real.
        sqlx::query("UPDATE content.boards SET image_limit=10 WHERE slug=$1")
            .bind(&f.boards[0]).execute(&f.owner).await.unwrap();
        let now = f.now().await;
        let op = f.public_write(0, 0, peer(90), now).await.unwrap();
        let intake = IntakeStore::connect(&std::env::var("INTAKE_DATABASE_URL").unwrap()).await.unwrap();
        let queue = MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap()).await.unwrap();
        let upload = intake.reserve("owned-quota.png").await.unwrap();
        f.jobs.lock().unwrap().push(upload.id.clone());
        intake.begin_upload(&upload.id, &upload.capability).await.unwrap();
        intake.finish_upload(&upload.id, &upload.capability, 100).await.unwrap();
        let claim = queue.claim().await.unwrap().expect("owned qualification queue is idle");
        assert_eq!(claim.id, upload.id);
        let lease = claim.lease_token.unwrap();
        let output = queue.prepare_output(&claim.id, &lease, &OutputMetadata {
            sha256: "a".repeat(64), bytes: 100, width: 10, height: 10,
        }).await.unwrap();
        queue.approve_output(&claim.id, &lease, &output.id).await.unwrap();
        let attachment = NewAttachment { upload, spoiler: false };
        let before = f.snapshot().await;
        let media_before: serde_json::Value = sqlx::query_scalar("SELECT jsonb_build_object('job',(SELECT to_jsonb(j) FROM media.jobs j WHERE id=$1),'asset',(SELECT to_jsonb(a) FROM media.assets a WHERE id=$2),'clock',(SELECT last_number FROM content.media_clock WHERE singleton))")
            .bind(&attachment.upload.id).bind(&output.id).fetch_one(&f.owner).await.unwrap();
        rejected(f.public_attachment(0, 0, peer(90), now + 10, Some(&attachment)).await, 1);
        assert_eq!(f.snapshot().await, before);
        let media_after: serde_json::Value = sqlx::query_scalar("SELECT jsonb_build_object('job',(SELECT to_jsonb(j) FROM media.jobs j WHERE id=$1),'asset',(SELECT to_jsonb(a) FROM media.assets a WHERE id=$2),'clock',(SELECT last_number FROM content.media_clock WHERE singleton))")
            .bind(&attachment.upload.id).bind(&output.id).fetch_one(&f.owner).await.unwrap();
        assert_eq!(media_after, media_before);
        // Reuse the exact capability through a real reply while OP quota is
        // still full: failed OP never consumed it, and replies bypass quota.
        let reply = f.public_attachment(0, op, peer(90), now + 10, Some(&attachment)).await.unwrap();
        let saved = board_store::post_media::attachment(&f.public, reply).await.unwrap().unwrap();
        assert_eq!(saved.asset_id, output.id);
        rejected(f.public_write(0, 0, peer(90), now + 20).await, 1);
    }).await;
}
