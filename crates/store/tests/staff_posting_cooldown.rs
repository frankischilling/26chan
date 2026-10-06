#![cfg(feature = "database-tests")]

use board_domain::poster_id::PosterIdKey;
use board_store::{
    NewPost, PostIdentityKeys, PostMetadata, PostingContext, PostingCooldownReason,
    StaffPostAuthority, StaffPostIdentity, StoreError,
};
use sqlx::{PgPool, Postgres, Transaction};
use std::net::IpAddr;

// Source imgboard.php:6004-6016: trusted staff have a fixed five-second
// same-board, same-actor timer over the newest surviving post ID, OPs included.
// These text-only tests deliberately make no Pass, duplicate or named-metajanitor claims.
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
            // Copy imported ordinary timers, never relax them or mutate imported boards.
            let affected = sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) SELECT $1,'Owned staff timer','Synthetic fixture',2000,100,100,100,10,posting_reply_seconds,posting_image_seconds,posting_thread_seconds FROM content.boards WHERE slug='g'")
                .bind(board).execute(&owner).await.unwrap().rows_affected();
            assert_eq!(affected, 1, "Imported /g/ policy must be present");
        }
        let account = sqlx::query_scalar("INSERT INTO staff_identity.accounts(role,flags) VALUES('moderator',ARRAY['capcode','capcodename','developer']) RETURNING id")
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
            identity: ordinary.then_some(StaffPostIdentity {
                capcode: None,
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
        let key = PosterIdKey::parse(&self.key).unwrap();
        board_store::create_post_with_metadata(
            &self.public,
            &self.boards[board],
            parent,
            &post(),
            None,
            context(peer, epoch),
            PostMetadata {
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

    // Controlled-clock history through the real public INSERT trigger. Only
    // fixture seeding skips admission; every tested staff write uses real proof.
    async fn seed(&self, board: usize, parent: i64, peer: IpAddr, epoch: i64) -> i64 {
        let mut tx = self.public.begin().await.unwrap();
        let actor = self.actor(peer);
        prepare(&mut tx, &actor, parent == 0).await;
        let id: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        let thread = if parent == 0 {
            sqlx::query("INSERT INTO content.threads(id,board) VALUES($1,$2)")
                .bind(id)
                .bind(&self.boards[board])
                .execute(&mut *tx)
                .await
                .unwrap();
            id
        } else {
            parent
        };
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at) VALUES($1,$2,$3,'Anonymous','Owned staff timer','Owned seed',to_timestamp($4::double precision))")
            .bind(id).bind(&self.boards[board]).bind(thread).bind(epoch).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        id
    }

    async fn snapshot(&self) -> serde_json::Value {
        sqlx::query_scalar("SELECT jsonb_build_object('posts',(SELECT coalesce(jsonb_agg(to_jsonb(p) ORDER BY id),'[]') FROM content.posts p WHERE board=ANY($1)),'threads',(SELECT coalesce(jsonb_agg(to_jsonb(t) ORDER BY id),'[]') FROM content.threads t WHERE board=ANY($1)),'history',(SELECT coalesce(jsonb_agg(to_jsonb(h) ORDER BY post_id),'[]') FROM post_secrets.posting_history h WHERE board=ANY($1)),'actions',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY board,actor_hash),'[]') FROM post_secrets.posting_thread_actions a WHERE board=ANY($1)),'audit',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY id),'[]') FROM content.moderation_audit a WHERE board=ANY($1)))")
            .bind(&self.boards[..]).fetch_one(&self.owner).await.unwrap()
    }

    async fn cleanup(self) {
        for query in [
            "DELETE FROM content.moderation_audit WHERE board=ANY($1)",
            "DELETE FROM post_secrets.staff_post_intents WHERE board=ANY($1)",
            "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=ANY($1))",
            "DELETE FROM content.posts WHERE board=ANY($1)",
            "DELETE FROM content.threads WHERE board=ANY($1)",
            "DELETE FROM post_secrets.posting_thread_actions WHERE board=ANY($1)",
            "DELETE FROM content.boards WHERE slug=ANY($1)",
        ] {
            sqlx::query(query)
                .bind(&self.boards[..])
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

fn peer(n: u8) -> IpAddr {
    IpAddr::from([192, 0, 2, n])
}
fn post() -> NewPost {
    NewPost {
        name: "Anonymous".into(),
        subject: "Owned staff timer".into(),
        comment: "Owned text-only staff timer fixture".into(),
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
async fn prepare(tx: &mut Transaction<'_, Postgres>, actor: &[u8], op: bool) {
    sqlx::query("SELECT content.lock_posting_actor($1,$2)")
        .bind(actor)
        .bind(op)
        .execute(&mut **tx)
        .await
        .unwrap();
    sqlx::query("SELECT set_config('board.posting_actor',encode($1::bytea,'hex'),true)")
        .bind(actor)
        .execute(&mut **tx)
        .await
        .unwrap();
}
fn rejected(result: Result<i64, StoreError>, seconds: i64) {
    match result {
        Err(StoreError::PostingCooldownRejected(r)) => {
            assert_eq!(r.reason, PostingCooldownReason::Reply);
            assert_eq!(r.remaining_seconds, seconds);
            assert_eq!(
                r.source_message(),
                format!(
                    "Error: You must wait  {seconds} second{} before posting a reply.",
                    if seconds == 1 { "" } else { "s" }
                )
            );
        }
        other => panic!("Expected staff timer rejection: {other:?}"),
    }
}

#[tokio::test]
async fn four_seconds_rejects_five_accepts_all_op_reply_pairs_and_staff_modes() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let now = f.now().await;
    let mut n = 1;
    for ordinary in [false, true] {
        for previous_reply in [false, true] {
            for incoming_reply in [false, true] {
                let ip = peer(n);
                n += 1;
                let op = f.seed(0, 0, ip, now - 1000).await;
                f.write(
                    0,
                    if previous_reply { op } else { 0 },
                    ip,
                    now,
                    ordinary,
                    true,
                )
                .await
                .unwrap();
                let before = f.snapshot().await;
                let parent = if incoming_reply { op } else { 0 };
                rejected(f.write(0, parent, ip, now + 4, ordinary, true).await, 1);
                assert_eq!(
                    f.snapshot().await,
                    before,
                    "Rejected writes must roll back every content/history/action change"
                );
                f.write(0, parent, ip, now + 5, ordinary, true)
                    .await
                    .unwrap();
            }
        }
    }
    f.cleanup().await;
}

#[tokio::test]
async fn newest_post_id_not_max_time_and_board_peer_isolation() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let now = f.now().await;
    let op = f.seed(0, 0, peer(20), now).await;
    let newer = f.seed(0, op, peer(20), now - 100).await;
    assert!(newer > op);
    f.write(0, 0, peer(20), now, false, true).await.unwrap();
    // An OP with a lower timestamp and higher ID likewise supersedes a reply.
    let newer_op = f.seed(0, 0, peer(20), now - 100).await;
    assert!(newer_op > newer);
    f.write(0, op, peer(20), now, false, true).await.unwrap();
    rejected(f.write(0, op, peer(20), now + 4, false, true).await, 1);
    f.write(1, 0, peer(20), now + 4, false, true).await.unwrap();
    f.write(0, op, peer(21), now + 4, false, true)
        .await
        .unwrap();
    // Sticky status does not exempt the newest OP from this staff timer.
    let sticky = f.seed(0, 0, peer(22), now).await;
    sqlx::query("UPDATE content.threads SET sticky=true WHERE id=$1")
        .bind(sticky)
        .execute(&f.owner)
        .await
        .unwrap();
    rejected(f.write(0, op, peer(22), now + 4, false, true).await, 1);
    f.cleanup().await;
}

#[tokio::test]
async fn public_staff_mixed_history_preserves_imported_public_policy() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let now = f.now().await;
    let op = f.public_write(0, 0, peer(30), now).await.unwrap();
    rejected(f.write(0, op, peer(30), now + 4, false, true).await, 1);
    let staff = f
        .write(0, op, peer(30), now + 5, false, true)
        .await
        .unwrap();
    let delay: i32 =
        sqlx::query_scalar("SELECT posting_reply_seconds FROM content.boards WHERE slug=$1")
            .bind(&f.boards[0])
            .fetch_one(&f.owner)
            .await
            .unwrap();
    assert!(delay > 5, "Fixture retains imported ordinary cooldown");
    rejected(
        f.public_write(0, op, peer(30), now + 10).await,
        i64::from(delay) - 5,
    );
    f.public_write(0, op, peer(30), now + 5 + i64::from(delay))
        .await
        .unwrap();
    let history: bool = sqlx::query_scalar(
        "SELECT actor_hash=$2 FROM post_secrets.posting_history WHERE post_id=$1",
    )
    .bind(staff)
    .bind(f.actor(peer(30)).as_slice())
    .fetch_one(&f.owner)
    .await
    .unwrap();
    assert!(
        history,
        "Real staff proof registers the same trusted actor as public posting"
    );
    f.cleanup().await;
}

#[tokio::test]
async fn simultaneous_staff_writes_serialize_and_rejected_transaction_leaves_no_state() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let now = f.now().await;
    let op = f.seed(0, 0, peer(40), now - 1000).await;
    let (one, two) = tokio::join!(
        f.write(0, op, peer(40), now, false, true),
        f.write(0, op, peer(40), now, true, true)
    );
    match (one, two) {
        (Ok(_), error @ Err(_)) | (error @ Err(_), Ok(_)) => rejected(error, 5),
        other => panic!("Exactly one same-actor staff post must commit: {other:?}"),
    }
    let counts: (i64, i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM content.posts WHERE board=$1),(SELECT count(*) FROM post_secrets.posting_history WHERE board=$1),(SELECT count(*) FROM content.moderation_audit WHERE board=$1)")
        .bind(&f.boards[0]).fetch_one(&f.owner).await.unwrap();
    assert_eq!(counts, (2, 2, 1));
    let before = f.snapshot().await;
    rejected(f.write(0, 0, peer(40), now + 4, false, true).await, 1);
    assert_eq!(f.snapshot().await, before);
    f.write(0, 0, peer(40), now + 5, false, true).await.unwrap();
    f.cleanup().await;
}

#[tokio::test]
async fn invalid_proof_fails_before_timer_and_capability_keeps_history_private() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let now = f.now().await;
    let op = f.seed(0, 0, peer(50), now).await;
    for ordinary in [false, true] {
        let before = f.snapshot().await;
        assert!(matches!(
            f.write(0, op, peer(50), now + 4, ordinary, false).await,
            Err(StoreError::AuthorizationChanged)
        ));
        assert_eq!(f.snapshot().await, before);
    }
    let signature = "content.check_staff_posting_cooldown(bytea,text,bigint)";
    let secure: bool = sqlx::query_scalar("SELECT p.prosecdef AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp'] AND r.rolname='board_posting_cooldown_owner' AND NOT(r.rolcanlogin OR r.rolsuper OR r.rolcreatedb OR r.rolcreaterole OR r.rolreplication OR r.rolbypassrls) AND has_function_privilege('board_staff',p.oid,'EXECUTE') AND NOT EXISTS(SELECT 1 FROM aclexplode(p.proacl) a WHERE a.grantee=0 AND a.privilege_type='EXECUTE') FROM pg_proc p JOIN pg_roles r ON r.oid=p.proowner WHERE p.oid=$1::regprocedure")
        .bind(signature).fetch_one(&f.owner).await.unwrap();
    assert!(secure);
    for role in [
        "board_public",
        "board_auth",
        "board_media",
        "board_media_read",
        "board_monitor",
        "board_media_intake",
    ] {
        let allowed: bool = sqlx::query_scalar("SELECT has_function_privilege($1,$2,'EXECUTE')")
            .bind(role)
            .bind(signature)
            .fetch_one(&f.owner)
            .await
            .unwrap();
        assert!(!allowed, "Unexpected staff timer capability for {role}");
    }
    for connection in [&f.staff, &f.public, &f.auth] {
        let error = sqlx::query("SELECT actor_hash,request_at FROM post_secrets.posting_history")
            .fetch_all(connection)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501")
        );
    }
    let error = sqlx::query("SELECT * FROM content.check_staff_posting_cooldown($1,$2,$3)")
        .bind(f.actor(peer(50)).as_slice())
        .bind(&f.boards[0])
        .bind(now)
        .fetch_all(&f.public)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
    for actor in [
        None,
        Some(vec![]),
        Some(vec![0_u8; 31]),
        Some(vec![0_u8; 33]),
    ] {
        let error = sqlx::query("SELECT * FROM content.check_staff_posting_cooldown($1,$2,$3)")
            .bind(actor)
            .bind(&f.boards[0])
            .bind(now)
            .fetch_all(&f.staff)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
    }
    f.cleanup().await;
}

#[tokio::test]
async fn staff_op_checks_recent_history_before_rollover_can_erase_it() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let now = f.now().await;
    sqlx::query("UPDATE content.boards SET thread_limit=1 WHERE slug=$1")
        .bind(&f.boards[0])
        .execute(&f.owner)
        .await
        .unwrap();
    let op = f.public_write(0, 0, peer(60), now).await.unwrap();
    let before = f.snapshot().await;
    rejected(f.write(0, 0, peer(60), now + 4, false, true).await, 1);
    assert_eq!(
        f.snapshot().await,
        before,
        "Rejected staff OP must not roll away the recent OP/history it must check"
    );
    let accepted = f.write(0, 0, peer(60), now + 5, false, true).await.unwrap();
    let live: Vec<i64> = sqlx::query_scalar("SELECT id FROM content.threads WHERE board=$1 AND NOT deleted AND archived_at IS NULL ORDER BY id")
        .bind(&f.boards[0]).fetch_all(&f.owner).await.unwrap();
    assert_eq!(
        live,
        vec![accepted],
        "Normal rollover happens only after the strict five-second gate accepts"
    );
    let retained: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM post_secrets.posting_history WHERE post_id=$1)",
    )
    .bind(op)
    .fetch_one(&f.owner)
    .await
    .unwrap();
    assert!(!retained, "Normal rollover clears the previous OP identity");
    f.cleanup().await;
}

#[tokio::test]
async fn staff_actor_gate_precedes_board_lock_and_waiter_sees_preceding_commit() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let now = f.now().await;
    let op = f.seed(0, 0, peer(70), now - 1000).await;
    let mut queued = f.clone();
    queued.staff = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&queued.staff)
        .await
        .unwrap();
    // An actual preceding public INSERT holds the same actor gate until commit.
    let mut held = f.public.begin().await.unwrap();
    let actor = f.actor(peer(70));
    prepare(&mut held, &actor, false).await;
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *held)
        .await
        .unwrap();
    let task =
        tokio::spawn(async move { queued.write(0, op, peer(70), now + 4, false, true).await });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT $2=ANY(pg_blocking_pids($1))")
                .bind(pid)
                .bind(blocker)
                .fetch_one(&f.owner)
                .await
                .unwrap();
            if waiting {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("Staff writer must wait on the public writer's actor gate");
    let mut board_probe = f.owner.begin().await.unwrap();
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE NOWAIT")
        .bind(&f.boards[0])
        .execute(&mut *board_probe)
        .await
        .expect("Queued staff writer must not lock its board before its actor");
    board_probe.rollback().await.unwrap();
    let recent: i64 = sqlx::query_scalar("INSERT INTO content.posts(board,thread_id,name,subject,comment,created_at) VALUES($1,$2,'Anonymous','Owned serialization','Owned public preceding commit',to_timestamp($3::double precision)) RETURNING id")
        .bind(&f.boards[0]).bind(op).bind(now).fetch_one(&mut *held).await.unwrap();
    held.commit().await.unwrap();
    rejected(
        tokio::time::timeout(std::time::Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap(),
        1,
    );
    let ids: Vec<i64> = sqlx::query_scalar(
        "SELECT post_id FROM post_secrets.posting_history WHERE board=$1 ORDER BY post_id",
    )
    .bind(&f.boards[0])
    .fetch_all(&f.owner)
    .await
    .unwrap();
    assert_eq!(
        ids,
        vec![op, recent],
        "Staff waiter must observe committed public history and leave no post behind"
    );
    f.cleanup().await;
}

async fn issue_unused_proof(f: &Fixture, ordinary: bool) -> Vec<u8> {
    let ticket: Vec<u8> =
        sqlx::query_scalar("SELECT sha256(convert_to(gen_random_uuid()::text,'UTF8'))")
            .fetch_one(&f.owner)
            .await
            .unwrap();
    let id: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&f.owner)
        .await
        .unwrap();
    let stamp = chrono::DateTime::from_timestamp(f.now().await, 0).unwrap();
    if ordinary {
        let (limit, user_ids, op_markup): (i32, bool, bool) = sqlx::query_as("SELECT max_authorized_comment_chars,user_ids,op_markup FROM content.boards WHERE slug=$1")
            .bind(&f.boards[0]).fetch_one(&f.owner).await.unwrap();
        let key = PosterIdKey::parse(&f.key).unwrap();
        let ip = peer(80);
        let count = key.count_context(&f.boards[0], id, ip).unwrap();
        let bound = serde_json::json!({
            "poster_id": if user_ids { key.label(&f.boards[0], id, ip).unwrap() } else { String::new() },
            "poster_fingerprint": count.fingerprint, "poster_epoch": count.epoch,
            "post_sage": "false", "country": "", "country_name": "", "flag": "",
            "source_op_reply": if op_markup { "true" } else { "false" },
            "dice_result": "", "fortune_text": "", "fortune_color": "",
            "peer": ip.to_string(), "deletion_hash": "synthetic-not-a-password", "op_password_proof": ""
        });
        sqlx::query("SELECT staff_identity.issue_ordinary_post_authority($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19)")
            .bind(&ticket).bind(&f.session).bind(&f.csrf).bind(900_i32).bind(id).bind(&f.boards[0]).bind(id)
            .bind("Anonymous").bind("Owned unused authority").bind("Owned authority cleanup fixture").bind(stamp)
            .bind(true).bind(limit).bind(None::<Vec<u8>>).bind(None::<String>).bind("").bind(None::<String>).bind(true).bind(bound)
            .execute(&f.auth).await.unwrap();
    } else {
        sqlx::query(
            "SELECT staff_identity.issue_post_authority($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
        )
        .bind(&ticket)
        .bind(&f.session)
        .bind(&f.csrf)
        .bind(900_i32)
        .bind(false)
        .bind(id)
        .bind(&f.boards[0])
        .bind(id)
        .bind("Anonymous")
        .bind("Owned unused authority")
        .bind("Owned authority cleanup fixture")
        .bind(stamp)
        .execute(&f.auth)
        .await
        .unwrap();
    }
    ticket
}

#[tokio::test]
async fn rejected_badged_proofs_are_discarded_only_by_matching_ticket_session_and_kind() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let now = f.now().await;
    let op = f.seed(0, 0, peer(80), now).await;
    rejected(f.write(0, op, peer(80), now + 4, false, true).await, 1);
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM post_secrets.staff_post_intents WHERE account_id=$1",
    )
    .bind(f.account)
    .fetch_one(&f.owner)
    .await
    .unwrap();
    assert_eq!(
        count, 0,
        "Rejected badged staff write must discard its separately issued proof"
    );
    let badged = issue_unused_proof(&f, false).await;
    let ordinary = issue_unused_proof(&f, true).await;
    let exists = |ticket: Vec<u8>| async {
        sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(SELECT 1 FROM post_secrets.staff_post_intents WHERE token_hash=$1)",
        )
        .bind(ticket)
        .fetch_one(&f.owner)
        .await
        .unwrap()
    };
    let wrong = vec![0_u8; 32];
    for (ticket, session) in [
        (&wrong, &f.session),
        (&badged, &wrong),
        (&ordinary, &f.session),
    ] {
        sqlx::query("SELECT staff_identity.discard_badged_post_authority($1,$2)")
            .bind(ticket)
            .bind(session)
            .execute(&f.auth)
            .await
            .unwrap();
        assert!(
            exists(badged.clone()).await,
            "Unmatched cleanup must preserve another badged proof"
        );
        assert!(
            exists(ordinary.clone()).await,
            "Badged cleanup must never delete an ordinary proof"
        );
    }
    sqlx::query("SELECT staff_identity.discard_badged_post_authority($1,$2)")
        .bind(&badged)
        .bind(&f.session)
        .execute(&f.auth)
        .await
        .unwrap();
    assert!(!exists(badged.clone()).await);
    assert!(exists(ordinary.clone()).await);
    // Repeating the exact cleanup is harmless and cannot consume another proof.
    sqlx::query("SELECT staff_identity.discard_badged_post_authority($1,$2)")
        .bind(&badged)
        .bind(&f.session)
        .execute(&f.auth)
        .await
        .unwrap();
    assert!(exists(ordinary).await);
    let secure: bool = sqlx::query_scalar("SELECT p.prosecdef AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp'] AND r.rolname='board_staff_post_owner' AND NOT(r.rolcanlogin OR r.rolsuper OR r.rolcreatedb OR r.rolcreaterole OR r.rolreplication OR r.rolbypassrls) AND has_function_privilege('board_auth',p.oid,'EXECUTE') AND NOT has_function_privilege('board_staff',p.oid,'EXECUTE') AND NOT has_function_privilege('board_public',p.oid,'EXECUTE') AND NOT EXISTS(SELECT 1 FROM aclexplode(p.proacl) a WHERE a.grantee=0 AND a.privilege_type='EXECUTE') FROM pg_proc p JOIN pg_roles r ON r.oid=p.proowner WHERE p.oid='staff_identity.discard_badged_post_authority(bytea,bytea)'::regprocedure")
        .fetch_one(&f.owner).await.unwrap();
    assert!(
        secure,
        "Badged cleanup is an auth-only, least-privilege capability"
    );
    for invalid in [
        None,
        Some(vec![]),
        Some(vec![0_u8; 31]),
        Some(vec![0_u8; 33]),
    ] {
        for bad_ticket in [false, true] {
            let (ticket, session) = if bad_ticket {
                (invalid.as_deref(), Some(f.session.as_slice()))
            } else {
                (Some(badged.as_slice()), invalid.as_deref())
            };
            assert!(
                sqlx::query("SELECT staff_identity.discard_badged_post_authority($1,$2)")
                    .bind(ticket)
                    .bind(session)
                    .execute(&f.auth)
                    .await
                    .is_err(),
                "Malformed cleanup identity must fail closed"
            );
        }
    }
    f.cleanup().await;
}
