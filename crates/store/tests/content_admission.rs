#![cfg(feature = "database-tests")]
mod support;

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
        support::share_key(&boards[1], &boards[0]);
        for board in &boards {
            sqlx::query("INSERT INTO content.boards(captcha_enabled,slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,comment_spoiler_cleanup,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) VALUES(false,$1,'Owned content admission','Synthetic',2000,100,100,100,10,true,0,0,0)")
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
        self.create_options(board, parent, (name, subject, comment), "")
            .await
    }

    async fn create_options(
        &self,
        board: usize,
        parent: i64,
        fields: (&str, &str, &str),
        options: &str,
    ) -> Result<i64, StoreError> {
        let (name, subject, comment) = fields;
        support::create_post_with_metadata(
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
                drawing: None,
                spoiler: false,
                keys: PostIdentityKeys {
                    tripcode: None,
                    poster_id: None,
                },
                country_database: None,
                flag: "",
                options,
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
            "DELETE FROM admission.network_rules WHERE boards=ANY($1)",
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
async fn network_policy_updates_wait_for_the_shared_admission_generation() {
    let _guard = POLICY_TEST.lock().await;
    let f = Fixture::new().await;
    let id: i64 = sqlx::query_scalar("INSERT INTO admission.network_rules(boards,range_start,range_end) VALUES($1,0,4294967295) RETURNING id")
        .bind(&f.boards[0]).fetch_one(&f.owner).await.unwrap();
    let mut held = f.public.begin().await.unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *held)
        .await
        .unwrap();
    let before: i64 =
        sqlx::query_scalar("SELECT revision FROM content.lock_content_admission($1,$2)")
            .bind(&f.boards[0])
            .bind(f.peer.to_string())
            .fetch_one(&mut *held)
            .await
            .unwrap();
    let update = {
        let owner = f.owner.clone();
        tokio::spawn(async move {
            sqlx::query("UPDATE admission.network_rules SET images_only=true WHERE id=$1")
                .bind(id)
                .execute(&owner)
                .await
        })
    };
    let observed = tokio::time::timeout(std::time::Duration::from_secs(5), async { loop {
        let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE NOT granted AND $1=ANY(pg_blocking_pids(pid)))")
            .bind(pid).fetch_one(&f.owner).await.unwrap();
        if waiting { break; } tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }}).await;
    held.rollback().await.unwrap();
    update.await.unwrap().unwrap();
    observed.expect("Network update waited on the same policy generation");
    let after: i64 = sqlx::query_scalar("SELECT revision FROM admission.policy WHERE singleton")
        .fetch_one(&f.owner)
        .await
        .unwrap();
    assert_eq!(after, before + 1);
    let changed: bool = sqlx::query_scalar(
        "SELECT images_only FROM content.network_admission_rules($1,$2) WHERE id=$3",
    )
    .bind(&f.boards[0])
    .bind(Utc::now().timestamp())
    .bind(id)
    .fetch_one(&f.public)
    .await
    .unwrap();
    assert!(changed);
    let updated: i64 = sqlx::query_scalar(
        "SELECT updated_at FROM content.network_admission_rules($1,$2) WHERE id=$3",
    )
    .bind(&f.boards[0])
    .bind(Utc::now().timestamp())
    .bind(id)
    .fetch_one(&f.public)
    .await
    .unwrap();
    assert!(updated > 0 && updated <= Utc::now().timestamp());
    sqlx::query("UPDATE admission.network_rules SET updated_at=123 WHERE id=$1")
        .bind(id)
        .execute(&f.owner)
        .await
        .unwrap();
    let imported: i64 =
        sqlx::query_scalar("SELECT updated_at FROM admission.network_rules WHERE id=$1")
            .bind(id)
            .fetch_one(&f.owner)
            .await
            .unwrap();
    assert_eq!(
        imported, 123,
        "Explicit operator timestamp correction is retained"
    );
    sqlx::query("UPDATE admission.network_rules SET images_only=false WHERE id=$1")
        .bind(id)
        .execute(&f.owner)
        .await
        .unwrap();
    let refreshed: i64 =
        sqlx::query_scalar("SELECT updated_at FROM admission.network_rules WHERE id=$1")
            .bind(id)
            .fetch_one(&f.owner)
            .await
            .unwrap();
    assert!(refreshed > imported && refreshed <= Utc::now().timestamp());
    f.cleanup().await;
}

#[tokio::test]
async fn source_network_stage_preserves_precedence_and_rolls_back_incomplete_post_effects() {
    let _guard = POLICY_TEST.lock().await;
    let mut f = Fixture::new().await;
    f.peer = "192.0.2.44".parse().unwrap();
    let parent = f.create(0, 0, "", "Owned network parent").await.unwrap();
    let content = f.rule("network-owned").await;
    sqlx::query("UPDATE admission.rules SET log=true WHERE id=$1")
        .bind(content)
        .execute(&f.owner)
        .await
        .unwrap();
    let range: i64 = sqlx::query_scalar("INSERT INTO admission.network_rules(boards,range_start,range_end) VALUES($1,3221226028,3221226028) RETURNING id")
        .bind(&f.boards[0]).fetch_one(&f.owner).await.unwrap();
    let rejected = f.create(0, parent, "", "network-owned").await;
    assert!(
        matches!(rejected, Err(StoreError::ContentRejected(message)) if message.starts_with("Posting from your IP range"))
    );
    assert_eq!(f.count("posts").await, 1);
    let effects: i64 = sqlx::query_scalar("SELECT (SELECT count(*) FROM admission.logs WHERE rule_id=$1)+(SELECT count(*) FROM admission.hits WHERE rule_id=$1)")
        .bind(content).fetch_one(&f.owner).await.unwrap();
    assert_eq!(
        effects, 0,
        "Later network rejection rolls back prior log effects"
    );
    assert!(
        matches!(f.create_named(0,parent,"moot Ep8pui8Vw2","","ordinary text").await,
        Err(StoreError::ContentRejected(message)) if message.starts_with("Posting from your IP range"))
    );

    sqlx::query("UPDATE admission.rules SET log=false WHERE id=$1")
        .bind(content)
        .execute(&f.owner)
        .await
        .unwrap();
    assert!(matches!(f.create(0,parent,"","network-owned").await,
        Err(StoreError::ContentRejected(message)) if message.contains("system thinks your post is spam")));
    sqlx::query("UPDATE admission.rules SET quiet=true WHERE id=$1")
        .bind(content)
        .execute(&f.owner)
        .await
        .unwrap();
    assert!(matches!(
        f.create(0, parent, "", "network-owned").await,
        Err(StoreError::ContentQuiet { .. })
    ));
    assert_eq!(f.count("posts").await, 1);
    sqlx::query("DELETE FROM admission.rules WHERE id=$1")
        .bind(content)
        .execute(&f.owner)
        .await
        .unwrap();
    sqlx::query("UPDATE admission.network_rules SET ops_only=true WHERE id=$1")
        .bind(range)
        .execute(&f.owner)
        .await
        .unwrap();
    f.create(0, parent, "", "Allowed reply under OP-only rule")
        .await
        .unwrap();
    assert!(matches!(f.create(0,0,"","Blocked OP").await,
        Err(StoreError::ContentRejected(message)) if message.starts_with("Posting threads")));
    sqlx::query("UPDATE admission.network_rules SET ops_only=false,images_only=true WHERE id=$1")
        .bind(range)
        .execute(&f.owner)
        .await
        .unwrap();
    f.create(0, parent, "", "No image under image-only rule")
        .await
        .unwrap();
    sqlx::query("UPDATE admission.network_rules SET images_only=false,expires_at=1 WHERE id=$1")
        .bind(range)
        .execute(&f.owner)
        .await
        .unwrap();
    f.create(0, parent, "", "Expired policy is inactive")
        .await
        .unwrap();
    sqlx::query("UPDATE admission.network_rules SET expires_at=0,range_start=NULL,range_end=NULL,asn=64512 WHERE id=$1")
        .bind(range).execute(&f.owner).await.unwrap();
    let count = f.count("posts").await;
    assert!(matches!(
        f.create(0, parent, "", "Unavailable trusted ASN provider")
            .await,
        Err(StoreError::Database(_))
    ));
    assert_eq!(f.count("posts").await, count);
    sqlx::query("UPDATE admission.network_rules SET reports_only=true WHERE id=$1")
        .bind(range)
        .execute(&f.owner)
        .await
        .unwrap();
    f.create(
        0,
        parent,
        "",
        "Report-only rules do not require posting providers",
    )
    .await
    .unwrap();
    for query in [
        "SELECT * FROM admission.network_rules",
        "UPDATE admission.network_rules SET active=false",
    ] {
        assert!(sqlx::query(query).execute(&f.public).await.is_err());
    }
    f.cleanup().await;
}

#[tokio::test]
async fn generated_results_reach_rules_before_final_blank_admission() {
    let _guard = POLICY_TEST.lock().await;
    let f = Fixture::new().await;
    sqlx::query("UPDATE content.boards SET dice_roll=true,fortune_trip=true WHERE slug=$1")
        .bind(&f.boards[0])
        .execute(&f.owner)
        .await
        .unwrap();
    let parent = f.create(0, 0, "", "ordinary parent").await.unwrap();
    let rule = f.rule("rolled").await;
    for parent in [0, parent] {
        let result = f
            .create_options(0, parent, ("Anonymous", "", ""), "dice+1d1")
            .await;
        assert!(matches!(result, Err(StoreError::ContentRejected(_))));
    }
    assert_eq!(f.count("posts").await, 1);
    sqlx::query("UPDATE admission.rules SET log=true WHERE id=$1")
        .bind(rule)
        .execute(&f.owner)
        .await
        .unwrap();
    let post = f
        .create_options(0, 0, ("Anonymous", "", ""), "dice+1d1")
        .await
        .unwrap();
    let saved: (String, Option<String>) =
        sqlx::query_as("SELECT comment,dice_result FROM content.posts WHERE id=$1")
            .bind(post)
            .fetch_one(&f.owner)
            .await
            .unwrap();
    assert_eq!(
        saved,
        (
            "<b>Rolled 1 (1d1)<br><br></b>".into(),
            Some("Rolled 1 (1d1)".into())
        )
    );
    // Removing the sole generated content must recheck the final persisted row.
    let error = sqlx::query("UPDATE content.posts SET dice_result=NULL WHERE id=$1")
        .bind(post)
        .execute(&f.owner)
        .await
        .unwrap_err();
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.constraint()),
        Some("posts_generated_payload_check")
    );
    let logged: String = sqlx::query_scalar(
        "SELECT comment FROM admission.logs WHERE rule_id=$1 ORDER BY id DESC LIMIT 1",
    )
    .bind(rule)
    .fetch_one(&f.owner)
    .await
    .unwrap();
    assert_eq!(logged, "<b>Rolled 1 (1d1)<br><br></b>");
    let random_dice = f
        .create_options(0, 0, ("Anonymous", "", ""), "dice+25d1000")
        .await
        .unwrap();
    let saved_dice: String =
        sqlx::query_scalar("SELECT dice_result FROM content.posts WHERE id=$1")
            .bind(random_dice)
            .fetch_one(&f.owner)
            .await
            .unwrap();
    let logged: String = sqlx::query_scalar(
        "SELECT comment FROM admission.logs WHERE rule_id=$1 ORDER BY id DESC LIMIT 1",
    )
    .bind(rule)
    .fetch_one(&f.owner)
    .await
    .unwrap();
    assert_eq!(logged, format!("<b>{saved_dice}<br><br></b>"));
    sqlx::query("UPDATE admission.rules SET pattern='/Your fortune:/',regex=true,log=false,quiet=true WHERE id=$1")
        .bind(rule).execute(&f.owner).await.unwrap();
    let quiet = f
        .create_options(0, 0, ("Anonymous", "", ""), "fortune")
        .await;
    assert!(matches!(quiet, Err(StoreError::ContentQuiet { .. })));
    assert_eq!(f.count("posts").await, 3);
    sqlx::query("UPDATE admission.rules SET quiet=false,log=true WHERE id=$1")
        .bind(rule)
        .execute(&f.owner)
        .await
        .unwrap();
    let fortune = f
        .create_options(0, 0, ("Anonymous", "", ""), "fortune")
        .await
        .unwrap();
    let saved: (String, Option<String>, Option<String>) =
        sqlx::query_as("SELECT comment,fortune_text,fortune_color FROM content.posts WHERE id=$1")
            .bind(fortune)
            .fetch_one(&f.owner)
            .await
            .unwrap();
    assert!(saved.0.starts_with("<span class=\"fortune\""));
    assert!(saved.1.is_some());
    assert!(saved.2.is_some());
    let logged: String = sqlx::query_scalar(
        "SELECT comment FROM admission.logs WHERE rule_id=$1 ORDER BY id DESC LIMIT 1",
    )
    .bind(rule)
    .fetch_one(&f.owner)
    .await
    .unwrap();
    assert_eq!(
        logged,
        format!(
            "<span class=\"fortune\" style=\"color:{}\"><br><br><b>Your fortune: {}</b></span>",
            saved.2.as_deref().unwrap(),
            saved.1.as_deref().unwrap()
        )
    );
    let error =
        sqlx::query("UPDATE content.posts SET fortune_text=NULL,fortune_color=NULL WHERE id=$1")
            .bind(fortune)
            .execute(&f.owner)
            .await
            .unwrap_err();
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.constraint()),
        Some("posts_generated_payload_check")
    );
    let safe: bool = sqlx::query_scalar("SELECT NOT has_function_privilege('board_public','content.require_attachment_for_empty_post()','EXECUTE') AND NOT has_schema_privilege('board_attachment_owner','content','CREATE')")
        .fetch_one(&f.owner).await.unwrap();
    assert!(safe);
    let mut readiness = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_thread_transfer_owner")
        .execute(&mut *readiness)
        .await
        .unwrap();
    let ready: bool = sqlx::query_scalar("SELECT content.text_thread_transfer_ready()")
        .fetch_one(&mut *readiness)
        .await
        .unwrap();
    assert!(ready);
    readiness.rollback().await.unwrap();
    assert!(matches!(
        f.create(0, 0, "", "").await,
        Err(StoreError::Invalid(_))
    ));
    f.cleanup().await;
}

#[tokio::test]
async fn native_invalid_unicode_preserves_posts_and_admission_effects() {
    let _guard = POLICY_TEST.lock().await;
    let f = Fixture::new().await;
    // The fixed name check is active even without configured content rules.
    assert!(matches!(
        f.create_named(0, 0, "\u{10000}\u{309d}", "", "ordinary").await,
        Err(StoreError::Database(sqlx::Error::Protocol(message)))
            if message == "Content admission is unavailable."
    ));
    assert_eq!(f.count("posts").await, 0);
    assert_eq!(f.count("threads").await, 0);
    let rule = f.rule("paper").await;
    sqlx::query("UPDATE admission.rules SET log=true,autosage=true WHERE id=$1")
        .bind(rule)
        .execute(&f.owner)
        .await
        .unwrap();
    assert!(matches!(
        f.create(0, 0, "", "\u{10000}\u{309d}").await,
        Err(StoreError::Database(sqlx::Error::Protocol(message)))
            if message == "Content admission is unavailable."
    ));
    assert_eq!(f.count("posts").await, 0);
    assert_eq!(f.count("threads").await, 0);
    for query in [
        "SELECT count(*) FROM admission.hits WHERE board=ANY($1)",
        "SELECT count(*) FROM admission.logs WHERE board=ANY($1)",
        "SELECT count(*) FROM admission.bans WHERE board=ANY($1)",
    ] {
        assert_eq!(
            sqlx::query_scalar::<_, i64>(query)
                .bind(f.boards.as_slice())
                .fetch_one(&f.owner)
                .await
                .unwrap(),
            0
        );
    }
    f.create_named(0, 0, "\u{10000}", "", "\u{20000}\u{309d}")
        .await
        .unwrap();
    assert_eq!(f.count("posts").await, 1);
    assert_eq!(f.count("threads").await, 1);
    f.cleanup().await;
}

#[tokio::test]
async fn fixed_trip_checks_follow_configured_actions_without_reading_private_passwords() {
    let _guard = POLICY_TEST.lock().await;
    let f = Fixture::new().await;
    for name in ["moot Ep8pui8Vw2", "mοоt Ep8pui8Vw2"] {
        assert!(matches!(f.create_named(0,0,name,"","").await,
            Err(StoreError::ContentRejected(message)) if message == "Error: Upload failed."));
    }
    assert_eq!(f.count("posts").await, 0);
    for name in ["moot#Ep8pui8Vw2", "moot#l0l1c0m"] {
        f.create_named(0, 0, name, "", &format!("ordinary content {name}"))
            .await
            .unwrap();
    }
    // The earlier fixed subject decision wins over the later name/trip stage.
    assert!(
        matches!(f.create_named(0,0,"moot Ep8pui8Vw2","Administrator","ordinary").await,
        Err(StoreError::ContentRejected(message)) if message == "You can't post with that subject.")
    );
    let rule = f.rule("moot").await;
    assert!(matches!(f.create_named(0,0,"moot Ep8pui8Vw2","","").await,
        Err(StoreError::ContentRejected(message)) if message.contains("Please reformat and try again.")));
    sqlx::query("UPDATE admission.rules SET quiet=true WHERE id=$1")
        .bind(rule)
        .execute(&f.owner)
        .await
        .unwrap();
    assert!(matches!(
        f.create_named(0, 0, "moot Ep8pui8Vw2", "", "").await,
        Err(StoreError::ContentQuiet { .. })
    ));
    assert_eq!(f.count("posts").await, 2);
    for autosage in [false, true] {
        sqlx::query("UPDATE admission.rules SET quiet=false,log=true,autosage=$2 WHERE id=$1")
            .bind(rule)
            .bind(autosage)
            .execute(&f.owner)
            .await
            .unwrap();
        let allowed = f
            .create_named(0, 0, "moot", "", &format!("ordinary {autosage}"))
            .await
            .unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, bool>("SELECT permasage FROM content.threads WHERE id=$1")
                .bind(allowed)
                .fetch_one(&f.owner)
                .await
                .unwrap(),
            autosage
        );
        for query in [
            "DELETE FROM admission.hits WHERE rule_id=$1",
            "DELETE FROM admission.logs WHERE rule_id=$1",
        ] {
            sqlx::query(query)
                .bind(rule)
                .execute(&f.owner)
                .await
                .unwrap();
        }
        let posts = f.count("posts").await;
        let threads = f.count("threads").await;
        assert!(matches!(f.create_named(0,0,"moot Ep8pui8Vw2","","").await,
            Err(StoreError::ContentRejected(message)) if message == "Error: Upload failed."));
        assert_eq!(f.count("posts").await, posts);
        assert_eq!(f.count("threads").await, threads);
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM admission.hits WHERE rule_id=$1")
                .bind(rule)
                .fetch_one(&f.owner)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM admission.logs WHERE rule_id=$1")
                .bind(rule)
                .fetch_one(&f.owner)
                .await
                .unwrap(),
            0
        );
    }
    f.cleanup().await;
}

#[tokio::test]
async fn rules_precede_final_blank_checks_but_follow_required_subject_and_line_checks() {
    let _guard = POLICY_TEST.lock().await;
    let f = Fixture::new().await;
    let thread = f.create(0, 0, "", "existing thread").await.unwrap();
    let rule = f.rule("anonymous").await;
    for comment in ["", "[spoiler] [/spoiler]", "😀", "~?rep?~"] {
        for parent in [0, thread] {
            assert!(matches!(
                f.create(0, parent, "", comment).await,
                Err(StoreError::ContentRejected(_))
            ));
        }
    }
    assert_eq!(f.count("posts").await, 1);
    assert_eq!(f.count("threads").await, 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count FROM admission.hits WHERE rule_id=$1")
            .bind(rule)
            .fetch_one(&f.owner)
            .await
            .unwrap(),
        1
    );
    sqlx::query("UPDATE content.boards SET text_only=true WHERE slug=$1")
        .bind(&f.boards[0])
        .execute(&f.owner)
        .await
        .unwrap();
    assert!(matches!(
        f.create(0, 0, "", "nonblank text-only OP").await,
        Err(StoreError::ContentRejected(_))
    ));
    sqlx::query("DELETE FROM admission.hits WHERE rule_id=$1")
        .bind(rule)
        .execute(&f.owner)
        .await
        .unwrap();
    sqlx::query("UPDATE content.boards SET require_subject=true,comment_max_lines=1 WHERE slug=$1")
        .bind(&f.boards[0])
        .execute(&f.owner)
        .await
        .unwrap();
    for (subject, comment, error) in [
        ("##", "".into(), "Error: New threads require a subject."),
        (
            "valid",
            "x".repeat(2001),
            "Enter a comment within this board's character limit.",
        ),
        ("valid", "a\nb\nc".into(), "Error: Too many lines."),
        (
            "valid",
            "x\n".repeat(7) + "end",
            "Error: Our system thinks your post is spam.",
        ),
    ] {
        assert!(matches!(f.create(0, 0, subject, &comment).await,
            Err(StoreError::Invalid(actual)) if actual == error));
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM admission.hits WHERE rule_id=$1")
            .bind(rule)
            .fetch_one(&f.owner)
            .await
            .unwrap(),
        0
    );
    sqlx::query("UPDATE content.boards SET require_subject=false,text_only=false,comment_max_lines=100 WHERE slug=$1")
        .bind(&f.boards[0]).execute(&f.owner).await.unwrap();
    // Allowing actions remain transactional if final content admission fails.
    for autosage in [false, true] {
        sqlx::query("UPDATE admission.rules SET log=true,autosage=$2 WHERE id=$1")
            .bind(rule)
            .bind(autosage)
            .execute(&f.owner)
            .await
            .unwrap();
        let allowed = f
            .create(0, 0, "", &format!("valid content {autosage}"))
            .await
            .unwrap();
        assert_eq!(
            sqlx::query_scalar::<_, bool>("SELECT permasage FROM content.threads WHERE id=$1")
                .bind(allowed)
                .fetch_one(&f.owner)
                .await
                .unwrap(),
            autosage
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM admission.logs WHERE board=$1")
                .bind(&f.boards[0])
                .fetch_one(&f.owner)
                .await
                .unwrap(),
            i64::from(!autosage)
        );
        for query in [
            "DELETE FROM admission.hits WHERE rule_id=$1",
            "DELETE FROM admission.logs WHERE rule_id=$1",
        ] {
            sqlx::query(query)
                .bind(rule)
                .execute(&f.owner)
                .await
                .unwrap();
        }
        for (parent, error) in [
            (0, "Error: New threads require a subject or comment."),
            (thread, "Error: No text entered."),
        ] {
            assert!(matches!(f.create(0,parent,"","[spoiler] [/spoiler]").await,
                Err(StoreError::Invalid(actual)) if actual == error));
        }
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM admission.hits WHERE rule_id=$1")
                .bind(rule)
                .fetch_one(&f.owner)
                .await
                .unwrap(),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT count(*) FROM admission.logs WHERE board=$1")
                .bind(&f.boards[0])
                .fetch_one(&f.owner)
                .await
                .unwrap(),
            0
        );
    }
    sqlx::query("UPDATE admission.rules SET log=false,autosage=false,quiet=true WHERE id=$1")
        .bind(rule)
        .execute(&f.owner)
        .await
        .unwrap();
    for parent in [0, thread] {
        assert!(matches!(
            f.create(0, parent, "", "").await,
            Err(StoreError::ContentQuiet { .. })
        ));
    }
    assert_eq!(f.count("posts").await, 3);
    assert_eq!(f.count("threads").await, 3);
    assert!(
        !sqlx::query_scalar::<_, bool>("SELECT permasage FROM content.threads WHERE id=$1")
            .bind(thread)
            .fetch_one(&f.owner)
            .await
            .unwrap()
    );
    f.cleanup().await;
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
    f.create(0, 0, "", "paper after capacity recovery")
        .await
        .unwrap();
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
    // The prior OP belongs to an independent ban-expiry setup operation.
    support::cleanup_posting(&f.owner, &f.boards[0]).await;
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
        support::create_post_with_context(
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
    let result = support::create_post_with_context(
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
    assert!(matches!(result, Err(StoreError::Conflict(_))));
    let effects: i64 = sqlx::query_scalar("SELECT count(*) FROM admission.bans WHERE board=$1")
        .bind(&f.boards[0])
        .fetch_one(&f.owner)
        .await
        .unwrap();
    assert_eq!(
        effects, 0,
        "An upload to a no-image board cannot create a filename ban"
    );
    sqlx::query("UPDATE content.boards SET image_limit=100 WHERE slug=$1")
        .bind(&f.boards[0])
        .execute(&f.owner)
        .await
        .unwrap();
    let metadata: (String, Option<String>, Option<Vec<u8>>, Option<bool>) = sqlx::query_as(
        "SELECT * FROM content.attachment_upload_filter_metadata($1,$2,$3,0,false,false)",
    )
    .bind(&id)
    .bind(&attachment.upload.capability)
    .bind(&f.boards[0])
    .fetch_one(&f.public)
    .await
    .unwrap();
    assert_eq!(metadata, ("phpé".into(), None, None, None));
    assert!(
        sqlx::query(
            "SELECT * FROM content.attachment_upload_filter_metadata($1,$2,$3,0,true,true)"
        )
        .bind(&id)
        .bind(&attachment.upload.capability)
        .bind(&f.boards[0])
        .execute(&f.public)
        .await
        .is_err()
    );
    let result = support::create_post_with_context(
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
        matches!(&result,Err(StoreError::ContentRejected(message)) if message=="Error: Abnormal reply."),
        "unexpected filename result: {result:?}"
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
    let thread = f
        .create(0, 0, "Owned privilege control", "Owned live thread body")
        .await
        .unwrap();
    let rule = f.rule("paper").await;
    for query in [
        "SELECT * FROM admission.rules",
        "SELECT * FROM admission.hits",
        "SELECT * FROM admission.logs",
        "SELECT * FROM admission.bans",
        "DELETE FROM admission.policy WHERE singleton",
        "SELECT * FROM staff_identity.accounts",
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
    // Privilege controls must mutate this test's live rows, never unrelated
    // historical tombstones or rules left by another owned test fixture.
    for (mutation, snapshot, id, field, initial, changed) in [
        (
            "UPDATE content.threads SET permasage=true WHERE board=$1 AND id=$2 AND NOT deleted AND NOT content_erased",
            "SELECT to_jsonb(t) FROM content.threads t WHERE board=$1 AND id=$2",
            thread,
            "permasage",
            false,
            true,
        ),
        (
            "UPDATE admission.rules SET active=false WHERE board=$1 AND id=$2",
            "SELECT to_jsonb(r) FROM admission.rules r WHERE board=$1 AND id=$2",
            rule,
            "active",
            true,
            false,
        ),
    ] {
        let before: serde_json::Value = sqlx::query_scalar(snapshot)
            .bind(&f.boards[0])
            .bind(id)
            .fetch_one(&f.owner)
            .await
            .unwrap();
        assert_eq!(
            before[field], initial,
            "The owned positive control must be live"
        );
        let mut healthy = f.owner.begin().await.unwrap();
        let rows = sqlx::query(mutation)
            .bind(&f.boards[0])
            .bind(id)
            .execute(&mut *healthy)
            .await
            .unwrap()
            .rows_affected();
        assert_eq!(
            rows, 1,
            "The healthy role must change exactly one owned row"
        );
        let within: serde_json::Value = sqlx::query_scalar(snapshot)
            .bind(&f.boards[0])
            .bind(id)
            .fetch_one(&mut *healthy)
            .await
            .unwrap();
        assert_eq!(within[field], changed);
        healthy.rollback().await.unwrap();
        let restored: serde_json::Value = sqlx::query_scalar(snapshot)
            .bind(&f.boards[0])
            .bind(id)
            .fetch_one(&f.owner)
            .await
            .unwrap();
        assert_eq!(restored, before, "Rollback restores the complete owned row");
        let error = sqlx::query(mutation)
            .bind(&f.boards[0])
            .bind(id)
            .execute(&f.public)
            .await
            .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501"),
            "{mutation}"
        );
        let after_denial: serde_json::Value = sqlx::query_scalar(snapshot)
            .bind(&f.boards[0])
            .bind(id)
            .fetch_one(&f.owner)
            .await
            .unwrap();
        assert_eq!(
            after_denial, before,
            "Denied public mutation changes no owned state"
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
    let create = |board: usize, parent: i64, session: PostingSession| {
        let public = f.public.clone();
        let board = f.boards[board].clone();
        let peer = f.peer;
        async move {
            support::create_post_with_anonymous_session(
                &public,
                &board,
                parent,
                &NewPost {
                    name: "Anonymous".into(),
                    subject: "".into(),
                    comment: support::distinct_comment("paper"),
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
                    drawing: None,
                    spoiler: false,
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
    let op = create(0, 0, session).await.unwrap();
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
        create(0, 0, session).await,
        Err(StoreError::ContentRejected(_))
    ));
    sqlx::query("UPDATE post_secrets.anonymous_sessions SET posts=11 WHERE token_hash=$1")
        .bind(fingerprints.token.as_slice())
        .execute(&f.owner)
        .await
        .unwrap();
    // Retain same-actor session contention across boards without asking one
    // actor to create two OPs in the cross-board cooldown window.
    support::cleanup_posting(&f.owner, &f.boards[0]).await;
    let (a, b) = tokio::time::timeout(std::time::Duration::from_secs(10), async {
        tokio::join!(create(0, op, session), create(1, 0, session))
    })
    .await
    .unwrap();
    a.unwrap();
    b.unwrap();
    assert_eq!(f.count("posts").await, 3);
    sqlx::query("UPDATE post_secrets.anonymous_sessions SET expires_at=extract(epoch FROM clock_timestamp())::bigint-1 WHERE token_hash=$1")
        .bind(fingerprints.token.as_slice()).execute(&f.owner).await.unwrap();
    assert!(matches!(
        create(0, 0, session).await,
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

#[tokio::test]
async fn automatic_ban_exemption_preserves_rejection_and_quiet_effects() {
    let _guard = POLICY_TEST.lock().await;
    let mut f = Fixture::new().await;
    f.peer = "192.0.2.182".parse().unwrap();
    let rule = f.rule("ownedexemption").await;
    sqlx::query("UPDATE admission.rules SET ban_days=3 WHERE id=$1")
        .bind(rule)
        .execute(&f.owner)
        .await
        .unwrap();
    sqlx::query("INSERT INTO admission.automatic_ban_exemptions(network) VALUES('192.0.2.0/24')")
        .execute(&f.owner)
        .await
        .unwrap();
    assert!(matches!(
        f.create(0, 0, "", "ownedexemption").await,
        Err(StoreError::ContentRejected(_))
    ));
    assert_eq!(f.count("posts").await, 0);
    let bans: i64 = sqlx::query_scalar("SELECT count(*) FROM admission.bans WHERE board=ANY($1)")
        .bind(&f.boards[..])
        .fetch_one(&f.owner)
        .await
        .unwrap();
    assert_eq!(bans, 0);
    let hits: i64 = sqlx::query_scalar("SELECT count FROM admission.hits WHERE rule_id=$1")
        .bind(rule)
        .fetch_one(&f.owner)
        .await
        .unwrap();
    assert_eq!(
        hits, 1,
        "An exemption suppresses the ban, not the filter hit"
    );
    sqlx::query("UPDATE admission.rules SET quiet=true WHERE id=$1")
        .bind(rule)
        .execute(&f.owner)
        .await
        .unwrap();
    assert!(matches!(
        f.create(0, 0, "", "ownedexemption").await,
        Err(StoreError::ContentQuiet { .. })
    ));
    assert!(
        f.create(1, 0, "", "Ordinary content remains permitted")
            .await
            .is_ok()
    );
    sqlx::query(
        "UPDATE admission.automatic_ban_exemptions SET active=false WHERE network='192.0.2.0/24'",
    )
    .execute(&f.owner)
    .await
    .unwrap();
    assert!(matches!(
        f.create(0, 0, "", "ownedexemption").await,
        Err(StoreError::ContentQuiet { .. })
    ));
    assert!(matches!(
        f.create(1, 0, "", "The ordinary peer is now banned").await,
        Err(StoreError::ContentRejected(_))
    ));
    sqlx::query(
        "UPDATE admission.automatic_ban_exemptions SET active=true WHERE network='192.0.2.0/24'",
    )
    .execute(&f.owner)
    .await
    .unwrap();
    assert!(matches!(
        f.create(1, 0, "", "Exemption does not remove an existing ban")
            .await,
        Err(StoreError::ContentRejected(_))
    ));
    sqlx::query("DELETE FROM admission.bans WHERE board=ANY($1)")
        .bind(&f.boards[..])
        .execute(&f.owner)
        .await
        .unwrap();
    // The same recorder serves the filename-proxy automatic-ban branch.
    let revision: i64 = sqlx::query_scalar("SELECT revision FROM admission.policy")
        .fetch_one(&f.owner)
        .await
        .unwrap();
    sqlx::query(
        "SELECT content.record_content_admission($1,$2,$3,NULL,'filename',0,'','','','phpab')",
    )
    .bind(&f.boards[0])
    .bind(f.peer.to_string())
    .bind(revision)
    .execute(&f.public)
    .await
    .unwrap();
    let bans: i64 = sqlx::query_scalar("SELECT count(*) FROM admission.bans WHERE board=ANY($1)")
        .bind(&f.boards[..])
        .fetch_one(&f.owner)
        .await
        .unwrap();
    assert_eq!(bans, 0);
    // An overfull policy must not turn into an empty exemption list and ban.
    sqlx::query("INSERT INTO admission.automatic_ban_exemptions(network) SELECT ('192.0.2.'||n||'/32')::cidr FROM generate_series(1,129) n")
        .execute(&f.owner).await.unwrap();
    sqlx::query("DELETE FROM admission.hits WHERE rule_id=$1")
        .bind(rule)
        .execute(&f.owner)
        .await
        .unwrap();
    let before = f.count("posts").await;
    assert!(matches!(
        f.create(0, 0, "", "ownedexemption").await,
        Err(StoreError::Database(_))
    ));
    assert_eq!(f.count("posts").await, before);
    let effects:i64=sqlx::query_scalar("SELECT (SELECT count(*) FROM admission.hits WHERE rule_id=$1)+(SELECT count(*) FROM admission.bans WHERE board=ANY($2))")
        .bind(rule).bind(&f.boards[..]).fetch_one(&f.owner).await.unwrap();
    assert_eq!(effects, 0);
    sqlx::query("DELETE FROM admission.automatic_ban_exemptions WHERE network='192.0.2.0/24' OR network IN (SELECT ('192.0.2.'||n||'/32')::cidr FROM generate_series(1,129) n)")
        .execute(&f.owner).await.unwrap();
    f.cleanup().await;
}
