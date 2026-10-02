#![cfg(feature = "database-tests")]

use board_store::{NewPost, PostIdentityKeys, PostMetadata, PostingContext, StoreError};
use chrono::Utc;
use sqlx::PgPool;
use std::net::IpAddr;

// These fixtures exercise the shared capacity row and policy generation.
static POLICY_TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

struct Fixture {
    owner: PgPool,
    public: PgPool,
    boards: [String; 2],
    peer: IpAddr,
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
        let token: String =
            sqlx::query_scalar("SELECT substr(replace(gen_random_uuid()::text,'-',''),1,8)")
                .fetch_one(&owner)
                .await
                .unwrap();
        let boards = [format!("ca{token}"), format!("cb{token}")];
        for board in &boards {
            sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,comment_spoiler_cleanup) VALUES($1,'Owned content admission','Synthetic',2000,100,100,100,10,true)")
                .bind(board).execute(&owner).await.unwrap();
        }
        let peer = format!("2001:db8:ad:{}:{}::1", &token[..4], &token[4..])
            .parse()
            .unwrap();
        Self {
            owner,
            public,
            boards,
            peer,
        }
    }

    async fn rule(&self, pattern: &str) -> i64 {
        sqlx::query_scalar("INSERT INTO admission.rules(board,pattern) VALUES($1,$2) RETURNING id")
            .bind(&self.boards[0])
            .bind(pattern)
            .fetch_one(&self.owner)
            .await
            .unwrap()
    }

    async fn create(
        &self,
        board: usize,
        parent: i64,
        subject: &str,
        comment: &str,
    ) -> Result<i64, StoreError> {
        self.create_named(board, parent, "Anonymous", subject, comment)
            .await
    }

    async fn create_named(
        &self,
        board: usize,
        parent: i64,
        name: &str,
        subject: &str,
        comment: &str,
    ) -> Result<i64, StoreError> {
        board_store::create_post_with_metadata(
            &self.public,
            &self.boards[board],
            parent,
            &NewPost {
                name: name.into(),
                subject: subject.into(),
                comment: comment.into(),
                deletion_hash: "owned-test-hash".into(),
                sage: false,
            },
            None,
            PostingContext {
                request_start: Utc::now(),
                peer: Some(self.peer),
                op_password_proof: None,
            },
            PostMetadata {
                keys: PostIdentityKeys {
                    tripcode: None,
                    poster_id: None,
                },
                country_database: None,
                flag: "",
                options: "",
            },
        )
        .await
    }

    async fn count(&self, table: &str) -> i64 {
        let query = match table {
            "posts" => "SELECT count(*) FROM content.posts WHERE board=ANY($1)",
            "threads" => "SELECT count(*) FROM content.threads WHERE board=ANY($1)",
            _ => panic!("Unexpected owned fixture table"),
        };
        sqlx::query_scalar(query)
            .bind(self.boards.as_slice())
            .fetch_one(&self.owner)
            .await
            .unwrap()
    }

    async fn cleanup(self) {
        for query in [
            "DELETE FROM admission.rules WHERE board=ANY($1)",
            "DELETE FROM admission.hits WHERE board=ANY($1)",
            "DELETE FROM admission.logs WHERE board=ANY($1)",
            "DELETE FROM admission.bans WHERE board=ANY($1)",
            "DELETE FROM content.post_media WHERE post_id IN (SELECT id FROM content.posts WHERE board=ANY($1))",
            "DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=ANY($1))",
            "DELETE FROM content.posts WHERE board=ANY($1)",
            "DELETE FROM content.threads WHERE board=ANY($1)",
        ] {
            sqlx::query(query)
                .bind(self.boards.as_slice())
                .execute(&self.owner)
                .await
                .unwrap();
        }
        sqlx::query("DELETE FROM content.boards WHERE slug=ANY($1)")
            .bind(self.boards.as_slice())
            .execute(&self.owner)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn rules_receive_escaped_public_identity_without_private_trip_input() {
    let _guard = POLICY_TEST.lock().await;
    let f = Fixture::new().await;
    let rule = f.rule(r#"/postertrip\">!ozOtJW9BFA/u"#).await;
    sqlx::query("UPDATE admission.rules SET regex=true,log=true WHERE id=$1")
        .bind(rule)
        .execute(&f.owner)
        .await
        .unwrap();
    let post = f
        .create_named(0, 0, "Alice#password", "<raw>", "[code]<safe>&'\"[/code]")
        .await
        .unwrap();
    let (name, subject, comment): (String, String, String) =
        sqlx::query_as("SELECT name,subject,comment FROM admission.logs WHERE board=$1")
            .bind(&f.boards[0])
            .fetch_one(&f.owner)
            .await
            .unwrap();
    assert_eq!(name, r#"Alice</span> <span class="postertrip">!ozOtJW9BFA"#);
    assert!(!name.contains("password"));
    assert_eq!(subject, "&lt;raw&gt;");
    assert_eq!(comment, "&lt;safe&gt;&amp;&#039;&quot;");
    let published: (String, Option<String>) =
        sqlx::query_as("SELECT name,trip FROM content.posts WHERE id=$1")
            .bind(post)
            .fetch_one(&f.owner)
            .await
            .unwrap();
    assert_eq!(published, ("Alice".into(), Some("!ozOtJW9BFA".into())));
    // The raw secret alone cannot match a rule or enter a filter log.
    sqlx::query("UPDATE admission.rules SET regex=false,pattern='password',log=false WHERE id=$1")
        .bind(rule)
        .execute(&f.owner)
        .await
        .unwrap();
    f.create_named(0, 0, "Alice#password", "", "ordinary content")
        .await
        .unwrap();
    assert_eq!(f.count("posts").await, 2);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM admission.logs WHERE board=$1")
            .bind(&f.boards[0])
            .fetch_one(&f.owner)
            .await
            .unwrap(),
        1
    );
    f.cleanup().await;
}

#[tokio::test]
async fn exhausted_log_capacity_rolls_back_hit_update_and_post() {
    let _guard = POLICY_TEST.lock().await;
    let f = Fixture::new().await;
    let rule = f.rule("paper").await;
    sqlx::query("UPDATE admission.rules SET log=true WHERE id=$1")
        .bind(rule)
        .execute(&f.owner)
        .await
        .unwrap();
    f.create(0, 0, "", "paper").await.unwrap();
    sqlx::query("UPDATE admission.hits SET last_hit=clock_timestamp()-interval '3601 seconds' WHERE rule_id=$1")
        .bind(rule).execute(&f.owner).await.unwrap();
    let before: (i64, chrono::DateTime<Utc>) =
        sqlx::query_as("SELECT count,last_hit FROM admission.hits WHERE rule_id=$1")
            .bind(rule)
            .fetch_one(&f.owner)
            .await
            .unwrap();
    let old_limit: i32 =
        sqlx::query_scalar("SELECT log_limit FROM admission.capacity WHERE singleton")
            .fetch_one(&f.owner)
            .await
            .unwrap();
    sqlx::query("UPDATE admission.capacity SET log_limit=(SELECT count(*) FROM admission.logs)::integer WHERE singleton")
        .execute(&f.owner).await.unwrap();
    let denied = f.create(0, 0, "", "paper").await;
    // Restore the shared fixture setting before asserting on the result.
    sqlx::query("UPDATE admission.capacity SET log_limit=$1 WHERE singleton")
        .bind(old_limit)
        .execute(&f.owner)
        .await
        .unwrap();
    assert!(matches!(denied,Err(StoreError::Database(ref error))
        if error.as_database_error().unwrap().code().as_deref()==Some("53300")));
    let after: (i64, chrono::DateTime<Utc>) =
        sqlx::query_as("SELECT count,last_hit FROM admission.hits WHERE rule_id=$1")
            .bind(rule)
            .fetch_one(&f.owner)
            .await
            .unwrap();
    assert_eq!(after, before);
    assert_eq!(f.count("posts").await, 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM admission.logs WHERE board=$1")
            .bind(&f.boards[0])
            .fetch_one(&f.owner)
            .await
            .unwrap(),
        1
    );
    f.create(0, 0, "", "paper").await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count FROM admission.hits WHERE rule_id=$1")
            .bind(rule)
            .fetch_one(&f.owner)
            .await
            .unwrap(),
        before.0 + 1
    );
    f.cleanup().await;
}

#[tokio::test]
async fn ordered_reject_autosage_log_quiet_and_global_ban_are_persisted() {
    let _guard = POLICY_TEST.lock().await;
    let f = Fixture::new().await;
    let rule = f.rule("paper").await;
    sqlx::query("UPDATE admission.rules SET min_count=2 WHERE id=$1")
        .bind(rule)
        .execute(&f.owner)
        .await
        .unwrap();
    assert!(matches!(
        f.create(0, 0, "", "paperpaper").await,
        Err(StoreError::ContentRejected(_))
    ));
    assert_eq!(f.count("posts").await, 0);
    assert_eq!(f.count("threads").await, 0);
    let hits: i64 = sqlx::query_scalar("SELECT count FROM admission.hits WHERE rule_id=$1")
        .bind(rule)
        .fetch_one(&f.owner)
        .await
        .unwrap();
    assert_eq!(hits, 1);
    assert!(matches!(
        f.create(0, 0, "", "paperpaper").await,
        Err(StoreError::ContentRejected(_))
    ));
    let hits: i64 = sqlx::query_scalar("SELECT count FROM admission.hits WHERE rule_id=$1")
        .bind(rule)
        .fetch_one(&f.owner)
        .await
        .unwrap();
    assert_eq!(hits, 1);
    sqlx::query("UPDATE admission.rules SET min_count=1,autosage=true,log=true,quiet=true,ban_days=3 WHERE id=$1")
        .bind(rule).execute(&f.owner).await.unwrap();
    let thread = f.create(0, 0, "", "paper").await.unwrap();
    let (permasage, bump): (bool, chrono::DateTime<Utc>) =
        sqlx::query_as("SELECT permasage,bumped_at FROM content.threads WHERE id=$1")
            .bind(thread)
            .fetch_one(&f.owner)
            .await
            .unwrap();
    assert!(permasage);
    f.create(0, thread, "", "ordinary reply").await.unwrap();
    let next: chrono::DateTime<Utc> =
        sqlx::query_scalar("SELECT bumped_at FROM content.threads WHERE id=$1")
            .bind(thread)
            .fetch_one(&f.owner)
            .await
            .unwrap();
    assert_eq!(next, bump);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM admission.bans WHERE board=$1")
            .bind(&f.boards[0])
            .fetch_one(&f.owner)
            .await
            .unwrap(),
        0
    );
    sqlx::query("UPDATE admission.rules SET autosage=false WHERE id=$1")
        .bind(rule)
        .execute(&f.owner)
        .await
        .unwrap();
    let rejected_second = f.rule("paper").await;
    let logged = f
        .create(0, 0, "", "[spoiler]paper[/spoiler]")
        .await
        .unwrap();
    assert!(logged > thread);
    let saved: String = sqlx::query_scalar(
        "SELECT comment FROM admission.logs WHERE board=$1 ORDER BY id DESC LIMIT 1",
    )
    .bind(&f.boards[0])
    .fetch_one(&f.owner)
    .await
    .unwrap();
    assert_eq!(saved, "paper");
    assert!(matches!(
        f.create(0, 0, "Administrator", "paper").await,
        Err(StoreError::ContentRejected(_))
    ));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM admission.logs WHERE board=$1")
            .bind(&f.boards[0])
            .fetch_one(&f.owner)
            .await
            .unwrap(),
        2
    );
    sqlx::query("UPDATE admission.rules SET active=false WHERE id=$1")
        .bind(rule)
        .execute(&f.owner)
        .await
        .unwrap();
    sqlx::query("UPDATE admission.rules SET quiet=true WHERE id=$1")
        .bind(rejected_second)
        .execute(&f.owner)
        .await
        .unwrap();
    let before = f.count("posts").await;
    assert!(
        matches!(f.create(0,0,"","paper").await,Err(StoreError::ContentQuiet {post}) if post==thread)
    );
    assert!(
        matches!(f.create(0,thread,"","paper").await,Err(StoreError::ContentQuiet {post}) if post==logged+1)
    );
    assert_eq!(f.count("posts").await, before);
    assert!(matches!(
        f.create(0, i64::MAX, "", "paper").await,
        Err(StoreError::NotFound)
    ));
    sqlx::query("UPDATE admission.rules SET ban_days=3 WHERE id=$1")
        .bind(rejected_second)
        .execute(&f.owner)
        .await
        .unwrap();
    assert!(matches!(
        f.create(0, 0, "", "paper").await,
        Err(StoreError::ContentQuiet { .. })
    ));
    assert!(matches!(
        f.create(1, 0, "", "ordinary content on another board")
            .await,
        Err(StoreError::ContentRejected(_))
    ));
    let (active, expiry): (bool, Option<chrono::DateTime<Utc>>) =
        sqlx::query_as("SELECT active,expires_at FROM admission.bans WHERE board=$1")
            .bind(&f.boards[0])
            .fetch_one(&f.owner)
            .await
            .unwrap();
    assert!(active);
    assert!(expiry.unwrap() > Utc::now());
    sqlx::query(
        "UPDATE admission.bans SET expires_at=clock_timestamp()-interval '1 second' WHERE board=$1",
    )
    .bind(&f.boards[0])
    .execute(&f.owner)
    .await
    .unwrap();
    f.create(1, 0, "", "allowed after expiry").await.unwrap();
    f.cleanup().await;
}

#[tokio::test]
async fn invalid_policy_and_capacity_failures_preserve_post_and_effect_state() {
    let _guard = POLICY_TEST.lock().await;
    let f = Fixture::new().await;
    let rule = f.rule("/incomplete").await;
    sqlx::query("UPDATE admission.rules SET regex=true WHERE id=$1")
        .bind(rule)
        .execute(&f.owner)
        .await
        .unwrap();
    assert!(matches!(
        f.create(0, 0, "", "ordinary content").await,
        Err(StoreError::Database(_))
    ));
    assert_eq!(f.count("posts").await, 0);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM admission.hits WHERE board=$1")
            .bind(&f.boards[0])
            .fetch_one(&f.owner)
            .await
            .unwrap(),
        0
    );
    sqlx::query("UPDATE admission.rules SET pattern='paper',regex=false,log=true WHERE id=$1")
        .bind(rule)
        .execute(&f.owner)
        .await
        .unwrap();
    f.create(0, 0, "", "paper").await.unwrap();
    // A held private capacity row forces a real runtime SQL timeout. It cannot
    // become an allow decision or create a partial post or another hit/log.
    let mut lock = f.owner.begin().await.unwrap();
    sqlx::query("SELECT singleton FROM admission.capacity WHERE singleton FOR UPDATE")
        .execute(&mut *lock)
        .await
        .unwrap();
    let public = f.public.clone();
    let board = f.boards[0].clone();
    let peer = f.peer;
    let pending = tokio::spawn(async move {
        board_store::create_post_with_context(
            &public,
            &board,
            0,
            &NewPost {
                name: "Anonymous".into(),
                subject: "".into(),
                comment: "paper".into(),
                deletion_hash: "owned".into(),
                sage: false,
            },
            None,
            PostingContext {
                request_start: Utc::now(),
                peer: Some(peer),
                op_password_proof: None,
            },
        )
        .await
    });
    assert!(matches!(
        pending.await.unwrap(),
        Err(StoreError::Database(_))
    ));
    lock.rollback().await.unwrap();
    assert_eq!(f.count("posts").await, 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM admission.logs WHERE board=$1")
            .bind(&f.boards[0])
            .fetch_one(&f.owner)
            .await
            .unwrap(),
        1
    );
    f.cleanup().await;
}

#[tokio::test]
async fn rule_updates_wait_for_snapshot_and_stale_effects_are_denied() {
    let _guard = POLICY_TEST.lock().await;
    let f = Fixture::new().await;
    let rule = f.rule("paper").await;
    let mut snapshot = f.public.begin().await.unwrap();
    let (revision, _): (i64, bool) =
        sqlx::query_as("SELECT * FROM content.lock_content_admission($1,$2)")
            .bind(&f.boards[0])
            .bind(f.peer.to_string())
            .fetch_one(&mut *snapshot)
            .await
            .unwrap();
    let mut update = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL lock_timeout='1s'")
        .execute(&mut *update)
        .await
        .unwrap();
    let error = sqlx::query("UPDATE admission.rules SET quiet=true WHERE id=$1")
        .bind(rule)
        .execute(&mut *update)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("55P03")
    );
    update.rollback().await.unwrap();
    snapshot.rollback().await.unwrap();
    sqlx::query("UPDATE admission.rules SET quiet=true WHERE id=$1")
        .bind(rule)
        .execute(&f.owner)
        .await
        .unwrap();
    let error = sqlx::query(
        "SELECT content.record_content_admission($1,$2,$3,$4,'quiet',0,'Anonymous','','paper','')",
    )
    .bind(&f.boards[0])
    .bind(f.peer.to_string())
    .bind(revision)
    .bind(rule)
    .execute(&f.public)
    .await
    .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("55000")
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM admission.hits WHERE board=$1")
            .bind(&f.boards[0])
            .fetch_one(&f.owner)
            .await
            .unwrap(),
        0
    );
    assert!(matches!(
        f.create(0, 0, "", "paper").await,
        Err(StoreError::ContentQuiet { .. })
    ));
    assert_eq!(f.count("posts").await, 0);
    f.cleanup().await;
}

#[tokio::test]
async fn filename_proxy_uses_authenticated_approved_metadata_and_byte_semantics() {
    let _guard = POLICY_TEST.lock().await;
    let f = Fixture::new().await;
    let intake = board_store::media_intake::IntakeStore::connect(
        &std::env::var("INTAKE_DATABASE_URL").unwrap(),
    )
    .await
    .unwrap();
    let queue =
        board_store::media::MediaQueue::connect(&std::env::var("MEDIA_DATABASE_URL").unwrap())
            .await
            .unwrap();
    let upload = intake.reserve("phpé").await.unwrap();
    let id = upload.id.clone();
    let error = sqlx::query("SELECT content.attachment_upload_filename($1,$2)")
        .bind(&id)
        .bind(&upload.capability)
        .execute(&f.public)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("P0001")
    );
    let error = sqlx::query("SELECT content.attachment_upload_filename($1,$2)")
        .bind(&id)
        .bind("0".repeat(64))
        .execute(&f.public)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().unwrap().code().as_deref(),
        Some("P0002")
    );
    intake.begin_upload(&id, &upload.capability).await.unwrap();
    intake
        .finish_upload(&id, &upload.capability, 100)
        .await
        .unwrap();
    let claim = queue.claim().await.unwrap().unwrap();
    assert_eq!(claim.id, id, "Requires the owned idle metadata queue");
    let lease = claim.lease_token.unwrap();
    // Exercise the real queue/approval protocol with synthetic metadata. This
    // test is not decoder, file publication or VM containment qualification.
    let output = queue
        .prepare_output(
            &id,
            &lease,
            &board_store::media_assets::OutputMetadata {
                sha256: "a".repeat(64),
                bytes: 123,
                width: 10,
                height: 20,
            },
        )
        .await
        .unwrap();
    queue.approve_output(&id, &lease, &output.id).await.unwrap();
    let attachment = board_store::post_media::NewAttachment {
        upload,
        spoiler: false,
    };
    let result = board_store::create_post_with_context(
        &f.public,
        &f.boards[0],
        0,
        &NewPost {
            name: "Anonymous".into(),
            subject: "".into(),
            comment: "ordinary content".into(),
            deletion_hash: "owned".into(),
            sage: false,
        },
        Some(&attachment),
        PostingContext {
            request_start: Utc::now(),
            peer: Some(f.peer),
            op_password_proof: None,
        },
    )
    .await;
    assert!(
        matches!(result,Err(StoreError::ContentRejected(message)) if message=="Error: Abnormal reply.")
    );
    assert_eq!(f.count("posts").await, 0);
    assert_eq!(f.count("threads").await, 0);
    let reason: String = sqlx::query_scalar("SELECT reason FROM admission.bans WHERE board=$1")
        .bind(&f.boards[0])
        .fetch_one(&f.owner)
        .await
        .unwrap();
    assert_eq!(reason, "filename");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM media_intake.handles WHERE job_id=$1")
            .bind(&id)
            .fetch_one(&f.owner)
            .await
            .unwrap(),
        1
    );
    for query in [
        "DELETE FROM media.assets WHERE job_id=$1",
        "DELETE FROM media_intake.handles WHERE job_id=$1",
        "DELETE FROM media.jobs WHERE id=$1",
    ] {
        sqlx::query(query)
            .bind(&id)
            .execute(&f.owner)
            .await
            .unwrap();
    }
    f.cleanup().await;
}

#[tokio::test]
async fn runtime_credentials_cannot_read_policy_state_or_invent_rule_actions() {
    let _guard = POLICY_TEST.lock().await;
    let f = Fixture::new().await;
    let rule = f.rule("paper").await;
    for query in [
        "SELECT * FROM admission.rules",
        "SELECT * FROM admission.hits",
        "SELECT * FROM admission.logs",
        "SELECT * FROM admission.bans",
        "UPDATE admission.rules SET active=false",
        "DELETE FROM admission.policy",
        "SELECT * FROM staff_identity.accounts",
        "UPDATE content.threads SET permasage=true",
    ] {
        let mut healthy = f.owner.begin().await.unwrap();
        sqlx::query(query).execute(&mut *healthy).await.unwrap();
        healthy.rollback().await.unwrap();
        let error = sqlx::query(query).execute(&f.public).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501"),
            "{query}"
        );
    }
    // Healthy scoped reader, with no private board policy disclosure.
    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM content.content_admission_rules($1)")
        .bind(&f.boards[0])
        .fetch_one(&f.public)
        .await
        .unwrap();
    assert_eq!(rows, 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM content.content_admission_rules('j')")
            .fetch_one(&f.public)
            .await
            .unwrap(),
        0
    );
    assert!(
        sqlx::query("SELECT * FROM content.lock_content_admission('j',NULL)")
            .execute(&f.public)
            .await
            .is_err()
    );
    let revision: i64 = sqlx::query_scalar("SELECT revision FROM admission.policy")
        .fetch_one(&f.owner)
        .await
        .unwrap();
    for kind in [
        Some("autosage"),
        Some("log"),
        Some("quiet"),
        Some("filename"),
        None,
    ] {
        assert!(sqlx::query("SELECT content.record_content_admission($1,$2,$3,$4,$5,0,'Anonymous','','paper','ordinary.png')")
            .bind(&f.boards[0]).bind(f.peer.to_string()).bind(revision).bind(rule).bind(kind)
            .execute(&f.public).await.is_err(),"{kind:?}");
    }
    assert!(sqlx::query("SELECT content.record_content_admission($1,$2,NULL,$3,'reject',0,'Anonymous','','paper','')")
        .bind(&f.boards[0]).bind(f.peer.to_string()).bind(rule)
        .execute(&f.public).await.is_err());
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM admission.bans WHERE board=$1")
            .bind(&f.boards[0])
            .fetch_one(&f.owner)
            .await
            .unwrap(),
        0
    );
    let mut restricted = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_admission_owner")
        .execute(&mut *restricted)
        .await
        .unwrap();
    assert!(
        sqlx::query("SELECT * FROM staff_identity.accounts")
            .execute(&mut *restricted)
            .await
            .is_err()
    );
    restricted.rollback().await.unwrap();
    let no_login: bool = sqlx::query_scalar("SELECT NOT(rolcanlogin OR rolsuper OR rolcreatedb OR rolcreaterole OR rolbypassrls) FROM pg_roles WHERE rolname='board_admission_owner'")
        .fetch_one(&f.owner).await.unwrap();
    assert!(no_login);
    f.cleanup().await;
}

#[tokio::test]
async fn leniency_uses_locked_anonymous_state_and_survives_cross_board_concurrency() {
    let _guard = POLICY_TEST.lock().await;
    use board_domain::anonymous_session::Capability;
    use board_store::{AnonymousPostingContext, anonymous_session::PostingSession};
    let f = Fixture::new().await;
    let capability = Capability::generate().unwrap();
    let fingerprints = capability.fingerprints(Some(f.peer), *b"XX");
    let session = PostingSession {
        fingerprints,
        minted: true,
        now: Utc::now(),
    };
    let create = |board: usize, session: PostingSession| {
        let public = f.public.clone();
        let board = f.boards[board].clone();
        let peer = f.peer;
        async move {
            board_store::create_post_with_anonymous_session(
                &public,
                &board,
                0,
                &NewPost {
                    name: "Anonymous".into(),
                    subject: "".into(),
                    comment: "paper".into(),
                    deletion_hash: "owned".into(),
                    sage: false,
                },
                None,
                AnonymousPostingContext {
                    posting: PostingContext {
                        request_start: Utc::now(),
                        peer: Some(peer),
                        op_password_proof: None,
                    },
                    session,
                },
                PostMetadata {
                    keys: PostIdentityKeys {
                        tripcode: None,
                        poster_id: None,
                    },
                    country_database: None,
                    flag: "",
                    options: "",
                },
            )
            .await
        }
    };
    create(0, session).await.unwrap();
    let rule = f.rule("paper").await;
    sqlx::query("UPDATE admission.rules SET lenient=true WHERE id=$1")
        .bind(rule)
        .execute(&f.owner)
        .await
        .unwrap();
    sqlx::query("UPDATE post_secrets.anonymous_sessions SET created_at=created_at-86400,network_at=network_at-86400,address_at=address_at-86400,environment_at=environment_at-86400,posts=10,pending=0 WHERE token_hash=$1")
        .bind(fingerprints.token.as_slice()).execute(&f.owner).await.unwrap();
    let session = PostingSession {
        minted: false,
        now: Utc::now(),
        ..session
    };
    assert!(matches!(
        create(0, session).await,
        Err(StoreError::ContentRejected(_))
    ));
    sqlx::query("UPDATE post_secrets.anonymous_sessions SET posts=11 WHERE token_hash=$1")
        .bind(fingerprints.token.as_slice())
        .execute(&f.owner)
        .await
        .unwrap();
    let (a, b) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        tokio::join!(create(0, session), create(1, session))
    })
    .await
    .unwrap();
    a.unwrap();
    b.unwrap();
    assert_eq!(f.count("posts").await, 3);
    sqlx::query("UPDATE post_secrets.anonymous_sessions SET expires_at=extract(epoch FROM clock_timestamp())::bigint-1 WHERE token_hash=$1")
        .bind(fingerprints.token.as_slice()).execute(&f.owner).await.unwrap();
    assert!(matches!(
        create(0, session).await,
        Err(StoreError::AuthorizationChanged)
    ));
    assert_eq!(f.count("posts").await, 3);
    sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
        .bind(fingerprints.token.as_slice())
        .execute(&f.owner)
        .await
        .unwrap();
    f.cleanup().await;
}
