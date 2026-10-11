#![cfg(feature = "database-tests")]
use board_domain::{anonymous_session::Capability, poster_id::PosterIdKey};
use board_store::{
    AnonymousPostingContext, NewPost, PostIdentityKeys, PostMetadata, PostingContext, StoreError,
    anonymous_session::PostingSession,
};
use chrono::{Timelike, Utc};
use sqlx::PgPool;
use std::net::IpAddr;

struct Fixture {
    owner: PgPool,
    public: PgPool,
    staff: PgPool,
    boards: Vec<String>,
    key: PosterIdKey,
    session: [u8; 32],
    csrf: [u8; 32],
    account: i64,
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
        let staff = PgPool::connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let suffix: String =
            sqlx::query_scalar("SELECT substr(replace(gen_random_uuid()::text,'-',''),1,8)")
                .fetch_one(&owner)
                .await
                .unwrap();
        let boards = vec![format!("ra{suffix}"), format!("rb{suffix}")];
        for board in &boards {
            sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,user_thread_limit) VALUES($1,'Owned restrictions','Synthetic fixture',2000,1000,1000,1000,10,0,0,0,100)").bind(board).execute(&owner).await.unwrap();
        }
        let account = sqlx::query_scalar("INSERT INTO staff_identity.accounts(role,allow_boards,deny_boards) VALUES('moderator',$1,'{}') RETURNING id").bind(&boards).fetch_one(&owner).await.unwrap();
        let session = Capability::generate().unwrap().storage_hash();
        let csrf = Capability::generate().unwrap().storage_hash();
        sqlx::query(
            "INSERT INTO staff_identity.credentials(id,account_id,credential) VALUES($1,$2,'{}')",
        )
        .bind(session.as_slice())
        .bind(account)
        .execute(&owner)
        .await
        .unwrap();
        sqlx::query("INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id) VALUES($1,$2,$3,$1)").bind(session.as_slice()).bind(csrf.as_slice()).bind(account).execute(&owner).await.unwrap();
        Self {
            owner,
            public,
            staff,
            boards,
            key: PosterIdKey::parse(&"b5".repeat(32)).unwrap(),
            session,
            csrf,
            account,
        }
    }

    async fn post(
        &self,
        board: usize,
        peer: &str,
        capability: Option<(&Capability, bool)>,
    ) -> Result<i64, StoreError> {
        let peer: IpAddr = peer.parse().unwrap();
        let now = Utc::now().with_nanosecond(0).unwrap();
        let posting = PostingContext {
            request_start: now,
            peer: Some(peer),
            op_password_proof: None,
        };
        let parent: Option<i64> = sqlx::query_scalar(
            "SELECT min(id) FROM content.threads WHERE board=$1 AND NOT deleted",
        )
        .bind(&self.boards[board])
        .fetch_one(&self.owner)
        .await
        .unwrap();
        let parent = parent.unwrap_or(0);
        let post = NewPost {
            name: "Anonymous".into(),
            subject: "Owned restriction".into(),
            comment: "Synthetic admission".into(),
            deletion_hash: "Owned recovery proof".into(),
            sage: false,
        };
        let metadata = PostMetadata {
            keys: PostIdentityKeys {
                tripcode: None,
                poster_id: Some(&self.key),
            },
            spoiler: false,
            country_database: None,
            flag: "",
            options: "",
            drawing: None,
        };
        if let Some((cap, minted)) = capability {
            board_store::create_post_with_anonymous_session(
                &self.public,
                &self.boards[board],
                parent,
                &post,
                None,
                AnonymousPostingContext {
                    posting,
                    session: PostingSession {
                        fingerprints: cap.fingerprints(Some(peer), *b"XX"),
                        minted,
                        now,
                    },
                },
                metadata,
            )
            .await
        } else {
            board_store::create_post_with_metadata(
                &self.public,
                &self.boards[board],
                parent,
                &post,
                None,
                posting,
                metadata,
            )
            .await
        }
    }

    async fn issue(&self, target: i64, global: bool, warning: bool, seconds: Option<i64>) -> i64 {
        self.try_issue(target, global, warning, seconds)
            .await
            .unwrap()
    }

    async fn try_issue(
        &self,
        target: i64,
        global: bool,
        warning: bool,
        seconds: Option<i64>,
    ) -> Result<i64, sqlx::Error> {
        sqlx::query_scalar("SELECT content.issue_post_restriction($1,$2,900,$3,$4,$5,$6,$7,'Synthetic <public> reason','Private synthetic reason')")
            .bind(self.session.as_slice()).bind(self.csrf.as_slice()).bind(&self.boards[0]).bind(target)
            .bind(global).bind(warning).bind(seconds).fetch_one(&self.staff).await
    }

    async fn unban(&self, id: i64) -> bool {
        sqlx::query_scalar("SELECT content.unban_restriction($1,$2,900,$3)")
            .bind(self.session.as_slice())
            .bind(self.csrf.as_slice())
            .bind(id)
            .fetch_one(&self.staff)
            .await
            .unwrap()
    }

    async fn count(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=ANY($1)")
            .bind(&self.boards)
            .fetch_one(&self.owner)
            .await
            .unwrap()
    }

    async fn report(
        &self,
        board: usize,
        target: i64,
        peer: &str,
        capability: &Capability,
        categorical: bool,
    ) -> Result<(), StoreError> {
        let now = Utc::now().with_nanosecond(0).unwrap();
        let peer = peer.parse().unwrap();
        let actor = self.key.public_report_rate_identity(peer);
        let session = PostingSession {
            fingerprints: capability.fingerprints(Some(peer), *b"XX"),
            minted: false,
            now,
        };
        if categorical {
            board_store::report_categorical_with_anonymous_session(
                &self.public,
                &self.boards[board],
                target,
                1,
                1,
                &actor,
                session,
            )
            .await
        } else {
            board_store::report_with_anonymous_session(
                &self.public,
                &self.boards[board],
                target,
                "Owned restriction report",
                &actor,
                session,
            )
            .await
        }
    }

    async fn cleanup(self, cap: &Capability) {
        let mut tx = self.owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_restriction_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SELECT singleton FROM restrictions.capacity WHERE singleton FOR UPDATE")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("UPDATE restrictions.capacity SET used_bytes=used_bytes-coalesce((SELECT sum(charged_bytes) FROM restrictions.bans WHERE board=ANY($1)),0) WHERE singleton").bind(&self.boards).execute(&mut *tx).await.unwrap();
        sqlx::query("DELETE FROM restrictions.audit WHERE ban_id IN(SELECT id FROM restrictions.bans WHERE board=ANY($1))").bind(&self.boards).execute(&mut *tx).await.unwrap();
        sqlx::query("DELETE FROM restrictions.bans WHERE board=ANY($1)")
            .bind(&self.boards)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("RESET ROLE").execute(&mut *tx).await.unwrap();
        sqlx::query("SELECT slug FROM content.boards WHERE slug=ANY($1) ORDER BY slug FOR UPDATE")
            .bind(&self.boards)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query(
            "SELECT id FROM content.threads WHERE board=ANY($1) ORDER BY board,id FOR UPDATE",
        )
        .bind(&self.boards)
        .execute(&mut *tx)
        .await
        .unwrap();
        sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=ANY($1))").bind(&self.boards).execute(&mut *tx).await.unwrap();
        sqlx::query("DELETE FROM content.posts WHERE board=ANY($1)")
            .bind(&self.boards)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("DELETE FROM content.threads WHERE board=ANY($1)")
            .bind(&self.boards)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("DELETE FROM post_secrets.posting_thread_actions WHERE board=ANY($1)")
            .bind(&self.boards)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
            .bind(cap.storage_hash().as_slice())
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("DELETE FROM content.boards WHERE slug=ANY($1)")
            .bind(&self.boards)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("DELETE FROM staff_identity.sessions WHERE account_id=$1")
            .bind(self.account)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("DELETE FROM staff_identity.credentials WHERE account_id=$1")
            .bind(self.account)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("DELETE FROM staff_identity.accounts WHERE id=$1")
            .bind(self.account)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
    }
}

fn rejected(result: Result<i64, StoreError>, message: &str) {
    assert!(
        matches!(result, Err(StoreError::ContentRejected(ref actual)) if actual == message),
        "unexpected posting result"
    );
}

#[tokio::test]
async fn real_public_writer_enforces_scope_warning_expiry_identity_and_atomicity() {
    let f = Fixture::new().await;
    let cap = Capability::generate().unwrap();
    f.post(1, "192.0.2.200", None).await.unwrap();
    let original = f.post(0, "192.0.2.134", Some((&cap, true))).await.unwrap();
    // UserPwd becomes established only after its first-second new-user case.
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    let actor = f
        .key
        .public_posting_rate_identity("192.0.2.134".parse().unwrap());
    let mut admitted = f.public.begin().await.unwrap();
    let state: String =
        sqlx::query_scalar("SELECT content.evaluate_posting_restrictions($1,NULL,$2,$3)")
            .bind(actor.as_bytes().as_slice())
            .bind(&f.boards[0])
            .bind(Utc::now().timestamp())
            .fetch_one(&mut *admitted)
            .await
            .unwrap();
    assert_eq!(state, "allowed");
    let contested = f.try_issue(original, false, false, None).await.unwrap_err();
    assert_eq!(
        contested
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("55P03")
    );
    admitted.commit().await.unwrap();
    let scoped = f.issue(original, false, false, None).await;
    let before = f.count().await;
    rejected(
        f.post(0, "192.0.2.134", None).await,
        board_store::restrictions::BANNED,
    );
    assert_eq!(f.count().await, before);
    // An internal/direct writer cannot bypass the application precheck.
    let encoded = actor
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let mut direct = f.public.begin().await.unwrap();
    sqlx::query("SELECT content.lock_posting_actor($1,false)")
        .bind(actor.as_bytes().as_slice())
        .execute(&mut *direct)
        .await
        .unwrap();
    sqlx::query("SELECT set_config('board.posting_actor',$1,true),set_config('board.restriction_token','',true)").bind(encoded).execute(&mut *direct).await.unwrap();
    let error = sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES(nextval('content.post_number'),$1,$2,'Anonymous','','Direct synthetic attempt')")
        .bind(&f.boards[0]).bind(original).execute(&mut *direct).await.unwrap_err();
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("P134B")
    );
    direct.rollback().await.unwrap();
    assert_eq!(f.count().await, before);
    f.post(1, "192.0.2.134", None).await.unwrap();
    // Same automatic capability on a new address is still restricted.
    rejected(
        f.post(0, "192.0.2.135", Some((&cap, false))).await,
        board_store::restrictions::BANNED,
    );
    // A different address without that cookie is a healthy allowed control.
    f.post(0, "192.0.2.136", None).await.unwrap();
    assert!(f.unban(scoped).await);
    assert!(!f.unban(scoped).await);
    let global = f.issue(original, true, false, None).await;
    rejected(
        f.post(1, "192.0.2.134", None).await,
        board_store::restrictions::BANNED,
    );
    assert!(f.unban(global).await);
    let warning = f.issue(original, false, true, Some(0)).await;
    // The source evaluates warning before board/global scope.
    rejected(
        f.post(1, "192.0.2.134", None).await,
        board_store::restrictions::WARNED,
    );
    let other = f
        .key
        .public_posting_rate_identity("192.0.2.136".parse().unwrap());
    assert!(
        !board_store::restrictions::acknowledge(
            &f.public,
            &other,
            None,
            &f.boards[1],
            Utc::now().timestamp(),
            warning
        )
        .await
        .unwrap()
    );
    let status = board_store::restrictions::status(
        &f.public,
        &actor,
        None,
        &f.boards[1],
        Utc::now().timestamp(),
    )
    .await
    .unwrap();
    assert_eq!(status.len(), 1);
    assert!(status[0].warning);
    assert_eq!(status[0].public_reason, "Synthetic <public> reason");
    assert!(
        board_store::restrictions::acknowledge(
            &f.public,
            &actor,
            None,
            &f.boards[1],
            Utc::now().timestamp(),
            warning
        )
        .await
        .unwrap()
    );
    assert!(
        !board_store::restrictions::acknowledge(
            &f.public,
            &actor,
            None,
            &f.boards[1],
            Utc::now().timestamp(),
            warning
        )
        .await
        .unwrap()
    );
    f.post(1, "192.0.2.134", None).await.unwrap();
    let finite = f.issue(original, false, false, Some(1)).await;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let later = f.issue(original, true, false, None).await;
    rejected(
        f.post(0, "192.0.2.134", None).await,
        board_store::restrictions::BANNED,
    );
    let mut tx = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let retired: bool = sqlx::query_scalar("SELECT NOT active AND retirement='expiration' AND EXISTS(SELECT 1 FROM restrictions.audit WHERE ban_id=$1 AND action='expiration') FROM restrictions.bans WHERE id=$1").bind(finite).fetch_one(&mut *tx).await.unwrap();
    assert!(
        retired,
        "visited expiry must persist even when a later ban refuses posting"
    );
    tx.rollback().await.unwrap();
    assert!(f.unban(later).await);
    f.post(0, "192.0.2.134", Some((&cap, false))).await.unwrap();

    // Reporting has its own source policy and independently derived actor.
    // Bind before content locks; issuance cannot race a saved report.
    let reporting_actor = f
        .key
        .public_report_rate_identity("192.0.2.134".parse().unwrap());
    let mut admitted = f.public.begin().await.unwrap();
    sqlx::query("SELECT content.bind_report_restrictions($1,$2,$3,$4,$5,$6)")
        .bind(reporting_actor.as_bytes().as_slice())
        .bind(reporting_actor.restriction_identity().as_bytes().as_slice())
        .bind(cap.storage_hash().as_slice())
        .bind(&f.boards[0])
        .bind(original)
        .bind(Utc::now().timestamp())
        .execute(&mut *admitted)
        .await
        .unwrap();
    assert_eq!(
        f.try_issue(original, false, false, None)
            .await
            .unwrap_err()
            .as_database_error()
            .and_then(|e| e.code())
            .as_deref(),
        Some("55P03")
    );
    admitted.rollback().await.unwrap();

    let other_target: i64 = sqlx::query_scalar("SELECT min(id) FROM content.posts WHERE board=$1")
        .bind(&f.boards[1])
        .fetch_one(&f.owner)
        .await
        .unwrap();
    let scoped_warning = f.issue(original, false, true, Some(0)).await;
    // GET omits source check_ban. A matching ban must not hide the form.
    board_store::report_admission::check_with_session(
        &f.public,
        &f.boards[0],
        original,
        &reporting_actor,
        Some(&cap.storage_hash()),
        Utc::now().timestamp(),
    )
    .await
    .unwrap();
    for categorical in [false, true] {
        assert!(matches!(
            f.report(0, original, "192.0.2.134", &cap, categorical)
                .await,
            Err(StoreError::Invalid(
                board_store::restrictions::REPORT_BANNED
            ))
        ));
    }
    // Reports respect a warning's board; posting warns before scope.
    f.report(1, other_target, "192.0.2.134", &cap, false)
        .await
        .unwrap();
    // Existing source quota errors still take precedence over the new ban check.
    assert!(matches!(
        f.report(0, original, "192.0.2.134", &cap, false).await,
        Err(StoreError::Invalid(
            "You have to wait a while before reporting another post."
        ))
    ));
    sqlx::query("DELETE FROM content.reports WHERE board=ANY($1)")
        .bind(&f.boards)
        .execute(&f.owner)
        .await
        .unwrap();
    assert!(f.unban(scoped_warning).await);

    let expired_report_ban = f.issue(original, false, false, Some(1)).await;
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    // Unlike posting, an active expired row blocks reports without retiring it.
    assert!(matches!(
        f.report(0, original, "192.0.2.134", &cap, false).await,
        Err(StoreError::Invalid(
            board_store::restrictions::REPORT_BANNED
        ))
    ));
    let mut owner = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *owner)
        .await
        .unwrap();
    let active: bool = sqlx::query_scalar("SELECT active AND NOT EXISTS(SELECT 1 FROM restrictions.audit WHERE ban_id=$1 AND action='expiration') FROM restrictions.bans WHERE id=$1")
        .bind(expired_report_ban).fetch_one(&mut *owner).await.unwrap();
    assert!(active);
    owner.rollback().await.unwrap();
    assert!(f.unban(expired_report_ban).await);

    let global_report_ban = f.issue(original, true, false, None).await;
    assert!(matches!(
        f.report(1, other_target, "192.0.2.134", &cap, false).await,
        Err(StoreError::Invalid(
            board_store::restrictions::REPORT_BANNED
        ))
    ));
    // The report source uses getPwd(), even during creation-second/idle reset.
    sqlx::query("UPDATE post_secrets.anonymous_sessions SET created_at=$1 WHERE token_hash=$2")
        .bind(Utc::now().timestamp())
        .bind(cap.storage_hash().as_slice())
        .execute(&f.owner)
        .await
        .unwrap();
    assert!(matches!(
        f.report(0, original, "192.0.2.240", &cap, false).await,
        Err(StoreError::Invalid(
            board_store::restrictions::REPORT_BANNED
        ))
    ));
    // The legacy staff direct SQL boundary refuses omitted and mismatched context.
    let error = sqlx::query("SELECT content.admit_report($1,$2,'Direct unbound attempt',$3)")
        .bind(&f.boards[0])
        .bind(original)
        .bind(reporting_actor.as_bytes().as_slice())
        .execute(&f.staff)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().and_then(|e| e.code()).as_deref(),
        Some("P135C")
    );
    let mut mismatched = f.staff.begin().await.unwrap();
    sqlx::query("SELECT content.bind_report_restrictions($1,$2,NULL,$3,$4,$5)")
        .bind(reporting_actor.as_bytes().as_slice())
        .bind(actor.as_bytes().as_slice())
        .bind(&f.boards[0])
        .bind(original)
        .bind(Utc::now().timestamp())
        .execute(&mut *mismatched)
        .await
        .unwrap();
    let error = sqlx::query("SELECT content.admit_report($1,$2,'Direct mismatched attempt',$3)")
        .bind(&f.boards[0])
        .bind(original)
        .bind([17u8; 32].as_slice())
        .execute(&mut *mismatched)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().and_then(|e| e.code()).as_deref(),
        Some("P135C")
    );
    mismatched.rollback().await.unwrap();
    assert!(f.unban(global_report_ban).await);
    // Native status/viewing must explain every retained reporting restriction,
    // including a source-new valid capability whose old address has changed.
    let other_actor = f
        .key
        .public_posting_rate_identity("192.0.2.240".parse().unwrap());
    for idle in [false, true] {
        let expired = f.issue(original, false, false, Some(1)).await;
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let warning = f.issue(original, false, true, Some(0)).await;
        let request_at = Utc::now().timestamp();
        sqlx::query("UPDATE post_secrets.anonymous_sessions SET created_at=$1,activity_at=0 WHERE token_hash=$2")
            .bind(if idle { request_at - 604800 } else { request_at })
            .bind(cap.storage_hash().as_slice()).execute(&f.owner).await.unwrap();
        let mut verification = f.owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_restriction_owner")
            .execute(&mut *verification)
            .await
            .unwrap();
        let source_new: bool = sqlx::query_scalar(
            "SELECT source_new FROM post_secrets.lookup_automatic_identity($1,$2)",
        )
        .bind(cap.storage_hash().as_slice())
        .bind(request_at)
        .fetch_one(&mut *verification)
        .await
        .unwrap();
        assert!(source_new);
        verification.rollback().await.unwrap();
        // The source posting exception remains intact. Status has its own
        // documented identity contract and does not extend posting bans.
        let posting: String =
            sqlx::query_scalar("SELECT content.evaluate_posting_restrictions($1,$2,$3,$4)")
                .bind(other_actor.as_bytes().as_slice())
                .bind(cap.storage_hash().as_slice())
                .bind(&f.boards[0])
                .bind(request_at)
                .fetch_one(&f.public)
                .await
                .unwrap();
        assert_eq!(posting, "allowed");
        assert!(matches!(
            f.report(0, original, "192.0.2.240", &cap, false).await,
            Err(StoreError::Invalid(
                board_store::restrictions::REPORT_BANNED
            ))
        ));
        let unknown = Capability::generate().unwrap().storage_hash();
        assert!(
            board_store::restrictions::status(
                &f.public,
                &other_actor,
                Some(&unknown),
                &f.boards[0],
                request_at
            )
            .await
            .unwrap()
            .is_empty()
        );
        assert!(
            !board_store::restrictions::acknowledge(
                &f.public,
                &other_actor,
                Some(&unknown),
                &f.boards[0],
                request_at,
                warning
            )
            .await
            .unwrap()
        );
        let expires: i64 = sqlx::query_scalar(
            "SELECT expires_at FROM post_secrets.anonymous_sessions WHERE token_hash=$1",
        )
        .bind(cap.storage_hash().as_slice())
        .fetch_one(&f.owner)
        .await
        .unwrap();
        sqlx::query("UPDATE post_secrets.anonymous_sessions SET expires_at=$1 WHERE token_hash=$2")
            .bind(request_at - 1)
            .bind(cap.storage_hash().as_slice())
            .execute(&f.owner)
            .await
            .unwrap();
        assert!(
            board_store::restrictions::status(
                &f.public,
                &other_actor,
                Some(&cap.storage_hash()),
                &f.boards[0],
                request_at
            )
            .await
            .unwrap()
            .is_empty()
        );
        assert!(
            !board_store::restrictions::acknowledge(
                &f.public,
                &other_actor,
                Some(&cap.storage_hash()),
                &f.boards[0],
                request_at,
                warning
            )
            .await
            .unwrap()
        );
        sqlx::query("UPDATE post_secrets.anonymous_sessions SET expires_at=$1 WHERE token_hash=$2")
            .bind(expires)
            .bind(cap.storage_hash().as_slice())
            .execute(&f.owner)
            .await
            .unwrap();
        let status = board_store::restrictions::status(
            &f.public,
            &other_actor,
            Some(&cap.storage_hash()),
            &f.boards[0],
            request_at,
        )
        .await
        .unwrap();
        assert_eq!(
            status.iter().map(|row| row.id).collect::<Vec<_>>(),
            vec![expired, warning]
        );
        assert!(
            board_store::restrictions::acknowledge(
                &f.public,
                &other_actor,
                Some(&cap.storage_hash()),
                &f.boards[0],
                request_at,
                warning
            )
            .await
            .unwrap()
        );
        let status = board_store::restrictions::status(
            &f.public,
            &other_actor,
            Some(&cap.storage_hash()),
            &f.boards[0],
            request_at,
        )
        .await
        .unwrap();
        assert_eq!(
            status.iter().map(|row| row.id).collect::<Vec<_>>(),
            vec![expired]
        );
        let mut verification = f.owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_restriction_owner")
            .execute(&mut *verification)
            .await
            .unwrap();
        let events: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM restrictions.audit WHERE ban_id=$1 AND action='warning-viewed'",
        )
        .bind(warning)
        .fetch_one(&mut *verification)
        .await
        .unwrap();
        assert_eq!(events, 1);
        verification.rollback().await.unwrap();
        assert!(f.unban(expired).await);
    }
    f.report(0, original, "192.0.2.240", &cap, false)
        .await
        .unwrap();
    let reports: Vec<i64> =
        sqlx::query_scalar("SELECT id FROM content.reports WHERE board=ANY($1)")
            .bind(&f.boards)
            .fetch_all(&f.owner)
            .await
            .unwrap();
    assert_eq!(reports.len(), 1);
    let mut verification = f.owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_report_snapshot_owner")
        .execute(&mut *verification)
        .await
        .unwrap();
    let snapshots: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM post_secrets.report_post_snapshots WHERE report_id=ANY($1)",
    )
    .bind(&reports)
    .fetch_one(&mut *verification)
    .await
    .unwrap();
    sqlx::query("RESET ROLE")
        .execute(&mut *verification)
        .await
        .unwrap();
    sqlx::query("SET LOCAL ROLE board_report_admission_owner")
        .execute(&mut *verification)
        .await
        .unwrap();
    let (evidence, memberships): (i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM post_secrets.report_weight_evidence WHERE report_id=ANY($1)),(SELECT count(*) FROM post_secrets.report_membership WHERE report_id=ANY($1))")
        .bind(&reports).fetch_one(&mut *verification).await.unwrap();
    sqlx::query("RESET ROLE")
        .execute(&mut *verification)
        .await
        .unwrap();
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *verification)
        .await
        .unwrap();
    let registrations: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM post_secrets.anonymous_reports WHERE report_id=ANY($1)",
    )
    .bind(&reports)
    .fetch_one(&mut *verification)
    .await
    .unwrap();
    verification.rollback().await.unwrap();
    assert_eq!(
        (snapshots, evidence, memberships, registrations),
        (1, 1, 1, 1)
    );
    sqlx::query("DELETE FROM content.reports WHERE board=ANY($1)")
        .bind(&f.boards)
        .execute(&f.owner)
        .await
        .unwrap();
    f.cleanup(&cap).await;
}
