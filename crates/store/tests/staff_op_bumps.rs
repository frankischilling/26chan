#![cfg(feature = "database-tests")]

use board_domain::poster_id::PosterIdKey;
use board_store::{
    NewPost, PostIdentityKeys, PostMetadata, PostingContext, PostingCooldownReason,
    StaffPostAuthority, StaffPostIdentity, StoreError,
};
use sqlx::PgPool;
use std::net::IpAddr;

// Source imgboard.php:5862-6016: authenticated ordinary-timer eligibility
// gates self-sage, independently of private same-peer OP membership.
// Owned text-only policy/time fixtures; no imported policy is changed.
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
            sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,op_bump_limit,op_bump_initial_seconds,op_bump_repeat_seconds,permasage_hours,op_markup) VALUES($1,'Owned OP bump','Synthetic fixture',2000,1000,1000,1000,10,0,0,0,true,120,60,0,true)")
                .bind(board).execute(&owner).await.unwrap();
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

    async fn now(&self) -> i64 {
        sqlx::query_scalar("SELECT floor(extract(epoch FROM clock_timestamp()))::bigint")
            .fetch_one(&self.owner)
            .await
            .unwrap()
    }

    async fn role(&self, role: &str, meta: bool) {
        sqlx::query("UPDATE staff_identity.accounts SET role=$2 WHERE id=$1")
            .bind(self.account)
            .bind(role)
            .execute(&self.owner)
            .await
            .unwrap();
        sqlx::query("UPDATE content.boards SET meta_board=$2 WHERE slug=ANY($1)")
            .bind(&self.boards[..])
            .bind(meta)
            .execute(&self.owner)
            .await
            .unwrap();
    }

    async fn write(
        &self,
        parent: i64,
        peer: IpAddr,
        epoch: i64,
        input: &NewPost,
        valid: bool,
        proof: Option<[u8; 32]>,
    ) -> Result<i64, StoreError> {
        let key = PosterIdKey::parse(&self.key).unwrap();
        let ticket: Vec<u8> =
            sqlx::query_scalar("SELECT sha256(convert_to(gen_random_uuid()::text,'UTF8'))")
                .fetch_one(&self.owner)
                .await
                .unwrap();
        let ticket: [u8; 32] = ticket.try_into().unwrap();
        let invalid = [0_u8; 32];
        // Match the current authenticated role, as the source issuer requires.
        // Empty options select the ordinary identity branch (name_allowed=true),
        // rather than the separate capcode-name capability branch.
        let role: String =
            sqlx::query_scalar("SELECT role FROM staff_identity.accounts WHERE id=$1")
                .bind(self.account)
                .fetch_one(&self.owner)
                .await
                .unwrap();
        let authority = StaffPostAuthority {
            auth_pool: &self.auth,
            session_hash: &self.session,
            csrf_hash: if valid { &self.csrf } else { &invalid },
            ticket_hash: &ticket,
            idle_seconds: 900,
            highlight: false,
            authorized_limits: role != "janitor",
            raw_name_nonempty: !input.name.is_empty(),
            identity: Some(StaffPostIdentity {
                capcode: None,
                name_allowed: true,
                administrator: role == "admin",
                tripcode_key: None,
            }),
        };
        let mut context = context(peer, epoch);
        context.op_password_proof = proof;
        board_store::create_ordinary_staff_post(
            &self.staff,
            &self.boards[0],
            parent,
            input,
            context,
            PostMetadata {
                keys: PostIdentityKeys {
                    tripcode: None,
                    poster_id: Some(&key),
                },
                country_database: None,
                flag: "",
                // Mirror parsed source input; the store strips sage from
                // source options and separately binds post_sage in the proof.
                options: if input.sage { "sage" } else { "" },
                spoiler: false,
            },
            authority,
        )
        .await
    }

    async fn membership(&self, id: i64) -> bool {
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM post_secrets.op_replies WHERE post_id=$1)")
            .bind(id)
            .fetch_one(&self.owner)
            .await
            .unwrap()
    }

    async fn mark(&self, thread: i64) {
        sqlx::query("UPDATE content.threads SET bumped_at=to_timestamp(1000000000) WHERE id=$1")
            .bind(thread)
            .execute(&self.owner)
            .await
            .unwrap();
    }

    async fn bumped(&self, thread: i64) -> bool {
        sqlx::query_scalar(
            "SELECT bumped_at<>to_timestamp(1000000000) FROM content.threads WHERE id=$1",
        )
        .bind(thread)
        .fetch_one(&self.owner)
        .await
        .unwrap()
    }

    async fn reply(&self, thread: i64, peer: IpAddr, epoch: i64, name: &str, bump: bool) -> i64 {
        self.mark(thread).await;
        let mut input = post();
        input.name = name.into();
        let id = self
            .write(thread, peer, epoch, &input, true, None)
            .await
            .unwrap();
        assert_eq!(
            self.bumped(thread).await,
            bump,
            "unexpected bump for {name:?} at {epoch}"
        );
        let capcode: Option<String> =
            sqlx::query_scalar("SELECT capcode FROM content.posts WHERE id=$1")
                .bind(id)
                .fetch_one(&self.owner)
                .await
                .unwrap();
        assert_eq!(capcode, None, "this suite covers unbadged posts only");
        id
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

    async fn snapshot(&self) -> serde_json::Value {
        sqlx::query_scalar("SELECT jsonb_build_object('posts',(SELECT coalesce(jsonb_agg(to_jsonb(p) ORDER BY id),'[]') FROM content.posts p WHERE board=ANY($1)),'threads',(SELECT coalesce(jsonb_agg(to_jsonb(t) ORDER BY id),'[]') FROM content.threads t WHERE board=ANY($1)),'history',(SELECT coalesce(jsonb_agg(to_jsonb(h) ORDER BY post_id),'[]') FROM post_secrets.posting_history h WHERE board=ANY($1)),'actions',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY board,actor_hash),'[]') FROM post_secrets.posting_thread_actions a WHERE board=ANY($1)),'op_peers',(SELECT coalesce(jsonb_agg(to_jsonb(o) ORDER BY thread_id),'[]') FROM post_secrets.op_peers o WHERE thread_id IN(SELECT id FROM content.threads WHERE board=ANY($1))),'op_replies',(SELECT coalesce(jsonb_agg(to_jsonb(o) ORDER BY post_id),'[]') FROM post_secrets.op_replies o WHERE thread_id IN(SELECT id FROM content.threads WHERE board=ANY($1))),'audit',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY id),'[]') FROM content.moderation_audit a WHERE board=ANY($1)))")
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
async fn authenticated_role_raw_name_and_meta_gate_initial_and_repeat_strict_edges() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let now = f.now().await;
    let mut n = 1;
    for role in ["janitor", "moderator", "manager", "admin"] {
        for meta in [false, true] {
            f.role(role, meta).await;
            for name in ["", "   ", "Anonymous", "Named"] {
                let limited = role == "janitor" && (!name.is_empty() || meta);
                for repeat in [false, true] {
                    for equality in [false, true] {
                        let ip = peer(n);
                        n += 1;
                        let op = f
                            .public_write(0, 0, ip, now - if repeat { 1000 } else { 120 })
                            .await
                            .unwrap();
                        if repeat {
                            f.public_write(0, op, ip, now - 60).await.unwrap();
                        }
                        let id = f
                            .reply(
                                op,
                                ip,
                                now - i64::from(!equality),
                                name,
                                !limited || equality,
                            )
                            .await;
                        assert!(
                            f.membership(id).await,
                            "exempt posts must retain private OP membership"
                        );
                        if name.trim().is_empty() {
                            let normalized: String =
                                sqlx::query_scalar("SELECT name FROM content.posts WHERE id=$1")
                                    .bind(id)
                                    .fetch_one(&f.owner)
                                    .await
                                    .unwrap();
                            assert_eq!(
                                normalized, "Anonymous",
                                "raw-name eligibility survives normalization"
                            );
                        }
                    }
                }
            }
        }
    }
    f.cleanup().await;
}

#[tokio::test]
async fn exempt_same_peer_membership_limits_later_janitor_and_public_replies() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let now = f.now().await;
    for (n, role, name) in [
        (150, "moderator", "Named"),
        (151, "manager", "Named"),
        (152, "admin", "Named"),
        (153, "janitor", ""),
    ] {
        let ip = peer(n);
        let op = f.public_write(0, 0, ip, now - 1000).await.unwrap();
        f.role(role, false).await;
        let first = f.reply(op, ip, now, name, true).await;
        assert!(f.membership(first).await);
        f.role("janitor", false).await;
        let next = f.reply(op, ip, now + 59, "Named", false).await;
        assert!(f.membership(next).await);
        // Public writes must see the exempt staff membership too. Use a separate
        // thread so the preceding eligible janitor cannot mask that assertion.
        f.role(role, false).await;
        let public_op = f
            .public_write(0, 0, peer(n + 10), now - 1000)
            .await
            .unwrap();
        f.reply(public_op, peer(n + 10), now, name, true).await;
        f.mark(public_op).await;
        let id = f
            .public_write(0, public_op, peer(n + 10), now + 59)
            .await
            .unwrap();
        assert!(!f.bumped(public_op).await);
        assert!(f.membership(id).await);
    }
    f.cleanup().await;
}

#[tokio::test]
async fn different_peer_and_password_only_markup_do_not_create_self_bump_membership() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let now = f.now().await;
    f.role("janitor", true).await;
    let op = f.public_write(0, 0, peer(180), now - 10).await.unwrap();
    let different = f.reply(op, peer(181), now, "Named", true).await;
    assert!(!f.membership(different).await);
    let proof: Vec<u8> = sqlx::query_scalar("SELECT sha256(convert_to(password_hash,'UTF8')) FROM post_secrets.deletion WHERE post_id=$1")
        .bind(op).fetch_one(&f.owner).await.unwrap();
    f.mark(op).await;
    let password_only = f
        .write(
            op,
            peer(182),
            now,
            &post(),
            true,
            Some(proof.try_into().unwrap()),
        )
        .await
        .unwrap();
    assert!(
        f.bumped(op).await,
        "cosmetic password OP status is not transport membership"
    );
    assert!(!f.membership(password_only).await);
    let flags: i16 = sqlx::query_scalar("SELECT comment_format FROM content.posts WHERE id=$1")
        .bind(password_only)
        .fetch_one(&f.owner)
        .await
        .unwrap();
    assert_ne!(
        flags & 16,
        0,
        "valid password proof must actually enable OP markup"
    );
    f.cleanup().await;
}

#[tokio::test]
async fn explicit_sage_permaage_sticky_and_disabled_policy_keep_precedence() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let now = f.now().await;
    let mut n = 190;
    for (role, name) in [("moderator", "Named"), ("janitor", "Named")] {
        f.role(role, false).await;
        for (sage, permaage, sticky, permasage, enabled, expected) in [
            (true, false, false, false, true, false),
            (true, true, false, false, true, true),
            (false, true, true, false, true, false),
            (false, true, false, true, true, false),
            (false, false, false, false, false, true),
        ] {
            let ip = peer(n);
            n += 1;
            let op = f.public_write(0, 0, ip, now - 10).await.unwrap();
            sqlx::query("UPDATE content.boards SET op_bump_limit=$2 WHERE slug=$1")
                .bind(&f.boards[0])
                .bind(enabled)
                .execute(&f.owner)
                .await
                .unwrap();
            sqlx::query(
                "UPDATE content.threads SET permaage=$2,sticky=$3,permasage=$4 WHERE id=$1",
            )
            .bind(op)
            .bind(permaage)
            .bind(sticky)
            .bind(permasage)
            .execute(&f.owner)
            .await
            .unwrap();
            let mut input = post();
            input.name = name.into();
            input.sage = sage;
            f.mark(op).await;
            let id = f.write(op, ip, now, &input, true, None).await.unwrap();
            assert_eq!(f.bumped(op).await, expected);
            assert!(f.membership(id).await);
        }
    }
    f.cleanup().await;
}

#[tokio::test]
async fn newest_surviving_id_not_max_timestamp_controls_repeat_window() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let now = f.now().await;
    f.role("janitor", false).await;
    for deleted in [false, true] {
        let ip = peer(if deleted { 211 } else { 210 });
        let op = f.public_write(0, 0, ip, now - 1000).await.unwrap();
        let lower = f.public_write(0, op, ip, now - 100).await.unwrap();
        let higher = f.public_write(0, op, ip, now - 90).await.unwrap();
        // Seed inverted content timestamps without making a backwards-clock
        // public request fail admission; rate history stays safely in the past.
        sqlx::query(
            "UPDATE content.posts SET created_at=to_timestamp($2::double precision) WHERE id=$1",
        )
        .bind(lower)
        .bind(now - 10)
        .execute(&f.owner)
        .await
        .unwrap();
        assert!(higher > lower);
        if deleted {
            sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1")
                .bind(higher)
                .execute(&f.owner)
                .await
                .unwrap();
            assert!(!f.membership(higher).await);
        }
        f.reply(op, ip, now, "Named", !deleted).await;
    }
    f.cleanup().await;
}

#[tokio::test]
async fn invalid_proof_and_both_cooldowns_roll_back_bumps_and_membership() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let now = f.now().await;
    let op = f.public_write(0, 0, peer(220), now - 1000).await.unwrap();
    f.role("moderator", false).await;
    f.reply(op, peer(220), now, "Named", true).await;
    let before = f.snapshot().await;
    assert!(matches!(
        f.write(op, peer(220), now + 60, &post(), false, None).await,
        Err(StoreError::AuthorizationChanged)
    ));
    assert_eq!(
        f.snapshot().await,
        before,
        "invalid proof must leave no content or membership changes"
    );
    rejected(
        f.write(op, peer(220), now + 4, &post(), true, None).await,
        1,
    );
    assert_eq!(
        f.snapshot().await,
        before,
        "exempt staff still have the five-second admission gate"
    );
    f.reply(op, peer(220), now + 5, "Named", true).await;
    f.role("janitor", false).await;
    sqlx::query("UPDATE content.boards SET posting_reply_seconds=21 WHERE slug=$1")
        .bind(&f.boards[0])
        .execute(&f.owner)
        .await
        .unwrap();
    let before = f.snapshot().await;
    rejected(
        f.write(op, peer(220), now + 15, &post(), true, None).await,
        1,
    );
    assert_eq!(
        f.snapshot().await,
        before,
        "ceil-half ordinary janitor cooldown must roll back too"
    );
    f.reply(op, peer(220), now + 16, "Named", false).await;
    f.cleanup().await;
}
