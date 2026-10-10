#![cfg(feature = "database-tests")]

use board_domain::poster_id::PosterIdKey;
use board_store::{NewPost, PostIdentityKeys, PostMetadata, PostingContext, StoreError};
use sqlx::{PgPool, Postgres, Transaction};
use std::net::{IpAddr, Ipv6Addr};

// Capacity fixtures are rollback-only and must not overlap this binary's tests.
static TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone)]
struct Fixture {
    owner: PgPool,
    public: PgPool,
    boards: [String; 2],
    actor: Vec<u8>,
}

impl Fixture {
    async fn new() -> Self {
        let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let public =
            board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
                .await
                .unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, String>("SELECT current_user::text")
                .fetch_one(&public)
                .await
                .unwrap(),
            "board_public"
        );
        let seed: String =
            sqlx::query_scalar("SELECT substr(replace(gen_random_uuid()::text,'-',''),1,9)")
                .fetch_one(&owner)
                .await
                .unwrap();
        let boards = [format!("a{seed}"), format!("b{seed}")];
        for board in &boards {
            sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Owned cooldown','Synthetic fixture',2000,100,100,100,10)").bind(board).execute(&owner).await.unwrap();
        }
        let actor = sqlx::query_scalar("SELECT sha256(convert_to(gen_random_uuid()::text,'UTF8'))")
            .fetch_one(&owner)
            .await
            .unwrap();
        Self {
            owner,
            public,
            boards,
            actor,
        }
    }

    async fn now(&self) -> i64 {
        sqlx::query_scalar("SELECT floor(extract(epoch FROM clock_timestamp()))::bigint")
            .fetch_one(&self.owner)
            .await
            .unwrap()
    }

    // Register controlled-clock content through the actual runtime INSERT
    // trigger, holding the same actor gate and transaction-local context.
    async fn seed(&self, board: usize, parent: i64, epoch: i64) -> i64 {
        let mut tx = self.public.begin().await.unwrap();
        lock(&mut tx, &self.actor, parent == 0).await.unwrap();
        set_actor(&mut tx, &self.actor).await.unwrap();
        let id: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        let thread = if parent == 0 { id } else { parent };
        if parent == 0 {
            sqlx::query("INSERT INTO content.threads(id,board) VALUES($1,$2)")
                .bind(id)
                .bind(&self.boards[board])
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at) VALUES($1,$2,$3,'Anonymous','Owned cooldown','Owned fixture',to_timestamp($4::double precision))")
            .bind(id).bind(&self.boards[board]).bind(thread).bind(epoch).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        id
    }

    async fn check(
        &self,
        actor: &[u8],
        board: usize,
        parent: i64,
        image: bool,
        epoch: i64,
    ) -> Option<(String, i64)> {
        let mut tx = self.public.begin().await.unwrap();
        lock(&mut tx, actor, parent == 0).await.unwrap();
        let result = sqlx::query_as(
            "SELECT kind,remaining_seconds FROM content.check_posting_cooldown($1,$2,$3,$4,$5)",
        )
        .bind(actor)
        .bind(&self.boards[board])
        .bind(parent)
        .bind(image)
        .bind(epoch)
        .fetch_optional(&mut *tx)
        .await
        .unwrap();
        tx.rollback().await.unwrap();
        result
    }

    async fn cleanup(self) {
        sqlx::query("DELETE FROM content.post_media WHERE post_id IN(SELECT id FROM content.posts WHERE board=ANY($1))").bind(&self.boards[..]).execute(&self.owner).await.unwrap();
        sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=ANY($1))").bind(&self.boards[..]).execute(&self.owner).await.unwrap();
        sqlx::query("DELETE FROM content.posts WHERE board=ANY($1)")
            .bind(&self.boards[..])
            .execute(&self.owner)
            .await
            .unwrap();
        sqlx::query("DELETE FROM content.threads WHERE board=ANY($1)")
            .bind(&self.boards[..])
            .execute(&self.owner)
            .await
            .unwrap();
        sqlx::query("DELETE FROM post_secrets.posting_thread_actions WHERE board=ANY($1)")
            .bind(&self.boards[..])
            .execute(&self.owner)
            .await
            .unwrap();
        // R9K fixtures use the same board ownership and cascade on board removal.
        sqlx::query("DELETE FROM post_secrets.robot9000_texts WHERE board=ANY($1)")
            .bind(&self.boards[..])
            .execute(&self.owner)
            .await
            .unwrap();
        sqlx::query("DELETE FROM post_secrets.robot9000_mutes WHERE board=ANY($1)")
            .bind(&self.boards[..])
            .execute(&self.owner)
            .await
            .unwrap();
        sqlx::query("DELETE FROM content.boards WHERE slug=ANY($1)")
            .bind(&self.boards[..])
            .execute(&self.owner)
            .await
            .unwrap();
    }
}

async fn lock(
    tx: &mut Transaction<'_, Postgres>,
    actor: &[u8],
    op: bool,
) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT content.lock_posting_actor($1,$2)")
        .bind(actor)
        .bind(op)
        .execute(&mut **tx)
        .await
        .map(|_| ())
}

async fn set_actor(tx: &mut Transaction<'_, Postgres>, actor: &[u8]) -> Result<(), sqlx::Error> {
    sqlx::query("SELECT set_config('board.posting_actor',encode($1::bytea,'hex'),true)")
        .bind(actor)
        .execute(&mut **tx)
        .await
        .map(|_| ())
}

fn code(error: &sqlx::Error) -> String {
    error
        .as_database_error()
        .unwrap()
        .code()
        .unwrap()
        .into_owned()
}

#[tokio::test]
async fn strict_edges_incoming_image_latest_reply_id_and_sticky_op() {
    let _guard = TEST.lock().await;
    let f = Fixture::new().await;
    let now = f.now().await;
    let op = f.seed(0, 0, now).await;
    assert_eq!(
        f.check(&f.actor, 0, op, false, now).await,
        None,
        "An OP must not start the reply timer"
    );
    sqlx::query("UPDATE content.threads SET sticky=true WHERE id=$1")
        .bind(op)
        .execute(&f.owner)
        .await
        .unwrap();
    // Unlike reply lookup, the OP timer is the latest surviving timestamp.
    let older_op = f.seed(0, 0, now - 100).await;
    assert!(older_op > op);
    assert_eq!(
        f.check(&f.actor, 0, 0, false, now + 599).await,
        Some(("thread".into(), 1))
    );
    assert_eq!(
        f.check(&f.actor, 0, 0, false, now + 600).await,
        None,
        "Same-board thread edge is strict, including sticky OPs"
    );
    sqlx::query("UPDATE content.boards SET posting_image_seconds=120 WHERE slug=$1")
        .bind(&f.boards[0])
        .execute(&f.owner)
        .await
        .unwrap();
    let reply = f.seed(0, op, now - 59).await;
    assert_eq!(
        f.check(&f.actor, 0, op, false, now).await,
        Some(("reply".into(), 1))
    );
    assert_eq!(f.check(&f.actor, 0, op, false, now + 1).await, None);
    assert_eq!(
        f.check(&f.actor, 0, op, true, now).await,
        Some(("image".into(), 61)),
        "Incoming image chooses image delay even after a text-only post"
    );
    assert_eq!(f.check(&f.actor, 0, op, true, now + 61).await, None);
    // The prior post may have an image; choosing the next delay still depends
    // only on the incoming post. File-only removal retains its actor identity.
    sqlx::query("INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler) VALUES($1,replace(gen_random_uuid()::text,'-',''),replace(gen_random_uuid()::text,'-',''),'owned.png',1,1,1,false)").bind(reply).execute(&f.owner).await.unwrap();
    assert_eq!(
        f.check(&f.actor, 0, op, false, now).await,
        Some(("reply".into(), 1))
    );
    board_store::post_media::delete_attachment(&f.public, &f.boards[0], reply)
        .await
        .unwrap();
    let retained: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM post_secrets.posting_history WHERE post_id=$1)",
    )
    .bind(reply)
    .fetch_one(&f.owner)
    .await
    .unwrap();
    assert!(retained, "File-only deletion retains posting identity");
    let newest_id = f.seed(0, op, now - 200).await;
    assert!(newest_id > reply);
    assert_eq!(
        f.check(&f.actor, 0, op, false, now).await,
        None,
        "Latest post ID, not maximum timestamp, chooses prior reply"
    );
    sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1")
        .bind(newest_id)
        .execute(&f.public)
        .await
        .unwrap();
    assert_eq!(
        f.check(&f.actor, 0, op, false, now).await,
        Some(("reply".into(), 1)),
        "Deleted latest reply falls back to surviving identity"
    );
    sqlx::query("UPDATE content.posts SET deleted=false WHERE id=$1")
        .bind(newest_id)
        .execute(&f.owner)
        .await
        .unwrap();
    assert_eq!(
        f.check(&f.actor, 0, op, false, now).await,
        Some(("reply".into(), 1)),
        "Undeletion must not reconstruct private identity"
    );
    let mut other = f.actor.clone();
    other[31] ^= 1;
    assert_eq!(
        f.check(&other, 0, op, false, now).await,
        None,
        "Full actor hash differs even when the lock stripe is identical"
    );
    f.cleanup().await;
}

#[tokio::test]
async fn cross_board_inclusive_edge_uses_database_clock_and_actions_survive_lifecycle() {
    let _guard = TEST.lock().await;
    let f = Fixture::new().await;
    let now = f.now().await;
    let op = f.seed(0, 0, now).await;
    let reply = f.seed(0, op, now).await;
    // Archive expires in the future; history disappears for both OP and reply.
    sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1").bind(op).execute(&f.owner).await.unwrap();
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM post_secrets.posting_history WHERE post_id=ANY($1)",
    )
    .bind(vec![op, reply])
    .fetch_one(&f.owner)
    .await
    .unwrap();
    assert_eq!(count, 0);
    assert_eq!(
        f.check(&f.actor, 0, 0, false, now).await,
        None,
        "Archive clears same-board identity"
    );
    assert_eq!(
        f.check(&f.actor, 1, 0, false, now + 86400).await.unwrap().0,
        "cross_board_thread",
        "Cross-board action survives archive and ignores future request timestamp"
    );
    sqlx::query("UPDATE content.threads SET deleted=true,archived_at=NULL,archive_expires_at=NULL WHERE id=$1").bind(op).execute(&f.owner).await.unwrap();
    assert_eq!(
        f.check(&f.actor, 1, 0, false, now).await.unwrap().0,
        "cross_board_thread"
    );
    // Validate the inclusive 300-second edge without assuming a round trip
    // cannot cross a clock tick. Retry only if the observed DB second changed.
    for age in [300_i64, 301] {
        let mut sampled = false;
        for _ in 0..32 {
            let before = f.now().await;
            sqlx::query(
                "UPDATE post_secrets.posting_thread_actions SET request_at=$2 WHERE actor_hash=$1",
            )
            .bind(&f.actor)
            .bind(before - age)
            .execute(&f.owner)
            .await
            .unwrap();
            let result = f.check(&f.actor, 1, 0, false, 0).await;
            if f.now().await == before {
                assert_eq!(
                    result,
                    if age == 300 {
                        Some(("cross_board_thread".into(), 1))
                    } else {
                        None
                    }
                );
                sampled = true;
                break;
            }
        }
        assert!(sampled, "Could not sample within a stable server second");
    }
    f.cleanup().await;
}

async fn create(
    f: &Fixture,
    board: usize,
    parent: i64,
    peer: IpAddr,
    comment: &str,
) -> Result<i64, StoreError> {
    let key = PosterIdKey::parse(&"34".repeat(32)).unwrap();
    board_store::create_post_with_metadata(
        &f.public,
        &f.boards[board],
        parent,
        &NewPost {
            name: "Anonymous".into(),
            subject: "Owned cooldown".into(),
            comment: comment.into(),
            deletion_hash: "synthetic-not-a-password".into(),
            sage: false,
        },
        None,
        PostingContext {
            request_start: chrono::Utc::now(),
            peer: Some(peer),
            op_password_proof: None,
        },
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

fn peer(f: &Fixture) -> IpAddr {
    let bytes: [u8; 16] = f.actor[..16].try_into().unwrap();
    IpAddr::V6(Ipv6Addr::from(bytes))
}

#[tokio::test]
async fn concurrent_cross_board_ops_accept_exactly_one_and_rejected_r9k_records_nothing() {
    let _guard = TEST.lock().await;
    let f = Fixture::new().await;
    let ip = peer(&f);
    let (one, two) = tokio::join!(
        create(&f, 0, 0, ip, "unique first board"),
        create(&f, 1, 0, ip, "unique second board")
    );
    let (winner, post) = match (one, two) {
        (Ok(id), Err(StoreError::PostingCooldownRejected(r))) => {
            assert_eq!(
                r.reason,
                board_store::PostingCooldownReason::CrossBoardThread
            );
            (0, id)
        }
        (Err(StoreError::PostingCooldownRejected(r)), Ok(id)) => {
            assert_eq!(
                r.reason,
                board_store::PostingCooldownReason::CrossBoardThread
            );
            (1, id)
        }
        other => panic!("Exactly one same-actor cross-board OP should commit: {other:?}"),
    };
    let counts: (i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM post_secrets.posting_history WHERE board=ANY($1)),(SELECT count(*) FROM post_secrets.posting_thread_actions WHERE board=ANY($1))").bind(&f.boards[..]).fetch_one(&f.owner).await.unwrap();
    assert_eq!(counts, (1, 1));
    sqlx::query("UPDATE content.boards SET robot9000=true,posting_reply_seconds=0,posting_thread_seconds=0 WHERE slug=$1").bind(&f.boards[winner]).execute(&f.owner).await.unwrap();
    create(&f, winner, post, ip, "a sufficiently original reply")
        .await
        .unwrap();
    let before: (i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM post_secrets.posting_history WHERE board=ANY($1)),(SELECT count(*) FROM post_secrets.posting_thread_actions WHERE board=ANY($1))").bind(&f.boards[..]).fetch_one(&f.owner).await.unwrap();
    sqlx::query("UPDATE post_secrets.posting_thread_actions SET request_at=request_at-600 WHERE board=ANY($1)").bind(&f.boards[..]).execute(&f.owner).await.unwrap();
    let actions_before:Vec<serde_json::Value>=sqlx::query_scalar("SELECT to_jsonb(a) FROM post_secrets.posting_thread_actions a WHERE board=ANY($1) ORDER BY board").bind(&f.boards[..]).fetch_all(&f.owner).await.unwrap();
    assert!(matches!(
        create(&f, winner, 0, ip, "A SUFFICIENTLY ORIGINAL REPLY!!!").await,
        Err(StoreError::Robot9000Rejected(_))
    ));
    let actions_after:Vec<serde_json::Value>=sqlx::query_scalar("SELECT to_jsonb(a) FROM post_secrets.posting_thread_actions a WHERE board=ANY($1) ORDER BY board").bind(&f.boards[..]).fetch_all(&f.owner).await.unwrap();
    assert_eq!(
        actions_before, actions_after,
        "R9K rejection must not refresh an existing successful action"
    );
    let after: (i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM post_secrets.posting_history WHERE board=ANY($1)),(SELECT count(*) FROM post_secrets.posting_thread_actions WHERE board=ANY($1))").bind(&f.boards[..]).fetch_one(&f.owner).await.unwrap();
    assert_eq!(
        before, after,
        "R9K rejection commits its mute but rolls back posting identity/action"
    );
    let mut other = ip;
    if let IpAddr::V6(address) = other {
        let mut bytes = address.octets();
        bytes[15] ^= 1;
        other = Ipv6Addr::from(bytes).into();
    }
    create(&f, 1 - winner, 0, other, "independent full peer identity")
        .await
        .unwrap();
    f.cleanup().await;
}

#[tokio::test]
async fn private_acl_malformed_inputs_and_transaction_rollback() {
    let _guard = TEST.lock().await;
    let f = Fixture::new().await;
    for (table, read_query) in [
        (
            "posting_actor_gates",
            "SELECT * FROM post_secrets.posting_actor_gates",
        ),
        (
            "posting_action_capacity",
            "SELECT * FROM post_secrets.posting_action_capacity",
        ),
        (
            "posting_history",
            "SELECT * FROM post_secrets.posting_history",
        ),
        (
            "posting_thread_actions",
            "SELECT * FROM post_secrets.posting_thread_actions",
        ),
    ] {
        for role in [
            "board_public",
            "board_staff",
            "board_auth",
            "board_media",
            "board_media_read",
            "board_monitor",
            "board_media_intake",
        ] {
            let privilege: bool = sqlx::query_scalar("SELECT has_table_privilege($1,$2,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')").bind(role).bind(format!("post_secrets.{table}")).fetch_one(&f.owner).await.unwrap();
            assert!(!privilege, "{role} may access private {table}");
        }
        let error = sqlx::query(read_query)
            .fetch_all(&f.public)
            .await
            .unwrap_err();
        assert_eq!(code(&error), "42501");
    }
    for signature in [
        "content.lock_posting_actor(bytea,boolean)",
        "content.check_posting_cooldown(bytea,text,bigint,boolean,bigint)",
    ] {
        let secure: bool = sqlx::query_scalar("SELECT p.prosecdef AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp'] AND r.rolname='board_posting_cooldown_owner' AND NOT (r.rolcanlogin OR r.rolsuper OR r.rolcreatedb OR r.rolcreaterole OR r.rolreplication OR r.rolbypassrls) AND has_function_privilege('board_public',p.oid,'EXECUTE') AND NOT EXISTS(SELECT 1 FROM aclexplode(p.proacl) a WHERE a.grantee=0 AND a.privilege_type='EXECUTE') FROM pg_proc p JOIN pg_roles r ON r.oid=p.proowner WHERE p.oid=$1::regprocedure").bind(signature).fetch_one(&f.owner).await.unwrap();
        assert!(secure, "Unsafe capability {signature}");
    }
    let private_trigger: bool = sqlx::query_scalar("SELECT p.prosecdef AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp'] AND NOT has_function_privilege('board_public',p.oid,'EXECUTE') AND NOT has_function_privilege('board_staff',p.oid,'EXECUTE') AND NOT EXISTS(SELECT 1 FROM aclexplode(p.proacl) a WHERE a.grantee=0 AND a.privilege_type='EXECUTE') FROM pg_proc p WHERE p.oid='content.record_inserted_posting_history()'::regprocedure").fetch_one(&f.owner).await.unwrap();
    assert!(
        private_trigger,
        "History trigger cannot be invoked as an arbitrary-post registration oracle"
    );
    let removed: bool = sqlx::query_scalar(
        "SELECT to_regprocedure('content.record_posting_history(bytea,bigint)') IS NULL",
    )
    .fetch_one(&f.owner)
    .await
    .unwrap();
    assert!(
        removed,
        "Runtime must not retain arbitrary-post registration API"
    );
    for actor in [
        None,
        Some(vec![]),
        Some(vec![0_u8; 31]),
        Some(vec![0_u8; 33]),
    ] {
        let error = sqlx::query("SELECT content.lock_posting_actor($1,false)")
            .bind(actor.as_deref())
            .execute(&f.public)
            .await
            .unwrap_err();
        assert_eq!(code(&error), "23514");
        let error = sqlx::query("SELECT * FROM content.check_posting_cooldown($1,$2,0,false,1)")
            .bind(actor.as_deref())
            .bind(&f.boards[0])
            .fetch_all(&f.public)
            .await
            .unwrap_err();
        assert_eq!(code(&error), "23514");
    }
    for context in [
        String::new(),
        "invalid".into(),
        "ab".repeat(31),
        "ab".repeat(33),
    ] {
        let mut tx = f.public.begin().await.unwrap();
        sqlx::query("SELECT set_config('board.posting_actor',$1,true)")
            .bind(context)
            .execute(&mut *tx)
            .await
            .unwrap();
        let id: i64 =
            sqlx::query_scalar("INSERT INTO content.threads(board) VALUES($1) RETURNING id")
                .bind(&f.boards[0])
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        let error=sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','Missing context','Owned fixture')").bind(id).bind(&f.boards[0]).execute(&mut *tx).await.unwrap_err();
        assert_eq!(
            code(&error),
            "23514",
            "Missing or malformed runtime identity must fail closed"
        );
        tx.rollback().await.unwrap();
    }
    let mut stale = f.public.begin().await.unwrap();
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *stale)
        .await
        .unwrap();
    lock(&mut stale, &f.actor, true).await.unwrap();
    set_actor(&mut stale, &f.actor).await.unwrap();
    let id: i64 = sqlx::query_scalar("INSERT INTO content.threads(board) VALUES($1) RETURNING id")
        .bind(&f.boards[0])
        .fetch_one(&mut *stale)
        .await
        .unwrap();
    let error=sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','Stale snapshot','Owned fixture')").bind(id).bind(&f.boards[0]).execute(&mut *stale).await.unwrap_err();
    assert_eq!(
        code(&error),
        "22023",
        "Registration rejects an isolation level that could hide capacity changes"
    );
    stale.rollback().await.unwrap();
    let mut tx = f.public.begin().await.unwrap();
    lock(&mut tx, &f.actor, true).await.unwrap();
    set_actor(&mut tx, &f.actor).await.unwrap();
    let id: i64 = sqlx::query_scalar("INSERT INTO content.threads(board) VALUES($1) RETURNING id")
        .bind(&f.boards[0])
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','Owned rollback','Owned fixture')").bind(id).bind(&f.boards[0]).execute(&mut *tx).await.unwrap();
    assert!(sqlx::query("SELECT 1/0").execute(&mut *tx).await.is_err());
    tx.rollback().await.unwrap();
    let absent:bool=sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM post_secrets.posting_history WHERE actor_hash=$1) AND NOT EXISTS(SELECT 1 FROM post_secrets.posting_thread_actions WHERE actor_hash=$1) AND NOT EXISTS(SELECT 1 FROM content.posts WHERE id=$2)").bind(&f.actor).bind(id).fetch_one(&f.owner).await.unwrap();
    assert!(
        absent,
        "A failed downstream transaction must leave no post, history or action"
    );
    f.cleanup().await;
}

#[tokio::test]
async fn actor_gate_is_acquired_before_the_board_lock() {
    let _guard = TEST.lock().await;
    let f = Fixture::new().await;
    let mut queued = f.clone();
    queued.public = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&queued.public)
        .await
        .unwrap();
    let ip = peer(&f);
    let actor = PosterIdKey::parse(&"34".repeat(32))
        .unwrap()
        .public_posting_rate_identity(ip);
    let bytes = actor.as_bytes();
    let stripe = (i32::from(bytes[0]) * 256 + i32::from(bytes[1])) % 4096;
    let mut held = f.owner.begin().await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *held)
        .await
        .unwrap();
    sqlx::query("SELECT stripe FROM post_secrets.posting_actor_gates WHERE stripe=$1 FOR UPDATE")
        .bind(stripe)
        .execute(&mut *held)
        .await
        .unwrap();
    // A caller bypassing store lock order must fail promptly in the trigger,
    // rather than taking a late actor lock after its content write.
    direct_insert_is_contended(&f, actor.as_bytes()).await;
    let task =
        tokio::spawn(async move { create(&queued, 0, 0, ip, "a gated original thread").await });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            let blocked: bool = sqlx::query_scalar("SELECT $2=ANY(pg_blocking_pids($1))")
                .bind(pid)
                .bind(blocker)
                .fetch_one(&f.owner)
                .await
                .unwrap();
            if blocked {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("Writer must wait for the actor gate");
    // NOWAIT would fail if the queued writer had locked its board first.
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE NOWAIT")
        .bind(&f.boards[0])
        .execute(&mut *held)
        .await
        .expect("Actor gate must precede the board row lock");
    held.commit().await.unwrap();
    task.await.unwrap().unwrap();
    let mut capacity = f.owner.begin().await.unwrap();
    sqlx::query(
        "SELECT singleton FROM post_secrets.posting_action_capacity WHERE singleton FOR UPDATE",
    )
    .execute(&mut *capacity)
    .await
    .unwrap();
    direct_insert_is_contended(&f, &f.actor).await;
    capacity.rollback().await.unwrap();
    f.cleanup().await;
}

async fn direct_insert_is_contended(f: &Fixture, actor: &[u8]) {
    let mut tx = f.public.begin().await.unwrap();
    set_actor(&mut tx, actor).await.unwrap();
    let id: i64 = sqlx::query_scalar("INSERT INTO content.threads(board) VALUES($1) RETURNING id")
        .bind(&f.boards[1])
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let result=tokio::time::timeout(std::time::Duration::from_secs(3),
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','Late gate','Owned fixture')")
            .bind(id).bind(&f.boards[1]).execute(&mut *tx)
    ).await.expect("Late trigger lock must reject immediately instead of waiting");
    assert_eq!(code(&result.unwrap_err()), "55P03");
    tx.rollback().await.unwrap();
}

// Migrator-owned synthetic populations are rollback-only. Supplying explicit
// actor context exercises the same INSERT trigger; context-free imports skip it.
async fn insert_as_owner(
    tx: &mut Transaction<'_, Postgres>,
    actor: &[u8],
    post: i64,
    board: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query("SAVEPOINT capacity_attempt")
        .execute(&mut **tx)
        .await?;
    let result = async {
        sqlx::query("SET LOCAL ROLE board_posting_cooldown_owner").execute(&mut **tx).await?;
        lock(tx, actor, true).await?;
        sqlx::query("RESET ROLE").execute(&mut **tx).await?;
        set_actor(tx, actor).await?;
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','Owned capacity attempt','Owned fixture')")
            .bind(post).bind(board).execute(&mut **tx).await?;
        Ok(())
    }.await;
    if result.is_err() {
        sqlx::query("ROLLBACK TO SAVEPOINT capacity_attempt")
            .execute(&mut **tx)
            .await?;
    }
    sqlx::query("RELEASE SAVEPOINT capacity_attempt")
        .execute(&mut **tx)
        .await?;
    result
}

#[tokio::test]
async fn bounded_action_capacity_preserves_live_entries_and_fails_closed() {
    let _guard = TEST.lock().await;
    let f = Fixture::new().await;
    let op = f.seed(0, 0, f.now().await).await;
    let mut tx = f.owner.begin().await.unwrap();
    sqlx::query("DELETE FROM content.posts WHERE id=$1")
        .bind(op)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM post_secrets.posting_thread_actions")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO post_secrets.posting_thread_actions(actor_hash,board,request_at) SELECT sha256(convert_to('owned-posting-capacity-'||n::text,'UTF8')),$1,floor(extract(epoch FROM clock_timestamp()))::bigint FROM generate_series(1,100000) n").bind(&f.boards[0]).execute(&mut *tx).await.unwrap();
    assert_eq!(
        code(
            &insert_as_owner(&mut tx, &f.actor, op, &f.boards[0])
                .await
                .unwrap_err()
        ),
        "P0087"
    );
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM post_secrets.posting_thread_actions")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    assert_eq!(
        count, 100000,
        "Capacity rejection never evicts live actions"
    );
    let absent: bool = sqlx::query_scalar(
        "SELECT NOT EXISTS(SELECT 1 FROM post_secrets.posting_history WHERE post_id=$1)",
    )
    .bind(op)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert!(absent, "Failed action reservation rolls back history too");
    sqlx::query("UPDATE post_secrets.posting_thread_actions SET request_at=0 WHERE actor_hash IN(SELECT sha256(convert_to('owned-posting-capacity-'||n::text,'UTF8')) FROM generate_series(1,65) n)").execute(&mut *tx).await.unwrap();
    insert_as_owner(&mut tx, &f.actor, op, &f.boards[0])
        .await
        .unwrap();
    let counts:(i64,i64)=sqlx::query_as("SELECT count(*),count(*) FILTER(WHERE request_at=0) FROM post_secrets.posting_thread_actions").fetch_one(&mut *tx).await.unwrap();
    assert_eq!(
        counts,
        (99937, 1),
        "A reservation cleans at most 64 expired action rows"
    );
    tx.rollback().await.unwrap();
    for (missing, delete_query) in [
        (
            "posting_action_capacity",
            "DELETE FROM post_secrets.posting_action_capacity",
        ),
        (
            "posting_actor_gates",
            "DELETE FROM post_secrets.posting_actor_gates",
        ),
    ] {
        let mut tx = f.owner.begin().await.unwrap();
        sqlx::query(delete_query).execute(&mut *tx).await.unwrap();
        sqlx::query("DELETE FROM content.posts WHERE id=$1")
            .bind(op)
            .execute(&mut *tx)
            .await
            .unwrap();
        assert_eq!(
            code(
                &insert_as_owner(&mut tx, &f.actor, op, &f.boards[0])
                    .await
                    .unwrap_err()
            ),
            "P0087",
            "Missing {missing} must fail closed"
        );
        tx.rollback().await.unwrap();
    }
    f.cleanup().await;
}

#[tokio::test]
async fn bounded_history_capacity_and_private_board_nonexposure() {
    let _guard = TEST.lock().await;
    let f = Fixture::new().await;
    let op = f.seed(0, 0, f.now().await).await;
    let mut tx = f.owner.begin().await.unwrap();
    sqlx::query("DELETE FROM content.posts WHERE id=$1")
        .bind(op)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("WITH seeded AS (INSERT INTO content.posts(board,thread_id,name,subject,comment) SELECT $1,$2,'Anonymous','Owned capacity','Owned capacity fixture' FROM generate_series(1,100000) RETURNING id,board,thread_id) INSERT INTO post_secrets.posting_history(post_id,board,thread_id,actor_hash,request_at) SELECT id,board,thread_id,$3,1 FROM seeded").bind(&f.boards[0]).bind(op).bind(&f.actor).execute(&mut *tx).await.unwrap();
    assert_eq!(
        code(
            &insert_as_owner(&mut tx, &f.actor, op, &f.boards[0])
                .await
                .unwrap_err()
        ),
        "P0087"
    );
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM post_secrets.posting_history WHERE board=$1")
            .bind(&f.boards[0])
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(
        count, 100000,
        "Surviving old identity cannot be evicted by age to admit a post"
    );
    tx.rollback().await.unwrap();
    sqlx::query("UPDATE content.boards SET staff_only=true WHERE slug=$1")
        .bind(&f.boards[0])
        .execute(&f.owner)
        .await
        .unwrap();
    let hidden = sqlx::query("SELECT * FROM content.check_posting_cooldown($1,$2,0,false,1)")
        .bind(&f.actor)
        .bind(&f.boards[0])
        .fetch_all(&f.public)
        .await
        .unwrap_err();
    let absent = sqlx::query("SELECT * FROM content.check_posting_cooldown($1,'',0,false,1)")
        .bind(&f.actor)
        .fetch_all(&f.public)
        .await
        .unwrap_err();
    assert_eq!(code(&hidden), "P0002");
    assert_eq!(
        hidden.as_database_error().unwrap().message(),
        absent.as_database_error().unwrap().message()
    );
    let hidden: bool =
        sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM content.posts WHERE id=$1)")
            .bind(op)
            .fetch_one(&f.public)
            .await
            .unwrap();
    assert!(
        hidden,
        "Public runtime cannot see the private post used by the fixture"
    );
    let denied = sqlx::query("UPDATE content.boards SET posting_reply_seconds=0 WHERE slug=$1")
        .bind(&f.boards[1])
        .execute(&f.public)
        .await
        .unwrap_err();
    assert_eq!(code(&denied), "42501");
    f.cleanup().await;
}
