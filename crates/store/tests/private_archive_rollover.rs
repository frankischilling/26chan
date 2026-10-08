#![cfg(feature = "database-tests")]

use board_domain::{capcode::Capcode, poster_id::PosterIdKey};
use board_store::{
    BoardSelection, NewPost, PostIdentityKeys, PostingContext, StaffPostAuthority,
    StaffPostIdentity, StoreError,
};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{net::IpAddr, time::Duration};

// Source trim_threads exempts JANITOR_BOARD, mapped to staff_only. trim_archive
// is a separate operation and must still run. Never modify imported /j policy.
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
    async fn new() -> Self {
        let owner = pool("MIGRATION_DATABASE_URL").await;
        let seed: String = sqlx::query_scalar("SELECT replace(gen_random_uuid()::text,'-','')")
            .fetch_one(&owner)
            .await
            .unwrap();
        let board = format!("v{}", &seed[..9]);
        sqlx::query("INSERT INTO content.boards(slug,title,description,staff_only,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) VALUES($1,'Owned private rollover','Synthetic fixture',true,2000,1000,1000,1000,10,0,0,0)")
            .bind(&board).execute(&owner).await.unwrap();
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
        let key = PosterIdKey::parse(&self.key).unwrap();
        let ticket: Vec<u8> =
            sqlx::query_scalar("SELECT sha256(convert_to(gen_random_uuid()::text,'UTF8'))")
                .fetch_one(&self.owner)
                .await
                .unwrap();
        let ticket: [u8; 32] = ticket.try_into().unwrap();
        let invalid = [0_u8; 32];
        board_store::create_staff_post_with_context_and_keys(
            &self.staff,
            &self.board,
            parent,
            &post(),
            context(actor),
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
                authorized_limits: true,
                raw_name_nonempty: true,
                identity: Some(StaffPostIdentity {
                    capcode: Some(Capcode::Moderator),
                    name_allowed: true,
                    administrator: false,
                    tripcode_key: None,
                }),
            },
        )
        .await
    }

    async fn policy(&self, retention: i32) {
        sqlx::query("UPDATE content.boards SET thread_limit=1,archive_retention_seconds=$2,archive_limit=1 WHERE slug=$1")
            .bind(&self.board).bind(retention).execute(&self.owner).await.unwrap();
    }

    async fn snapshot(&self) -> serde_json::Value {
        sqlx::query_scalar("SELECT jsonb_build_object('threads',(SELECT coalesce(jsonb_agg(to_jsonb(t) ORDER BY id),'[]') FROM content.threads t WHERE board=$1),'posts',(SELECT coalesce(jsonb_agg(to_jsonb(p) ORDER BY id),'[]') FROM content.posts p WHERE board=$1),'history',(SELECT coalesce(jsonb_agg(to_jsonb(h) ORDER BY post_id),'[]') FROM post_secrets.posting_history h WHERE board=$1),'actions',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY actor_hash),'[]') FROM post_secrets.posting_thread_actions a WHERE board=$1),'intents',(SELECT coalesce(jsonb_agg(to_jsonb(i) ORDER BY post_id),'[]') FROM post_secrets.staff_post_intents i WHERE board=$1),'audit',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY id),'[]') FROM content.moderation_audit a WHERE board=$1))")
            .bind(&self.board).fetch_one(&self.owner).await.unwrap()
    }

    async fn assert_active(&self, ids: &[i64]) {
        let actual: Vec<i64> = sqlx::query_scalar("SELECT id FROM content.threads WHERE board=$1 AND id=ANY($2) AND NOT deleted AND archived_at IS NULL AND archive_expires_at IS NULL ORDER BY id")
            .bind(&self.board).bind(ids).fetch_all(&self.owner).await.unwrap();
        let mut expected = ids.to_vec();
        expected.sort_unstable();
        assert_eq!(
            actual, expected,
            "private active threads must never become rollover victims"
        );
    }

    async fn assert_public_denied(&self, op: i64) {
        assert!(matches!(
            board_store::board(&self.public, &self.board).await,
            Err(StoreError::NotFound)
        ));
        assert!(matches!(
            board_store::thread(&self.public, &self.board, op).await,
            Err(StoreError::NotFound)
        ));
        assert!(matches!(
            board_store::board_snapshot(&self.public, &self.board, BoardSelection::All, Some(0))
                .await,
            Err(StoreError::NotFound)
        ));
        assert!(matches!(
            board_store::archive_snapshot(&self.public, &self.board).await,
            Err(StoreError::NotFound)
        ));
        let key = PosterIdKey::parse(&self.key).unwrap();
        for parent in [0, op] {
            assert!(matches!(
                board_store::create_post_with_identity_keys(
                    &self.public,
                    &self.board,
                    parent,
                    &post(),
                    None,
                    context(200),
                    PostIdentityKeys {
                        tripcode: None,
                        poster_id: Some(&key)
                    },
                )
                .await,
                Err(StoreError::NotFound)
            ));
        }
    }

    async fn cleanup(self) {
        for query in [
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

#[tokio::test]
async fn private_active_threads_survive_while_archive_cleanup_and_rejections_remain_effective() {
    let _serial = TEST.lock().await;
    for retention in [0, 3600] {
        let f = Fixture::new().await;
        let work = f.clone();
        let result = tokio::spawn(async move {
            let f = work;
            let mut active = Vec::new();
            let mut replies = Vec::new();
            for (n, sticky, undead) in [
                (1, false, false), (2, false, false), (3, true, false),
                (4, false, true), (5, true, true),
            ] {
                let op = f.write(0, n, true).await.unwrap();
                let reply = f.write(op, n + 20, true).await.unwrap();
                sqlx::query("UPDATE content.threads SET sticky=$2,undead=$3 WHERE id=$1 AND board=$4")
                    .bind(op).bind(sticky).bind(undead).bind(&f.board).execute(&f.owner).await.unwrap();
                active.push(op);
                replies.push(reply);
            }
            // Seed archives through real staff posts, changing only owned
            // lifecycle metadata. Newest expired archive must not consume a slot.
            let expired = f.write(0, 40, true).await.unwrap();
            let over_cap = f.write(0, 41, true).await.unwrap();
            let retained = f.write(0, 42, true).await.unwrap();
            for (id, age, expired) in [(expired, 90_i64, true), (over_cap, 300, false), (retained, 100, false)] {
                sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp()-make_interval(secs=>$2::double precision),archive_expires_at=clock_timestamp()+make_interval(secs=>$3::double precision) WHERE id=$1 AND board=$4")
                    .bind(id).bind(age).bind(if expired { -60_i64 } else { 3600 }).bind(&f.board).execute(&f.owner).await.unwrap();
            }
            f.policy(retention).await;
            let before = f.snapshot().await;
            f.assert_public_denied(active[0]).await;
            assert_eq!(f.snapshot().await, before, "public denial must not trim content");
            assert!(matches!(f.write(0, 50, false).await, Err(StoreError::AuthorizationChanged)));
            assert_eq!(f.snapshot().await, before, "invalid staff proof must roll back archive and active changes");
            for actor in 60..63 {
                active.push(f.write(0, actor, true).await.expect("private capacity must not displace any active OP"));
                f.assert_active(&active).await;
            }
            let states: Vec<(i64, bool)> = sqlx::query_as("SELECT id,deleted FROM content.threads WHERE board=$1 AND id=ANY($2) ORDER BY id")
                .bind(&f.board).bind(&[expired, over_cap, retained][..]).fetch_all(&f.owner).await.unwrap();
            assert_eq!(states, vec![(expired, true), (over_cap, true), (retained, retention == 0)]);
            let mut original_posts = active[..5].to_vec();
            original_posts.extend(replies);
            original_posts.sort_unstable();
            let existing: Vec<i64> = sqlx::query_scalar("SELECT id FROM content.posts WHERE board=$1 AND id=ANY($2) AND NOT deleted ORDER BY id")
                .bind(&f.board).bind(&original_posts).fetch_all(&f.owner).await.unwrap();
            assert_eq!(existing, original_posts, "existing OPs and replies survive together");
            let protection: Vec<(bool, bool)> = sqlx::query_as("SELECT sticky,undead FROM content.threads WHERE board=$1 AND id=ANY($2) ORDER BY id")
                .bind(&f.board).bind(&active[..5]).fetch_all(&f.owner).await.unwrap();
            assert_eq!(protection, vec![(false,false),(false,false),(true,false),(false,true),(true,true)]);
            let listed = board_store::board_snapshot(&f.staff, &f.board, BoardSelection::All, Some(0)).await.unwrap();
            assert_eq!(listed.threads.len(), active.len());
            f.assert_public_denied(active[0]).await;
        }).await;
        f.cleanup().await;
        result.unwrap();
    }
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
    .expect("production staff writer must actually wait behind the board-row owner");
}

#[tokio::test]
async fn queued_staff_op_observes_committed_private_policy_in_both_directions() {
    let _serial = TEST.lock().await;
    for private_after in [true, false] {
        let f = Fixture::new().await;
        let work = f.clone();
        let result = tokio::spawn(async move {
            let f = work;
            let first = f.write(0, 100, true).await.unwrap();
            f.policy(3600).await;
            sqlx::query("UPDATE content.boards SET staff_only=$2 WHERE slug=$1")
                .bind(&f.board).bind(!private_after).execute(&f.owner).await.unwrap();
            let one = PgPoolOptions::new().max_connections(1)
                .connect(&std::env::var("STAFF_DATABASE_URL").unwrap()).await.unwrap();
            let (writer_pid, role): (i32, String) = sqlx::query_as("SELECT pg_backend_pid(),current_user::text")
                .fetch_one(&one).await.unwrap();
            assert_eq!(role, "board_staff");
            let mut lock = f.owner.begin().await.unwrap();
            let blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut *lock).await.unwrap();
            sqlx::query("UPDATE content.boards SET staff_only=$2 WHERE slug=$1")
                .bind(&f.board).bind(private_after).execute(&mut *lock).await.unwrap();
            let mut queued = f.clone();
            queued.staff = one.clone();
            let writer = tokio::spawn(async move { queued.write(0, 101, true).await });
            wait_behind(&f.owner, writer_pid, blocker_pid).await;
            lock.commit().await.unwrap();
            let second = tokio::time::timeout(Duration::from_secs(10), writer).await.unwrap().unwrap().unwrap();
            f.assert_active(&[second]).await;
            let (deleted, archived): (bool, bool) = sqlx::query_as("SELECT deleted,archived_at IS NOT NULL FROM content.threads WHERE board=$1 AND id=$2")
                .bind(&f.board).bind(first).fetch_one(&f.owner).await.unwrap();
            assert_eq!((deleted, archived), (false, !private_after));
            if private_after { f.assert_active(&[first, second]).await; }
            one.close().await;
        }).await;
        f.cleanup().await;
        result.unwrap();
    }
}

#[tokio::test]
async fn private_active_exemption_does_not_raise_complete_listing_ceiling() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let work = f.clone();
    let result = tokio::spawn(async move {
        let f = work;
        f.policy(0).await;
        // Metadata-only owned fixtures exercise the preflight cheaply. No
        // forged post/proof and no thousand production writes are necessary.
        assert_eq!(board_store::MAX_BOARD_READ_THREADS, 1000);
        sqlx::query("INSERT INTO content.threads(board) SELECT $1 FROM generate_series(1,1001)")
            .bind(&f.board).execute(&f.owner).await.unwrap();
        let created = f.write(0, 150, true).await.expect("private OP creation has no new active-thread ceiling");
        f.assert_active(&[created]).await;
        let active: i64 = sqlx::query_scalar("SELECT count(*) FROM content.threads WHERE board=$1 AND NOT deleted AND archived_at IS NULL")
            .bind(&f.board).fetch_one(&f.owner).await.unwrap();
        assert_eq!(active, 1002);
        assert!(matches!(board_store::board_snapshot(&f.staff, &f.board, BoardSelection::All, Some(0)).await, Err(StoreError::ReadLimit)));
        assert!(matches!(board_store::json_board_snapshot(&f.staff, &f.board, BoardSelection::All, 0).await, Err(StoreError::ReadLimit)));
    }).await;
    f.cleanup().await;
    result.unwrap();
}
