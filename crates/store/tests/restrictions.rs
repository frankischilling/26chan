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
    f.cleanup(&cap).await;
}
