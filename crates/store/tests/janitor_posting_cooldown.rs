#![cfg(feature = "database-tests")]

use board_domain::poster_id::PosterIdKey;
use board_store::{
    NewPost, PostIdentityKeys, PostMetadata, PostingContext, PostingCooldownReason,
    StaffPostAuthority, StaffPostIdentity, StoreError,
};
use sqlx::{PgPool, Postgres, Transaction};
use std::net::IpAddr;

// Text-only reading: imgboard.php:5862-5866,5887-5958,6004-6016.
// Named/meta janitors run ordinary timers first, ceil-half only for replies/images.
// These fixtures make no Pass, duplicate-content, upload, or CAPTCHA claims.
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

    async fn role(&self, role: &str) {
        sqlx::query("UPDATE staff_identity.accounts SET role=$2 WHERE id=$1")
            .bind(self.account)
            .bind(role)
            .execute(&self.owner)
            .await
            .unwrap();
    }

    async fn meta(&self, board: usize, meta: bool) {
        sqlx::query("UPDATE content.boards SET meta_board=$2 WHERE slug=$1")
            .bind(&self.boards[board])
            .bind(meta)
            .execute(&self.owner)
            .await
            .unwrap();
    }

    async fn delay(&self) -> i64 {
        let value: i32 =
            sqlx::query_scalar("SELECT posting_reply_seconds FROM content.boards WHERE slug=$1")
                .bind(&self.boards[0])
                .fetch_one(&self.owner)
                .await
                .unwrap();
        i64::from(value)
    }

    async fn write(
        &self,
        board: usize,
        parent: i64,
        peer: IpAddr,
        epoch: i64,
        raw_name: &str,
        authorized: bool,
    ) -> Result<i64, StoreError> {
        let key = PosterIdKey::parse(&self.key).unwrap();
        let ticket: Vec<u8> =
            sqlx::query_scalar("SELECT sha256(convert_to(gen_random_uuid()::text,'UTF8'))")
                .fetch_one(&self.owner)
                .await
                .unwrap();
        let ticket: [u8; 32] = ticket.try_into().unwrap();
        let authority = StaffPostAuthority {
            auth_pool: &self.auth,
            session_hash: &self.session,
            csrf_hash: &self.csrf,
            ticket_hash: &ticket,
            idle_seconds: 900,
            highlight: false,
            authorized_limits: authorized,
            raw_name_nonempty: !raw_name.is_empty(),
            identity: Some(StaffPostIdentity {
                capcode: None,
                name_allowed: true,
                administrator: false,
                tripcode_key: None,
            }),
        };
        let mut input = post();
        input.name = raw_name.into();
        board_store::create_ordinary_staff_post(
            &self.staff,
            &self.boards[board],
            parent,
            &input,
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
            authority,
        )
        .await
    }

    async fn check(
        &self,
        board: usize,
        parent: i64,
        peer: IpAddr,
        epoch: i64,
        image: bool,
        janitor: bool,
    ) -> Option<(String, i64)> {
        let mut tx = if janitor {
            self.staff.begin().await
        } else {
            self.public.begin().await
        }
        .unwrap();
        prepare(&mut tx, &self.actor(peer), parent == 0).await;
        let sql = if janitor {
            "SELECT kind,remaining_seconds FROM content.check_janitor_posting_cooldown($1,$2,$3,$4,$5)"
        } else {
            "SELECT kind,remaining_seconds FROM content.check_posting_cooldown($1,$2,$3,$4,$5)"
        };
        let result = sqlx::query_as(sql)
            .bind(self.actor(peer).as_slice())
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

    async fn no_proofs(&self) {
        let count: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM post_secrets.staff_post_intents WHERE account_id=$1",
        )
        .bind(self.account)
        .fetch_one(&self.owner)
        .await
        .unwrap();
        assert_eq!(count, 0, "Rejected/consumed proofs must be cleaned up");
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
async fn janitor_raw_name_meta_matrix_and_literal_nonempty_names() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    f.role("janitor").await;
    let now = f.now().await;
    let half = (f.delay().await + 1) / 2;
    assert!(
        half > 5,
        "Imported policy must distinguish ordinary and staff gates"
    );
    let mut n = 1;
    for meta in [false, true] {
        f.meta(0, meta).await;
        for raw in ["", "Named", "   ", "#trip"] {
            let ip = peer(n);
            n += 1;
            let op = f.seed(0, 0, ip, now - 1000).await;
            f.seed(0, op, ip, now).await;
            let before = f.snapshot().await;
            if meta || !raw.is_empty() {
                rejected(f.write(0, op, ip, now + 5, raw, false).await, half - 5);
                assert_eq!(f.snapshot().await, before);
                f.no_proofs().await;
                f.write(0, op, ip, now + half, raw, false).await.unwrap();
            } else {
                f.write(0, op, ip, now + 5, raw, false).await.unwrap();
            }
            f.no_proofs().await;
        }
    }
    f.cleanup().await;
}

#[tokio::test]
async fn moderator_and_higher_ignore_named_meta_ordinary_gate() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let now = f.now().await;
    let mut n = 20;
    for role in ["moderator", "manager", "admin"] {
        f.role(role).await;
        for meta in [false, true] {
            f.meta(0, meta).await;
            for raw in ["", "Named"] {
                let ip = peer(n);
                n += 1;
                let op = f.seed(0, 0, ip, now - 1000).await;
                f.seed(0, op, ip, now).await;
                rejected(f.write(0, op, ip, now + 4, raw, true).await, 1);
                f.write(0, op, ip, now + 5, raw, true).await.unwrap();
            }
        }
    }
    f.cleanup().await;
}

#[tokio::test]
async fn ceil_half_reply_and_image_zero_one_odd_even_without_upload_api() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let now = f.now().await;
    let op = f.seed(0, 0, peer(40), now - 1000).await;
    f.seed(0, op, peer(40), now).await;
    // Only this owned synthetic board uses a parameter matrix. Imported board
    // policy is untouched, and every assertion calls the real SQL gate.
    for delay in [0_i32, 1, 3, 4, 15, 16] {
        sqlx::query("UPDATE content.boards SET posting_reply_seconds=$2,posting_image_seconds=$2 WHERE slug=$1")
            .bind(&f.boards[0]).bind(delay).execute(&f.owner).await.unwrap();
        let half = i64::from((delay + 1) / 2);
        for image in [false, true] {
            let kind = if image { "image" } else { "reply" };
            assert_eq!(
                f.check(0, op, peer(40), now + half, image, true).await,
                None
            );
            if half > 0 {
                assert_eq!(
                    f.check(0, op, peer(40), now + half - 1, image, true).await,
                    Some((kind.into(), 1))
                );
            }
            assert_eq!(
                f.check(0, op, peer(40), now, image, false).await,
                (delay > 0).then(|| (kind.into(), i64::from(delay))),
                "Public caller must retain full delay"
            );
        }
    }
    f.cleanup().await;
}

#[tokio::test]
async fn ordinary_error_precedes_five_seconds_and_shared_history_rolls_back() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    f.role("janitor").await;
    let now = f.now().await;
    let delay = f.delay().await;
    let half = (delay + 1) / 2;
    let ip = peer(50);
    let op = f.public_write(0, 0, ip, now - 1000).await.unwrap();
    f.public_write(0, op, ip, now).await.unwrap();
    let before = f.snapshot().await;
    rejected(f.write(0, op, ip, now + 4, "Named", false).await, half - 4);
    assert_eq!(f.snapshot().await, before);
    f.no_proofs().await;
    let janitor = f
        .write(0, op, ip, now + half, "Named", false)
        .await
        .unwrap();
    let same: bool = sqlx::query_scalar(
        "SELECT actor_hash=$2 FROM post_secrets.posting_history WHERE post_id=$1",
    )
    .bind(janitor)
    .bind(f.actor(ip).as_slice())
    .fetch_one(&f.owner)
    .await
    .unwrap();
    assert!(same);
    rejected(f.public_write(0, op, ip, now + half + 5).await, delay - 5);
    // Ordinary timer can already be satisfied while the newest OP still trips
    // the staff timer. Neither gate replaces the other.
    f.seed(0, 0, ip, now + 2 * half).await;
    rejected(
        f.write(0, op, ip, now + 2 * half + 4, "Named", false).await,
        1,
    );
    f.no_proofs().await;
    f.cleanup().await;
}

#[tokio::test]
async fn op_delay_is_full_and_cross_board_uses_database_clock() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    f.role("janitor").await;
    let now = f.now().await;
    let delay: i32 =
        sqlx::query_scalar("SELECT posting_thread_seconds FROM content.boards WHERE slug=$1")
            .bind(&f.boards[0])
            .fetch_one(&f.owner)
            .await
            .unwrap();
    assert!(delay >= 5);
    f.seed(0, 0, peer(60), now).await;
    assert_eq!(
        f.check(0, 0, peer(60), now + i64::from(delay) - 1, false, true)
            .await,
        Some(("thread".into(), 1))
    );
    assert_eq!(
        f.check(0, 0, peer(60), now + i64::from(delay), false, true)
            .await,
        None
    );
    match f
        .write(0, 0, peer(60), now + i64::from(delay) - 1, "Named", false)
        .await
    {
        Err(StoreError::PostingCooldownRejected(r)) => {
            assert_eq!(r.reason, PostingCooldownReason::Thread);
            assert_eq!(r.remaining_seconds, 1);
        }
        other => panic!("Expected full ordinary OP delay: {other:?}"),
    }
    f.no_proofs().await;
    let cross = f
        .check(1, 0, peer(60), now + 100_000, false, true)
        .await
        .unwrap();
    assert_eq!(cross.0, "cross_board_thread");
    assert!((1..=301).contains(&cross.1));
    // Boundary is tied to the DB clock. Retry only if its integer second changed
    // across the query, rather than accepting an ambiguous boundary observation.
    for age in [300_i64, 301] {
        loop {
            let stamp = f.now().await;
            sqlx::query("UPDATE post_secrets.posting_thread_actions SET request_at=$3 WHERE actor_hash=$1 AND board=$2")
                .bind(f.actor(peer(60)).as_slice()).bind(&f.boards[0]).bind(stamp - age)
                .execute(&f.owner).await.unwrap();
            let result = f.check(1, 0, peer(60), now + 100_000, false, true).await;
            if f.now().await == stamp {
                assert_eq!(
                    result,
                    (age == 300).then(|| ("cross_board_thread".into(), 1))
                );
                break;
            }
        }
    }
    f.cleanup().await;
}

#[derive(Clone)]
struct Proof {
    ticket: Vec<u8>,
    id: i64,
    board: String,
    stamp: chrono::DateTime<chrono::Utc>,
    bound: serde_json::Value,
    ordinary: bool,
}

async fn issue(f: &Fixture, ordinary: bool, authorized: bool, raw: bool, private: bool) -> Proof {
    let ticket: Vec<u8> =
        sqlx::query_scalar("SELECT sha256(convert_to(gen_random_uuid()::text,'UTF8'))")
            .fetch_one(&f.owner)
            .await
            .unwrap();
    let id: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&f.owner)
        .await
        .unwrap();
    let board = if private {
        "j".into()
    } else {
        f.boards[0].clone()
    };
    let stamp = chrono::DateTime::from_timestamp(f.now().await, 0).unwrap();
    let (limit, user_ids, op_markup, meta): (i32, bool, bool, bool) = sqlx::query_as("SELECT CASE WHEN $2 THEN max_authorized_comment_chars ELSE max_comment_chars END,user_ids,op_markup,meta_board FROM content.boards WHERE slug=$1")
        .bind(&board).bind(authorized).fetch_one(&f.owner).await.unwrap();
    let key = PosterIdKey::parse(&f.key).unwrap();
    let ip = peer(90);
    let count = key.count_context(&board, id, ip).unwrap();
    let bound = serde_json::json!({
        "poster_id": if user_ids { key.label(&board, id, ip).unwrap() } else { String::new() },
        "poster_fingerprint": count.fingerprint, "poster_epoch": count.epoch,
        "post_sage": "false", "country": "", "country_name": "", "flag": "",
        "source_op_reply": if op_markup { "true" } else { "false" },
        "dice_result": "", "fortune_text": "", "fortune_color": "",
        "peer": ip.to_string(), "deletion_hash": "synthetic-not-a-password", "op_password_proof": ""
    });
    let ordinary_timers: bool = if ordinary {
        sqlx::query_scalar("SELECT staff_identity.issue_ordinary_post_authority($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20)")
            .bind(&ticket).bind(&f.session).bind(&f.csrf).bind(900_i32).bind(id).bind(&board).bind(id)
            .bind("Anonymous").bind("Owned proof").bind("Owned proof context fixture").bind(stamp)
            .bind(authorized).bind(limit).bind(None::<Vec<u8>>).bind(None::<String>)
            .bind("").bind(None::<String>).bind(true).bind(&bound).bind(raw)
            .fetch_one(&f.auth).await.unwrap()
    } else {
        sqlx::query_scalar("SELECT staff_identity.issue_limited_post_authority($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17)")
            .bind(&ticket).bind(&f.session).bind(&f.csrf).bind(900_i32).bind(false).bind(id).bind(&board).bind(id)
            .bind("Anonymous").bind("Owned proof").bind("Owned proof context fixture").bind(stamp)
            .bind(authorized).bind(limit).bind(None::<Vec<u8>>).bind(None::<String>).bind(raw)
            .fetch_one(&f.auth).await.unwrap()
    };
    let is_janitor: bool =
        sqlx::query_scalar("SELECT role='janitor' FROM staff_identity.accounts WHERE id=$1")
            .bind(f.account)
            .fetch_one(&f.owner)
            .await
            .unwrap();
    assert_eq!(ordinary_timers, is_janitor && (raw || meta));
    let saved: (bool, bool, bool) = sqlx::query_as("SELECT raw_name_nonempty,is_janitor,meta_board FROM post_secrets.staff_post_intents WHERE token_hash=$1")
        .bind(&ticket).fetch_one(&f.owner).await.unwrap();
    assert_eq!(saved, (raw, is_janitor, meta));
    Proof {
        ticket,
        id,
        board,
        stamp,
        bound,
        ordinary,
    }
}

async fn proof_context(tx: &mut Transaction<'_, Postgres>, p: &Proof, raw: &str) {
    sqlx::query("SELECT set_config('board.staff_raw_name_nonempty',$1,true),set_config('board.post_trip','',true),set_config('board.staff_post_options','',true),set_config('board.wordfilter_payload','',true)")
        .bind(raw).execute(&mut **tx).await.unwrap();
    if p.ordinary {
        for (key, value) in p.bound.as_object().unwrap() {
            sqlx::query("SELECT set_config($1,$2,true)")
                .bind(format!("board.{key}"))
                .bind(value.as_str().unwrap())
                .execute(&mut **tx)
                .await
                .unwrap();
        }
    }
}

async fn consume(
    tx: &mut Transaction<'_, Postgres>,
    p: &Proof,
) -> Result<Option<String>, sqlx::Error> {
    sqlx::query_scalar("SELECT content.consume_staff_post_authority($1,$2,$3,$4,$5,$6,$7,$8)")
        .bind(&p.ticket)
        .bind(p.id)
        .bind(&p.board)
        .bind(p.id)
        .bind("Anonymous")
        .bind("Owned proof")
        .bind("Owned proof context fixture")
        .bind(p.stamp)
        .fetch_one(&mut **tx)
        .await
}

fn denied(error: sqlx::Error) {
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("28000")
    );
}

#[tokio::test]
async fn proof_raw_meta_role_and_pre_migration_context_fail_closed() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    for ordinary in [false, true] {
        for fault in [
            "raw",
            "missing_raw",
            "meta",
            "role",
            "legacy",
            "revoked",
            "scope",
            "expired",
        ] {
            sqlx::query("UPDATE staff_identity.accounts SET revoked_at=NULL,deny_boards=ARRAY[]::text[] WHERE id=$1")
                .bind(f.account).execute(&f.owner).await.unwrap();
            f.role("moderator").await;
            f.meta(0, false).await;
            let p = issue(&f, ordinary, ordinary, true, false).await;
            match fault {
                "meta" => f.meta(0, true).await,
                "role" => f.role("janitor").await,
                "revoked" => {
                    sqlx::query("UPDATE staff_identity.accounts SET revoked_at=clock_timestamp() WHERE id=$1")
                        .bind(f.account).execute(&f.owner).await.unwrap();
                }
                "scope" => {
                    sqlx::query(
                        "UPDATE staff_identity.accounts SET deny_boards=ARRAY[$2] WHERE id=$1",
                    )
                    .bind(f.account)
                    .bind(&p.board)
                    .execute(&f.owner)
                    .await
                    .unwrap();
                }
                "expired" => {
                    sqlx::query("UPDATE post_secrets.staff_post_intents SET expires_at=clock_timestamp()-interval '1 second' WHERE token_hash=$1")
                        .bind(&p.ticket).execute(&f.owner).await.unwrap();
                }
                "legacy" => {
                    sqlx::query("UPDATE post_secrets.staff_post_intents SET raw_name_nonempty=NULL,is_janitor=NULL,meta_board=NULL WHERE token_hash=$1")
                        .bind(&p.ticket).execute(&f.owner).await.unwrap();
                }
                _ => {}
            }
            let before = f.snapshot().await;
            let mut tx = f.staff.begin().await.unwrap();
            proof_context(
                &mut tx,
                &p,
                match fault {
                    "raw" => "false",
                    "missing_raw" => "",
                    _ => "true",
                },
            )
            .await;
            denied(consume(&mut tx, &p).await.unwrap_err());
            tx.rollback().await.unwrap();
            assert_eq!(f.snapshot().await, before);
            sqlx::query("DELETE FROM post_secrets.staff_post_intents WHERE token_hash=$1")
                .bind(&p.ticket)
                .execute(&f.owner)
                .await
                .unwrap();
        }
    }
    f.cleanup().await;
}

#[tokio::test]
async fn unchanged_ordinary_proof_consumes_once_and_cannot_replay() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    for role in ["janitor", "moderator"] {
        f.role(role).await;
        let p = issue(&f, true, role != "janitor", true, false).await;
        let mut tx = f.staff.begin().await.unwrap();
        proof_context(&mut tx, &p, "true").await;
        assert_eq!(consume(&mut tx, &p).await.unwrap(), None);
        tx.commit().await.unwrap();
        f.no_proofs().await;
        let mut replay = f.staff.begin().await.unwrap();
        proof_context(&mut replay, &p, "true").await;
        denied(consume(&mut replay, &p).await.unwrap_err());
        replay.rollback().await.unwrap();
    }
    f.cleanup().await;
}

#[tokio::test]
async fn account_lock_wait_rechecks_role_class_including_legacy_private_limits() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    for private in [false, true] {
        f.role("moderator").await;
        // /j/ uses legacy authorized_limits=false even for a moderator. The
        // new proof binds real role class instead of guessing it from that flag.
        let p = issue(&f, !private, !private, false, private).await;
        let mut unchanged = f.staff.begin().await.unwrap();
        proof_context(&mut unchanged, &p, "false").await;
        assert_eq!(
            consume(&mut unchanged, &p).await.unwrap(),
            None,
            "An unchanged moderator with legacy private authorized_limits=false must remain valid"
        );
        // Roll back consumption (including /j/'s deferred discussion link), so
        // the identical proof is still available for the actual lock race.
        unchanged.rollback().await.unwrap();
        let waiting = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&waiting)
            .await
            .unwrap();
        let mut held = f.owner.begin().await.unwrap();
        sqlx::query("SELECT id FROM staff_identity.accounts WHERE id=$1 FOR UPDATE")
            .bind(f.account)
            .execute(&mut *held)
            .await
            .unwrap();
        let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *held)
            .await
            .unwrap();
        let payload = p.clone();
        let task = tokio::spawn(async move {
            let mut tx = waiting.begin().await.unwrap();
            proof_context(&mut tx, &payload, "false").await;
            let result = consume(&mut tx, &payload).await;
            tx.rollback().await.unwrap();
            result
        });
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
        .expect("Consumer must actually wait for the account row lock");
        sqlx::query("UPDATE staff_identity.accounts SET role='janitor' WHERE id=$1")
            .bind(f.account)
            .execute(&mut *held)
            .await
            .unwrap();
        held.commit().await.unwrap();
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), task)
            .await
            .unwrap()
            .unwrap();
        denied(result.unwrap_err());
        let retained: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM post_secrets.staff_post_intents WHERE token_hash=$1)",
        )
        .bind(&p.ticket)
        .fetch_one(&f.owner)
        .await
        .unwrap();
        assert!(retained, "Failed consumption must roll back proof deletion");
        sqlx::query("DELETE FROM post_secrets.staff_post_intents WHERE token_hash=$1")
            .bind(&p.ticket)
            .execute(&f.owner)
            .await
            .unwrap();
    }
    f.cleanup().await;
}

#[tokio::test]
async fn janitor_discount_and_private_proof_helpers_have_no_public_capability() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let signature = "content.check_janitor_posting_cooldown(bytea,text,bigint,boolean,bigint)";
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
        assert!(!allowed, "No janitor discount capability for {role}");
    }
    let private = [
        "staff_identity.bind_post_timer_context(bytea,boolean)",
        "content.check_posting_cooldown_core(bytea,text,bigint,boolean,bigint,boolean)",
        "content.consume_staff_post_authority_core(bytea,bigint,text,bigint,text,text,text,timestamptz)",
        "staff_identity.issue_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz)",
        "staff_identity.issue_wordfiltered_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz,bytea,text)",
        "staff_identity.issue_limited_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text)",
        "staff_identity.issue_source_post_authority(bytea,bytea,bytea,integer,boolean,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text,text,text,boolean)",
        "staff_identity.issue_ordinary_post_authority(bytea,bytea,bytea,integer,bigint,text,bigint,text,text,text,timestamptz,boolean,integer,bytea,text,text,text,boolean,jsonb)",
    ];
    for role in ["board_public", "board_staff", "board_auth"] {
        for signature in private {
            let allowed: bool =
                sqlx::query_scalar("SELECT has_function_privilege($1,$2,'EXECUTE')")
                    .bind(role)
                    .bind(signature)
                    .fetch_one(&f.owner)
                    .await
                    .unwrap();
            assert!(
                !allowed,
                "Private helper or obsolete issuer exposed to {role}: {signature}"
            );
        }
        for column in ["raw_name_nonempty", "is_janitor", "meta_board"] {
            for privilege in ["SELECT", "UPDATE", "INSERT"] {
                let allowed: bool = sqlx::query_scalar(
                    "SELECT has_column_privilege($1,'post_secrets.staff_post_intents',$2,$3)",
                )
                .bind(role)
                .bind(column)
                .bind(privilege)
                .fetch_one(&f.owner)
                .await
                .unwrap();
                assert!(
                    !allowed,
                    "Proof context must stay private: {role} {privilege} {column}"
                );
            }
        }
    }
    for table in [
        "post_secrets.staff_post_intents",
        "staff_identity.accounts",
        "staff_identity.sessions",
    ] {
        let allowed: bool = sqlx::query_scalar(
            "SELECT has_any_column_privilege('board_posting_cooldown_owner',$1,'SELECT')",
        )
        .bind(table)
        .fetch_one(&f.owner)
        .await
        .unwrap();
        assert!(
            !allowed,
            "Cooldown owner needs no new staff-data access: {table}"
        );
    }
    let error = sqlx::query("SELECT * FROM content.check_janitor_posting_cooldown($1,$2,$3,$4,$5)")
        .bind(f.actor(peer(100)).as_slice())
        .bind(&f.boards[0])
        .bind(1_i64)
        .bind(false)
        .bind(f.now().await)
        .fetch_all(&f.public)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
    f.cleanup().await;
}

#[tokio::test]
async fn simultaneous_named_janitor_writes_share_actor_gate_and_clean_losing_proof() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    f.role("janitor").await;
    let now = f.now().await;
    let half = (f.delay().await + 1) / 2;
    let op = f.seed(0, 0, peer(110), now - 1000).await;
    let (one, two) = tokio::join!(
        f.write(0, op, peer(110), now, "Named", false),
        f.write(0, op, peer(110), now, "Named", false)
    );
    match (one, two) {
        (Ok(_), error @ Err(_)) | (error @ Err(_), Ok(_)) => rejected(error, half),
        other => panic!("Exactly one named janitor write must commit: {other:?}"),
    }
    f.no_proofs().await;
    let counts: (i64, i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM content.posts WHERE board=$1),(SELECT count(*) FROM post_secrets.posting_history WHERE board=$1),(SELECT count(*) FROM content.moderation_audit WHERE board=$1)")
        .bind(&f.boards[0]).fetch_one(&f.owner).await.unwrap();
    assert_eq!(counts, (2, 2, 1));
    f.cleanup().await;
}
