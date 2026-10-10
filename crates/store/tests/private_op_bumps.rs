#![cfg(feature = "database-tests")]

use board_domain::{capcode::Capcode, poster_id::PosterIdKey};
use board_store::{
    NewPost, PostIdentityKeys, PostMetadata, PostingContext, PostingCooldownReason,
    StaffPostAuthority, StaffPostIdentity, StoreError,
};
use sqlx::PgPool;
use std::net::IpAddr;

// Bump-only HMAC evidence must not widen public attribution. All posts use
// production writers; owner SQL changes only synthetic fixtures and history.
// This suite deliberately separates admission request time from content time.
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
        context: PostingContext,
        input: &NewPost,
        valid: bool,
        badged: bool,
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
                capcode: badged.then_some(Capcode::Moderator),
                name_allowed: true,
                administrator: role == "admin",
                tripcode_key: None,
            }),
        };
        if badged {
            return board_store::create_staff_post_with_context_and_keys(
                &self.staff,
                &self.boards[0],
                parent,
                input,
                context,
                PostIdentityKeys {
                    tripcode: None,
                    poster_id: Some(&key),
                },
                authority,
            )
            .await;
        }
        board_store::create_ordinary_staff_post(
            &self.staff,
            &self.boards[0],
            parent,
            input,
            context,
            PostMetadata {
                drawing: None,
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
            .write(thread, context(peer, epoch), &input, true, false)
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

    async fn snapshot(&self) -> serde_json::Value {
        sqlx::query_scalar("SELECT jsonb_build_object('posts',(SELECT coalesce(jsonb_agg(to_jsonb(p) ORDER BY id),'[]') FROM content.posts p WHERE board=ANY($1)),'threads',(SELECT coalesce(jsonb_agg(to_jsonb(t) ORDER BY id),'[]') FROM content.threads t WHERE board=ANY($1)),'history',(SELECT coalesce(jsonb_agg(to_jsonb(h) ORDER BY post_id),'[]') FROM post_secrets.posting_history h WHERE board=ANY($1)),'actions',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY board,actor_hash),'[]') FROM post_secrets.posting_thread_actions a WHERE board=ANY($1)),'op_peers',(SELECT coalesce(jsonb_agg(to_jsonb(o) ORDER BY thread_id),'[]') FROM post_secrets.op_peers o WHERE thread_id IN(SELECT id FROM content.threads WHERE board=ANY($1))),'op_replies',(SELECT coalesce(jsonb_agg(to_jsonb(o) ORDER BY post_id),'[]') FROM post_secrets.op_replies o WHERE thread_id IN(SELECT id FROM content.threads WHERE board=ANY($1))),'audit',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY id),'[]') FROM content.moderation_audit a WHERE board=ANY($1)))")
            .bind(&self.boards[..]).fetch_one(&self.owner).await.unwrap()
    }

    async fn cleanup(self) {
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

// Query the narrowly scoped API, never give runtime roles history-table access.
type Evidence = (bool, Option<i64>, Option<chrono::DateTime<chrono::Utc>>);
impl Fixture {
    fn actor(&self, ip: IpAddr) -> [u8; 32] {
        *PosterIdKey::parse(&self.key)
            .unwrap()
            .public_posting_rate_identity(ip)
            .as_bytes()
    }

    async fn evidence(
        &self,
        pool: &PgPool,
        board: usize,
        thread: i64,
        ip: IpAddr,
    ) -> Option<Evidence> {
        sqlx::query_as("SELECT own_reply,latest_post_id,latest_created_at FROM content.posting_op_bump_context($1,$2,$3)")
            .bind(self.actor(ip).as_slice()).bind(&self.boards[board]).bind(thread)
            .fetch_optional(pool).await.unwrap()
    }

    async fn private(&self) {
        sqlx::query("UPDATE content.boards SET staff_only=true WHERE slug=$1")
            .bind(&self.boards[0])
            .execute(&self.owner)
            .await
            .unwrap();
    }

    async fn badge(&self, parent: i64, ip: IpAddr, epoch: i64) -> i64 {
        self.role("moderator", false).await;
        let id = self
            .write(parent, context(ip, epoch), &post(), true, true)
            .await
            .unwrap();
        let capcode: Option<String> =
            sqlx::query_scalar("SELECT capcode FROM content.posts WHERE id=$1")
                .bind(id)
                .fetch_one(&self.owner)
                .await
                .unwrap();
        assert_eq!(capcode.as_deref(), Some("mod"));
        self.no_attribution(id).await;
        id
    }

    async fn no_attribution(&self, id: i64) {
        let (op_peer, reply, counted, flags, is_reply): (bool, bool, bool, i16, bool) = sqlx::query_as(
            "SELECT EXISTS(SELECT 1 FROM post_secrets.op_peers WHERE thread_id=p.id),
             EXISTS(SELECT 1 FROM post_secrets.op_replies WHERE post_id=p.id),
             EXISTS(SELECT 1 FROM post_secrets.poster_contexts WHERE post_id=p.id),p.comment_format,p.id<>p.thread_id
             FROM content.posts p WHERE p.id=$1",
        )
        .bind(id)
        .fetch_one(&self.owner)
        .await
        .unwrap();
        assert!(
            !op_peer && !reply && !counted,
            "bump evidence must not create legacy attribution"
        );
        if is_reply {
            assert_eq!(
                flags & 16,
                0,
                "private reply evidence must not create public OP markup"
            );
        } // Original posts already receive the OP formatter flag independently.
    }

    async fn public_reply(&self, op: i64, ip: IpAddr, epoch: i64, expected: bool) -> i64 {
        self.mark(op).await;
        let id = self.public_write(0, op, ip, epoch).await.unwrap();
        assert_eq!(self.bumped(op).await, expected);
        id
    }

    async fn content_time(&self, id: i64, epoch: i64) {
        sqlx::query(
            "UPDATE content.posts SET created_at=to_timestamp($2::double precision) WHERE id=$1",
        )
        .bind(id)
        .bind(epoch)
        .execute(&self.owner)
        .await
        .unwrap();
    }
}

// /j uses the limited private-discussion proof, never the ordinary public
// proof. Read imported policy; mutate only this fixture's account/posts.
#[tokio::test]
async fn private_janitor_discussion_uses_initial_repeat_and_authenticated_role_gates() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let now = f.now().await;
    let (enabled, initial, repeat, meta): (bool, i32, i32, bool) = sqlx::query_as(
        "SELECT op_bump_limit,op_bump_initial_seconds,op_bump_repeat_seconds,meta_board FROM content.boards WHERE slug='j'")
        .fetch_one(&f.owner).await.unwrap();
    assert!(
        enabled && initial > 0 && repeat > 0,
        "Imported /j self-bump policy is required"
    );
    let mut n = 1;
    for (role, named) in [("janitor", true), ("janitor", false), ("moderator", true)] {
        for repeated in [false, true] {
            for equality in [false, true] {
                let ip = peer(n);
                n += 1;
                let op = f
                    .discussion(0, ip, now - 2000, "moderator", true, true)
                    .await
                    .unwrap();
                let target = if repeated {
                    f.discussion(op, ip, now - 1000, "moderator", true, true)
                        .await
                        .unwrap()
                } else {
                    op
                };
                let threshold = i64::from(if repeated { repeat } else { initial });
                f.content_time(target, now - threshold + i64::from(!equality))
                    .await;
                f.mark(op).await;
                let id = f.discussion(op, ip, now, role, named, true).await.unwrap();
                assert_eq!(
                    f.bumped(op).await,
                    role != "janitor" || (!named && !meta) || equality
                );
                f.no_attribution(id).await;
                let evidence: Evidence =
                    sqlx::query_as("SELECT * FROM content.posting_op_bump_context($1,'j',$2)")
                        .bind(f.actor(ip).as_slice())
                        .bind(op)
                        .fetch_one(&f.staff)
                        .await
                        .unwrap();
                assert_eq!((evidence.0, evidence.1), (true, Some(id)));
                // Clean only the newly owned thread; leave imported /j unchanged.
                sqlx::query("DELETE FROM content.moderation_audit WHERE board='j' AND target_id IN(SELECT id FROM content.posts WHERE thread_id=$1)")
                    .bind(op).execute(&f.owner).await.unwrap();
                sqlx::query("DELETE FROM content.posts WHERE board='j' AND thread_id=$1")
                    .bind(op)
                    .execute(&f.owner)
                    .await
                    .unwrap();
                sqlx::query("DELETE FROM content.threads WHERE board='j' AND id=$1")
                    .bind(op)
                    .execute(&f.owner)
                    .await
                    .unwrap();
                sqlx::query("DELETE FROM post_secrets.posting_thread_actions WHERE board='j' AND actor_hash=$1")
                    .bind(f.actor(ip).as_slice()).execute(&f.owner).await.unwrap();
            }
        }
    }
    f.cleanup().await;
}

#[tokio::test]
async fn badged_ops_and_intervening_replies_limit_public_and_eligible_staff_only() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let now = f.now().await;
    let mut n = 40;
    for (janitor, meta) in [(false, false), (true, false), (true, true)] {
        for badged_op in [false, true] {
            let ip = peer(n);
            n += 1;
            let op = if badged_op {
                f.badge(0, ip, now - 10).await
            } else {
                let op = f.public_write(0, 0, ip, now - 1000).await.unwrap();
                f.badge(op, ip, now - 10).await;
                op
            };
            let before = f.evidence(&f.public, 0, op, ip).await.unwrap();
            assert!(before.0);
            assert_eq!(before.1.is_some(), !badged_op);
            let id = if janitor {
                f.role("janitor", meta).await;
                f.reply(op, ip, now, if meta { "" } else { "Named" }, false)
                    .await
            } else {
                f.public_reply(op, ip, now, false).await
            };
            assert_eq!(f.membership(id).await, !badged_op);
            let flags: i16 =
                sqlx::query_scalar("SELECT comment_format FROM content.posts WHERE id=$1")
                    .bind(id)
                    .fetch_one(&f.owner)
                    .await
                    .unwrap();
            assert_eq!(
                flags & 16 != 0,
                !badged_op,
                "only legacy ownership supplies markup"
            );
            let count: Option<i32> = sqlx::query_scalar("SELECT content.unique_posters($1,$2)")
                .bind(&f.boards[0])
                .bind(op)
                .fetch_one(&f.public)
                .await
                .unwrap();
            assert_eq!(
                count, None,
                "badged posts must still make the public count incomplete"
            );
        }
    }
    // Someone else's fresh badged reply cannot become the OP's repeat timer.
    let op = f.public_write(0, 0, peer(50), now - 1000).await.unwrap();
    f.badge(op, peer(51), now - 10).await;
    assert_eq!(
        f.evidence(&f.public, 0, op, peer(50)).await.unwrap().1,
        None
    );
    f.public_reply(op, peer(50), now, true).await;
    // A same-host reply alone does not prove ownership of someone else's OP.
    let op = f.public_write(0, 0, peer(52), now - 1000).await.unwrap();
    let badge = f.badge(op, peer(53), now - 10).await;
    let evidence = f.evidence(&f.public, 0, op, peer(53)).await.unwrap();
    assert_eq!((evidence.0, evidence.1), (false, Some(badge)));
    f.public_reply(op, peer(53), now, true).await;
    f.cleanup().await;
}

#[tokio::test]
async fn highest_surviving_id_uses_content_time_instead_of_history_request_time() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let now = f.now().await;
    for (n, delete_latest, latest_age) in [(60, false, 70), (61, false, 10), (62, true, 70)] {
        let ip = peer(n);
        let op = f.public_write(0, 0, ip, now - 1000).await.unwrap();
        let lower = f.badge(op, ip, now - 100).await;
        let higher = f.badge(op, ip, now - 90).await;
        assert!(higher > lower);
        f.content_time(lower, now - 5).await;
        f.content_time(higher, now - latest_age).await;
        if delete_latest {
            sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1")
                .bind(higher)
                .execute(&f.owner)
                .await
                .unwrap();
        }
        let evidence = f.evidence(&f.public, 0, op, ip).await.unwrap();
        assert_eq!(evidence.1, Some(if delete_latest { lower } else { higher }));
        assert_eq!(
            evidence.2.unwrap().timestamp(),
            now - if delete_latest { 5 } else { latest_age }
        );
        f.public_reply(op, ip, now, !delete_latest && latest_age >= 60)
            .await;
    }
    f.cleanup().await;
}

#[tokio::test]
async fn missing_history_and_rotated_keys_preserve_only_available_legacy_evidence() {
    let _serial = TEST.lock().await;
    let mut f = Fixture::new().await;
    let now = f.now().await;
    // Pre-0087 public OP: legacy ownership with no reply still gates initial time.
    let initial = f.public_write(0, 0, peer(70), now - 10).await.unwrap();
    sqlx::query("DELETE FROM post_secrets.posting_history WHERE thread_id=$1")
        .bind(initial)
        .execute(&f.owner)
        .await
        .unwrap();
    assert!(!f.evidence(&f.public, 0, initial, peer(70)).await.unwrap().0);
    f.role("janitor", false).await;
    f.reply(initial, peer(70), now, "Named", false).await;
    // Pre-0087 replies retain their legacy repeat evidence for public writers.
    let old = f.public_write(0, 0, peer(71), now - 1000).await.unwrap();
    f.public_write(0, old, peer(71), now - 10).await.unwrap();
    sqlx::query("DELETE FROM post_secrets.posting_history WHERE thread_id=$1")
        .bind(old)
        .execute(&f.owner)
        .await
        .unwrap();
    f.public_reply(old, peer(71), now, false).await;
    // Rotation: current-key badged reply has a higher ID but cannot prove the OP.
    // Both public and staff must merge it with old-key legacy ownership by ID.
    let mut rotated = Vec::new();
    for n in [72, 73] {
        let op = f.public_write(0, 0, peer(n), now - 1000).await.unwrap();
        let legacy = f.public_write(0, op, peer(n), now - 100).await.unwrap();
        rotated.push((n, op, legacy));
    }
    f.key = "ba".repeat(32);
    for (n, op, legacy) in rotated {
        let recent = f.badge(op, peer(n), now - 10).await;
        assert!(recent > legacy);
        let evidence = f.evidence(&f.public, 0, op, peer(n)).await.unwrap();
        assert_eq!((evidence.0, evidence.1), (false, Some(recent)));
        if n == 72 {
            f.public_reply(op, peer(n), now, false).await;
        } else {
            f.role("janitor", false).await;
            f.reply(op, peer(n), now, "Named", false).await;
        }
    }
    // The reverse merge direction matters too: a newer legacy-only row wins
    // over older-ID HMAC evidence even when its timestamp is earlier.
    let op = f.public_write(0, 0, peer(75), now - 2000).await.unwrap();
    let fresh = f.badge(op, peer(75), now - 100).await;
    let legacy = f.public_write(0, op, peer(75), now - 90).await.unwrap();
    assert!(legacy > fresh);
    sqlx::query("DELETE FROM post_secrets.posting_history WHERE post_id=$1")
        .bind(legacy)
        .execute(&f.owner)
        .await
        .unwrap();
    f.content_time(fresh, now - 5).await;
    assert_eq!(
        f.evidence(&f.public, 0, op, peer(75)).await.unwrap().1,
        Some(fresh)
    );
    f.public_reply(op, peer(75), now, true).await;
    // Historical private/badged ownership cannot be reconstructed without HMAC.
    let op = f.badge(0, peer(74), now - 10).await;
    sqlx::query("DELETE FROM post_secrets.posting_history WHERE thread_id=$1")
        .bind(op)
        .execute(&f.owner)
        .await
        .unwrap();
    f.role("janitor", false).await;
    let id = f.reply(op, peer(74), now, "Named", true).await;
    assert!(!f.membership(id).await);
    f.cleanup().await;
}

#[tokio::test]
async fn file_deletion_keeps_evidence_but_deleted_and_archived_threads_hide_it() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let now = f.now().await;
    let op = f.public_write(0, 0, peer(80), now - 1000).await.unwrap();
    let reply = f.badge(op, peer(80), now - 10).await;
    // Synthetic attachment metadata: the post itself still came from the writer.
    // This specifically exercises the file-only deletion bit, not upload admission.
    sqlx::query("INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler) VALUES($1,replace(gen_random_uuid()::text,'-',''),replace(gen_random_uuid()::text,'-',''),'owned.png',1,1,1,false)")
        .bind(reply).execute(&f.owner).await.unwrap();
    sqlx::query("UPDATE content.post_media SET file_deleted=true WHERE post_id=$1")
        .bind(reply)
        .execute(&f.owner)
        .await
        .unwrap();
    assert_eq!(
        f.evidence(&f.public, 0, op, peer(80)).await.unwrap().1,
        Some(reply)
    );
    f.public_reply(op, peer(80), now, false).await;
    sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 day' WHERE id=$1")
        .bind(op)
        .execute(&f.owner)
        .await
        .unwrap();
    assert!(f.evidence(&f.public, 0, op, peer(80)).await.is_none());
    let deleted = f.public_write(0, 0, peer(81), now - 1000).await.unwrap();
    sqlx::query("UPDATE content.threads SET deleted=true WHERE id=$1")
        .bind(deleted)
        .execute(&f.owner)
        .await
        .unwrap();
    assert!(f.evidence(&f.public, 0, deleted, peer(81)).await.is_none());
    let deleted_op = f.public_write(0, 0, peer(82), now - 1000).await.unwrap();
    sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1")
        .bind(deleted_op)
        .execute(&f.owner)
        .await
        .unwrap();
    assert!(
        f.evidence(&f.public, 0, deleted_op, peer(82))
            .await
            .is_none()
    );
    f.cleanup().await;
}

#[tokio::test]
async fn rejected_staff_writes_leave_private_history_content_and_bumps_unchanged() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let now = f.now().await;
    let op = f.badge(0, peer(90), now - 1000).await;
    f.badge(op, peer(90), now).await;
    let before = f.snapshot().await;
    assert!(matches!(
        f.write(op, context(peer(90), now + 60), &post(), false, false)
            .await,
        Err(StoreError::AuthorizationChanged)
    ));
    assert_eq!(f.snapshot().await, before);
    rejected(
        f.write(op, context(peer(90), now + 4), &post(), true, false)
            .await,
        1,
    );
    assert_eq!(f.snapshot().await, before);
    f.role("janitor", false).await;
    sqlx::query("UPDATE content.boards SET posting_reply_seconds=21 WHERE slug=$1")
        .bind(&f.boards[0])
        .execute(&f.owner)
        .await
        .unwrap();
    let before = f.snapshot().await;
    rejected(
        f.write(op, context(peer(90), now + 10), &post(), true, false)
            .await,
        1,
    );
    assert_eq!(f.snapshot().await, before);
    let id = f.reply(op, peer(90), now + 11, "Named", false).await;
    assert_eq!(
        f.evidence(&f.staff, 0, op, peer(90)).await.unwrap().1,
        Some(id)
    );
    f.cleanup().await;
}

#[tokio::test]
async fn scoped_context_does_not_expose_private_boards_or_history_tables() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    f.private().await;
    let now = f.now().await;
    let op = f.badge(0, peer(100), now - 1000).await;
    f.badge(op, peer(100), now - 10).await;
    assert!(f.evidence(&f.staff, 0, op, peer(100)).await.unwrap().0);
    assert!(f.evidence(&f.public, 0, op, peer(100)).await.is_none());
    assert!(
        f.evidence(&f.staff, 1, op, peer(100)).await.is_none(),
        "board and thread must match"
    );
    for connection in [&f.public, &f.staff] {
        let error = sqlx::query("SELECT actor_hash FROM post_secrets.posting_history LIMIT 1")
            .execute(connection)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501")
        );
    }
    let error = sqlx::query("SELECT * FROM content.staff_op_bump_context($1,$2,$3)")
        .bind(&f.boards[0])
        .bind(op)
        .bind(peer(100).to_string())
        .execute(&f.public)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
    let legacy: Option<Evidence> =
        sqlx::query_as("SELECT * FROM content.staff_op_bump_context($1,$2,$3)")
            .bind(&f.boards[0])
            .bind(op)
            .bind(peer(100).to_string())
            .fetch_optional(&f.staff)
            .await
            .unwrap();
    assert!(
        legacy.is_none(),
        "private boards must never gain legacy attribution"
    );
    f.cleanup().await;
}

impl Fixture {
    async fn discussion(
        &self,
        parent: i64,
        ip: IpAddr,
        epoch: i64,
        role: &str,
        named: bool,
        valid: bool,
    ) -> Result<i64, StoreError> {
        sqlx::query("UPDATE staff_identity.accounts SET role=$2 WHERE id=$1")
            .bind(self.account)
            .bind(role)
            .execute(&self.owner)
            .await
            .unwrap();
        let key = PosterIdKey::parse(&self.key).unwrap();
        let ticket: Vec<u8> =
            sqlx::query_scalar("SELECT sha256(convert_to(gen_random_uuid()::text,'UTF8'))")
                .fetch_one(&self.owner)
                .await
                .unwrap();
        let ticket: [u8; 32] = ticket.try_into().unwrap();
        let invalid = [0_u8; 32];
        // /j masks the saved name, but binds the original parsed-name fact.
        let mut input = post();
        input.name = "Anonymous".into();
        board_store::create_staff_post_with_context_and_keys(
            &self.staff,
            "j",
            parent,
            &input,
            context(ip, epoch),
            PostIdentityKeys {
                tripcode: None,
                poster_id: Some(&key),
            },
            StaffPostAuthority {
                auth_pool: &self.auth,
                session_hash: &self.session,
                csrf_hash: if valid { &self.csrf } else { &invalid },
                ticket_hash: &ticket,
                idle_seconds: 900,
                highlight: false,
                authorized_limits: role != "janitor",
                raw_name_nonempty: named,
                identity: None,
            },
        )
        .await
    }
}
