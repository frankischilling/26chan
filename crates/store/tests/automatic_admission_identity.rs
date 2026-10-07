#![cfg(feature = "database-tests")]
mod support;

use board_domain::anonymous_session::Capability;
use board_store::{
    AnonymousPostingContext, NewPost, PostIdentityKeys, PostMetadata, PostingContext, StoreError,
    anonymous_session::PostingSession,
};
use chrono::{DateTime, Utc};
use sqlx::PgPool;
use std::{
    future::Future,
    net::IpAddr,
    sync::{Arc, Mutex},
};

// These fixtures own their boards, tokens, policy and clock interventions.
// All counted posts use the actual public writer and server-resolved capability
// context. No function replacement, guard weakening or fabricated history.
#[derive(Clone)]
struct Fixture {
    owner: PgPool,
    public: PgPool,
    board: String,
    tokens: Arc<Mutex<Vec<[u8; 32]>>>,
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
        let board: String =
            sqlx::query_scalar("SELECT 'ai'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
                .fetch_one(&owner)
                .await
                .unwrap();
        sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,user_thread_limit,user_thread_period_hours) VALUES($1,'Owned automatic identity','Synthetic',2000,1000,1000,1000,10,0,0,0,10,24)")
            .bind(&board).execute(&owner).await.unwrap();
        Self {
            owner,
            public,
            board,
            tokens: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn capability(&self) -> Capability {
        let capability = Capability::generate().unwrap();
        self.tokens.lock().unwrap().push(capability.storage_hash());
        capability
    }

    async fn now(&self) -> i64 {
        sqlx::query_scalar("SELECT floor(extract(epoch FROM clock_timestamp()))::bigint")
            .fetch_one(&self.owner)
            .await
            .unwrap()
    }

    async fn policy(&self, limit: i32, hours: i32) {
        sqlx::query("UPDATE content.boards SET user_thread_limit=$2,user_thread_period_hours=$3 WHERE slug=$1")
            .bind(&self.board).bind(limit).bind(hours).execute(&self.owner).await.unwrap();
    }

    async fn post(
        &self,
        cap: &Capability,
        minted: bool,
        peer: IpAddr,
        now: i64,
        parent: i64,
    ) -> Result<i64, StoreError> {
        support::create_post_with_anonymous_session(
            &self.public,
            &self.board,
            parent,
            &post(),
            None,
            AnonymousPostingContext {
                posting: PostingContext {
                    request_start: timestamp(now),
                    peer: Some(peer),
                    op_password_proof: None,
                },
                session: session(cap, minted, peer, now),
            },
            PostMetadata {
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

    async fn identity(&self, cap: &Capability) -> Option<String> {
        sqlx::query_scalar("SELECT automatic_identity::text FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
            .bind(cap.storage_hash().as_slice()).fetch_optional(&self.owner).await.unwrap().flatten()
    }

    async fn report_identities(&self) -> Vec<Option<String>> {
        // The migrator may explicitly assume the private owner but must not
        // gain an inherited grant merely to inspect operator-owned fixtures.
        let mut tx = self.owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_report_admission_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        let identities = sqlx::query_scalar("SELECT automatic_identity::text FROM post_secrets.report_membership WHERE board=$1 ORDER BY report_id")
            .bind(&self.board).fetch_all(&mut *tx).await.unwrap();
        tx.rollback().await.unwrap();
        identities
    }

    async fn history_identity(&self, post: i64) -> Option<String> {
        sqlx::query_scalar("SELECT automatic_identity::text FROM post_secrets.posting_history WHERE board=$1 AND post_id=$2")
            .bind(&self.board).bind(post).fetch_one(&self.owner).await.unwrap()
    }

    async fn snapshot(&self) -> serde_json::Value {
        let tokens = self
            .tokens
            .lock()
            .unwrap()
            .iter()
            .map(|token| token.to_vec())
            .collect::<Vec<_>>();
        sqlx::query_scalar("SELECT jsonb_build_object('posts',(SELECT coalesce(jsonb_agg(to_jsonb(p) ORDER BY p.id),'[]') FROM content.posts p WHERE board=$1),'threads',(SELECT coalesce(jsonb_agg(to_jsonb(t) ORDER BY t.id),'[]') FROM content.threads t WHERE board=$1),'history',(SELECT coalesce(jsonb_agg(to_jsonb(h) ORDER BY h.post_id),'[]') FROM post_secrets.posting_history h WHERE board=$1),'sessions',(SELECT coalesce(jsonb_agg(to_jsonb(s) ORDER BY s.token_hash),'[]') FROM post_secrets.anonymous_sessions s WHERE token_hash=ANY($2::bytea[])),'members',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY a.post_id),'[]') FROM post_secrets.anonymous_posts a WHERE token_hash=ANY($2::bytea[])))")
            .bind(&self.board).bind(tokens).fetch_one(&self.owner).await.unwrap()
    }

    async fn cleanup(&self) {
        let mut tx = support::begin_cleanup(&self.owner, std::slice::from_ref(&self.board)).await;
        for query in [
            "DELETE FROM admission.rules WHERE board=$1",
            "DELETE FROM content.reports WHERE board=$1",
            "DELETE FROM content.post_media WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
            "DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
            "DELETE FROM content.posts WHERE board=$1",
            "DELETE FROM content.threads WHERE board=$1",
        ] {
            sqlx::query(query)
                .bind(&self.board)
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        support::cleanup_posting(&mut tx, &self.board).await;
        sqlx::query("DELETE FROM content.boards WHERE slug=$1")
            .bind(&self.board)
            .execute(&mut *tx)
            .await
            .unwrap();
        let tokens = self.tokens.lock().unwrap().clone();
        for token in tokens {
            sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
                .bind(token.as_slice())
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        tx.commit().await.unwrap();
    }
}

fn post() -> NewPost {
    NewPost {
        name: "Anonymous".into(),
        subject: "Owned identity".into(),
        comment: "owned automatic identity content".into(),
        deletion_hash: "same-independent-recovery-password".into(),
        sage: false,
    }
}
fn timestamp(now: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(now, 0).unwrap()
}
fn peer(n: u8) -> IpAddr {
    format!("198.51.100.{n}").parse().unwrap()
}
fn session(cap: &Capability, minted: bool, peer: IpAddr, now: i64) -> PostingSession {
    PostingSession {
        fingerprints: cap.fingerprints(Some(peer), *b"US"),
        minted,
        now: timestamp(now),
    }
}
fn rejected(result: Result<i64, StoreError>, limit: i32) {
    match result {
        Err(StoreError::ContentRejected(message)) => assert_eq!(
            message,
            format!(
                "Error: You may not post more than {limit} active thread{} at a time.",
                if limit > 1 { "s" } else { "" }
            )
        ),
        other => panic!("Expected automatic-identity OR-IP quota rejection: {other:?}"),
    }
}
async fn run<F, Fut>(test: F)
where
    F: FnOnce(Fixture) -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
{
    let fixture = Fixture::new().await;
    let outcome = tokio::spawn(test(fixture.clone())).await;
    fixture.cleanup().await;
    outcome.unwrap();
}

#[tokio::test]
async fn same_capability_cross_ip_counts_but_equal_recovery_passwords_do_not_merge() {
    run(|f| async move {
        f.policy(1, 24).await;
        let a = f.capability();
        let b = f.capability();
        let now = f.now().await;
        let first = f.post(&a, true, peer(1), now, 0).await.unwrap();
        let a_id = f
            .identity(&a)
            .await
            .expect("successful registration allocates identity");
        assert_eq!(
            f.history_identity(first).await.as_deref(),
            Some(a_id.as_str())
        );
        let before = f.snapshot().await;
        rejected(f.post(&a, false, peer(2), now + 1, 0).await, 1);
        assert_eq!(
            f.snapshot().await,
            before,
            "quota rejection cannot change activity or history"
        );
        // Both posts use the exact same independent recovery password.
        let second = f.post(&b, true, peer(3), now + 1, 0).await.unwrap();
        assert_ne!(f.identity(&b).await.as_deref(), Some(a_id.as_str()));
        assert_eq!(f.history_identity(second).await, f.identity(&b).await);
        // Minting another capability does not bypass the IP side of OR.
        let c = f.capability();
        let before = f.snapshot().await;
        rejected(f.post(&c, true, peer(1), now + 2, 0).await, 1);
        assert_eq!(f.snapshot().await, before);
        assert!(f.identity(&c).await.is_none());
    })
    .await;
}

#[tokio::test]
async fn overlapping_ip_and_identity_count_each_op_once_and_distinct_branches_add() {
    run(|f| async move {
        f.policy(2, 24).await;
        let a = f.capability();
        let b = f.capability();
        let now = f.now().await;
        f.post(&a, true, peer(10), now, 0).await.unwrap();
        // This one predecessor matches BOTH predicates. It occupies one slot.
        f.post(&a, false, peer(10), now + 1, 0).await.unwrap();
        rejected(f.post(&a, false, peer(11), now + 2, 0).await, 2);
        f.policy(3, 24).await;
        f.post(&b, true, peer(12), now + 2, 0).await.unwrap();
        // Two identity-only predecessors plus one IP-only predecessor fill 3.
        rejected(f.post(&a, false, peer(12), now + 3, 0).await, 3);
        // Existing different capability is still rejected by shared IP.
        f.policy(1, 24).await;
        rejected(f.post(&b, false, peer(10), now + 3, 0).await, 1);
    })
    .await;
}

#[tokio::test]
async fn source_creation_second_and_exact_idle_reset_exempt_only_identity_branch() {
    run(|f| async move {
        f.policy(1, 24).await;
        let cap = f.capability(); let now = f.now().await;
        f.post(&cap, true, peer(20), now, 0).await.unwrap();
        let identity = f.identity(&cap).await;
        // Existing token in the creation second is source-new too.
        f.post(&cap, false, peer(21), now, 0).await.unwrap();
        rejected(f.post(&cap, false, peer(22), now + 1, 0).await, 1);
        // Operator-owned session-clock fixture: exactly seven days, not seven
        // days plus one. No public API can write these activity fields.
        sqlx::query("UPDATE post_secrets.anonymous_sessions SET created_at=$2-604800,network_at=$2-604800,address_at=$2-604800,environment_at=$2-604800,activity_at=$2-604800 WHERE token_hash=$1")
            .bind(cap.storage_hash().as_slice()).bind(now + 2).execute(&f.owner).await.unwrap();
        f.post(&cap, false, peer(22), now + 2, 0).await.unwrap();
        assert_eq!(f.identity(&cap).await, identity, "idle reset retains automatic identity");
        let reset: i64 = sqlx::query_scalar("SELECT created_at FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
            .bind(cap.storage_hash().as_slice()).fetch_one(&f.owner).await.unwrap();
        assert_eq!(reset, now + 2);
        f.post(&cap, false, peer(23), now + 2, 0).await.unwrap();
        rejected(f.post(&cap, false, peer(24), now + 3, 0).await, 1);
        // Source-new is not a blanket quota exemption: IP still counts.
        rejected(f.post(&cap, false, peer(20), now + 2, 0).await, 1);
    }).await;
}

#[tokio::test]
async fn identity_quota_uses_strict_source_post_time_not_registration_time() {
    run(|f| async move {
        f.policy(1, 1).await;
        let a = f.capability(); let b = f.capability(); let c = f.capability(); let now = f.now().await;
        for (cap, ip, offset) in [(&a, 30, -3600), (&b, 31, -3599), (&c, 32, 10)] {
            let op = f.post(cap, true, peer(ip), now, 0).await.unwrap();
            // Historical source timestamps are operator-owned fixture data.
            // Keep real admission/history registration timestamps untouched.
            sqlx::query("UPDATE content.posts SET created_at=to_timestamp($2::double precision) WHERE id=$1")
                .bind(op).bind(now + 1 + offset).execute(&f.owner).await.unwrap();
            let recorded: i64 = sqlx::query_scalar("SELECT request_at FROM post_secrets.posting_history WHERE post_id=$1")
                .bind(op).fetch_one(&f.owner).await.unwrap();
            assert_eq!(recorded, now);
        }
        f.post(&a, false, peer(33), now + 1, 0).await.unwrap();
        rejected(f.post(&b, false, peer(34), now + 1, 0).await, 1);
        rejected(f.post(&c, false, peer(35), now + 1, 0).await, 1);
    }).await;
}

#[tokio::test]
async fn rejected_content_and_invalid_registration_leave_no_identity_activity_or_history() {
    run(|f| async move {
        let cap = f.capability(); let missing = f.capability(); let now = f.now().await;
        let op = f.post(&cap, true, peer(40), now, 0).await.unwrap();
        for (token, minted) in [(&missing, false), (&cap, true)] {
            let before = f.snapshot().await;
            assert!(matches!(f.post(token, minted, peer(41), now + 1, op).await, Err(StoreError::AuthorizationChanged)));
            assert_eq!(f.snapshot().await, before);
        }
        // Ordinary literal matching normalizes away spaces; use a word that
        // survives that source projection rather than an unnormalized phrase.
        let rule: i64 = sqlx::query_scalar("INSERT INTO admission.rules(board,pattern) VALUES($1,'automatic') RETURNING id")
            .bind(&f.board).fetch_one(&f.owner).await.unwrap();
        for (token, minted) in [(&missing, true), (&cap, false)] {
            let before = f.snapshot().await;
            let result = f.post(token, minted, peer(42), now + 1, op).await;
            assert!(matches!(&result, Err(StoreError::ContentRejected(message)) if message == "Error: Our system thinks your post is spam. Please reformat and try again."),
                "The owned active literal rule must reject content (minted={minted}): {result:?}");
            assert_eq!(f.snapshot().await, before);
        }
        sqlx::query("DELETE FROM admission.rules WHERE id=$1").bind(rule).execute(&f.owner).await.unwrap();
        sqlx::query("UPDATE post_secrets.anonymous_sessions SET expires_at=1 WHERE token_hash=$1")
            .bind(cap.storage_hash().as_slice()).execute(&f.owner).await.unwrap();
        let before = f.snapshot().await;
        assert!(matches!(f.post(&cap, false, peer(43), now + 1, op).await, Err(StoreError::AuthorizationChanged)));
        assert_eq!(f.snapshot().await, before);
        assert!(f.identity(&missing).await.is_none());
    }).await;
}

#[tokio::test]
async fn legacy_null_identity_is_forward_only_and_session_gc_preserves_captured_history() {
    run(|f| async move {
        let cap = f.capability(); let now = f.now().await;
        let old = f.post(&cap, true, peer(50), now, 0).await.unwrap();
        // Model pre-migration rows explicitly as the operator. Allocation on
        // the next successful registration must not backfill this old row.
        sqlx::query("UPDATE post_secrets.anonymous_sessions SET automatic_identity=NULL WHERE token_hash=$1")
            .bind(cap.storage_hash().as_slice()).execute(&f.owner).await.unwrap();
        sqlx::query("UPDATE post_secrets.posting_history SET automatic_identity=NULL WHERE post_id=$1")
            .bind(old).execute(&f.owner).await.unwrap();
        let reply = f.post(&cap, false, peer(51), now + 1, old).await.unwrap();
        let identity = f.identity(&cap).await.expect("legacy session allocates lazily");
        assert!(f.history_identity(old).await.is_none());
        assert_eq!(f.history_identity(reply).await.as_deref(), Some(identity.as_str()));
        sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
            .bind(cap.storage_hash().as_slice()).execute(&f.owner).await.unwrap();
        assert_eq!(f.history_identity(reply).await.as_deref(), Some(identity.as_str()));
        assert!(f.history_identity(old).await.is_none());
        assert!(board_store::find_post(&f.public, &f.board, reply).await.is_ok());
    }).await;
}

#[tokio::test]
async fn runtime_cannot_read_choose_or_retroactively_stamp_private_identity() {
    run(|f| async move {
        let cap = f.capability();
        let now = f.now().await;
        let old = support::create_post_with_context(
            &f.public,
            &f.board,
            0,
            &post(),
            None,
            PostingContext {
                request_start: timestamp(now),
                peer: Some(peer(60)),
                op_password_proof: None,
            },
        )
        .await
        .unwrap();
        assert!(f.history_identity(old).await.is_none());
        for query in [
            "SELECT automatic_identity FROM post_secrets.anonymous_sessions",
            "SELECT automatic_identity FROM post_secrets.posting_history",
            "SELECT automatic_identity FROM post_secrets.report_membership",
            "UPDATE post_secrets.anonymous_sessions SET automatic_identity=gen_random_uuid()",
            "UPDATE post_secrets.posting_history SET automatic_identity=gen_random_uuid()",
        ] {
            let error = sqlx::query(query).execute(&f.public).await.unwrap_err();
            assert_eq!(
                error.as_database_error().unwrap().code().as_deref(),
                Some("42501")
            );
        }
        let fingerprints = cap.fingerprints(Some(peer(60)), *b"US");
        let before = f.snapshot().await;
        let error =
            sqlx::query("SELECT content.register_anonymous_post($1,$2,$3,$4,true,$5,$6,$7)")
                .bind(fingerprints.token.as_slice())
                .bind(fingerprints.network.as_slice())
                .bind(fingerprints.address.as_slice())
                .bind(fingerprints.environment.as_slice())
                .bind(&f.board)
                .bind(old)
                .bind(now)
                .execute(&f.public)
                .await
                .unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("23514")
        );
        assert_eq!(
            f.snapshot().await,
            before,
            "same-transaction registration proof rejects historical adoption before allocation"
        );
        assert!(f.identity(&cap).await.is_none());
        assert!(f.history_identity(old).await.is_none());
    })
    .await;
}

#[tokio::test]
async fn reports_capture_private_identity_and_reject_cross_ip_duplicate_and_flood() {
    run(|f| async move {
        let author = f.capability();
        let reporter = f.capability();
        let now = f.now().await;
        let op = f.post(&author, true, peer(70), now, 0).await.unwrap();
        let reply = f.post(&author, false, peer(70), now, op).await.unwrap();
        let key = support::key(&f.board);
        let rate = key.public_report_rate_identity(peer(71));
        board_store::report_with_anonymous_session(
            &f.public,
            &f.board,
            op,
            "Owned identity-forwarding report",
            &rate,
            session(&reporter, true, peer(71), now),
        )
        .await
        .unwrap();
        let identity = f
            .identity(&reporter)
            .await
            .expect("report registration allocates identity");
        let captured = f.report_identities().await;
        assert_eq!(captured, vec![Some(identity.clone())]);
        let before = f.snapshot().await;
        let duplicate = board_store::report_with_anonymous_session(
            &f.public,
            &f.board,
            op,
            "Same capability on a different IP remains a duplicate",
            &key.public_report_rate_identity(peer(72)),
            session(&reporter, false, peer(72), now + 1),
        )
        .await;
        assert_eq!(
            duplicate.unwrap_err().to_string(),
            "You have already reported this post."
        );
        assert_eq!(
            f.snapshot().await,
            before,
            "cross-IP duplicate leaves anonymous activity unchanged"
        );
        let flood = board_store::report_with_anonymous_session(
            &f.public,
            &f.board,
            reply,
            "A different target on a different IP still observes session flood limits",
            &key.public_report_rate_identity(peer(73)),
            session(&reporter, false, peer(73), now + 2),
        )
        .await;
        assert_eq!(
            flood.unwrap_err().to_string(),
            "You have to wait a while before reporting another post."
        );
        assert_eq!(
            f.snapshot().await,
            before,
            "cross-IP flood leaves anonymous activity unchanged"
        );
        assert_eq!(f.report_identities().await, captured);
        let reporter_shape: serde_json::Value =
            sqlx::query_scalar("SELECT to_jsonb(s) FROM content.anonymous_session($1) s")
                .bind(reporter.storage_hash().as_slice())
                .fetch_one(&f.public)
                .await
                .unwrap();
        assert!(reporter_shape.get("automatic_identity").is_none());
        sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
            .bind(reporter.storage_hash().as_slice())
            .execute(&f.owner)
            .await
            .unwrap();
        let retained = f.report_identities().await;
        assert_eq!(
            retained, captured,
            "session GC must not erase report equality history"
        );
        let public_shape: serde_json::Value =
            sqlx::query_scalar("SELECT to_jsonb(s) FROM content.anonymous_session($1) s")
                .bind(author.storage_hash().as_slice())
                .fetch_one(&f.public)
                .await
                .unwrap();
        assert!(public_shape.get("automatic_identity").is_none());
    })
    .await;
}
