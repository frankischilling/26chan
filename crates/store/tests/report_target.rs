#![cfg(feature = "database-tests")]
mod support;

use board_domain::{anonymous_session::Capability, poster_id::PublicReportRateIdentity};
use board_store::{NewPost, StoreError, anonymous_session::PostingSession};
use sqlx::PgPool;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
struct ReportSessions(Arc<Mutex<Vec<[u8; 32]>>>);

impl ReportSessions {
    fn fresh(&self, peer: u8) -> PostingSession {
        let capability = Capability::generate().unwrap();
        self.0.lock().unwrap().push(capability.storage_hash());
        PostingSession {
            fingerprints: capability
                .fingerprints(Some(std::net::IpAddr::from([192, 0, 2, peer])), *b"US"),
            minted: true,
            now: chrono::Utc::now(),
        }
    }
}

const DISABLED: &str = "You cannot report posts on this board.";
const STICKY: &str = "Error: You cannot report a sticky.";
const CAPCODE: &str = "Error: You cannot report this post.";

fn report_identity(board: &str, peer: u8) -> PublicReportRateIdentity {
    support::key(board).public_report_rate_identity(std::net::IpAddr::from([192, 0, 2, peer]))
}

async fn rejected(
    public: &PgPool,
    board: &str,
    id: i64,
    expected: &str,
    sessions: &ReportSessions,
) {
    assert_eq!(
        board_store::report_target(public, board, id)
            .await
            .unwrap_err()
            .to_string(),
        expected
    );
    assert_eq!(
        board_store::report_with_anonymous_session(
            public,
            board,
            id,
            "Owned rejection fixture",
            &report_identity(board, 201),
            sessions.fresh(201)
        )
        .await
        .unwrap_err()
        .to_string(),
        expected
    );
}

#[tokio::test]
async fn report_targets_follow_source_policy_without_exposing_private_activity() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let seed: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&owner)
        .await
        .unwrap();
    let slug = format!("r{seed:x}");
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) VALUES($1,'Owned report fixture','Synthetic',2000,100,100,100,10,0,0,0)")
        .bind(&slug).execute(&owner).await.unwrap();
    let capability = Capability::generate().unwrap();
    let token = capability.storage_hash();
    let session = PostingSession {
        fingerprints: capability
            .fingerprints(Some(std::net::IpAddr::from([192, 0, 2, 201])), *b"US"),
        minted: true,
        now: chrono::Utc::now(),
    };
    let sessions = ReportSessions::default();
    let test_sessions = sessions.clone();
    let test_owner = owner.clone();
    let test_public = public.clone();
    let test_slug = slug.clone();
    let result = tokio::spawn(async move {
        let sessions = &test_sessions;
        let owner = &test_owner;
        let public = &test_public;
        let slug = &test_slug;
        let post = NewPost {
            name: "Anonymous".into(),
            subject: "Owned report fixture".into(),
            comment: "Owned report target content".into(),
            deletion_hash: "owned-report-hash".into(),
            sage: false,
        };
        let op = support::create_post(public, slug, 0, &post).await.unwrap();
        let reply = support::create_post(public, slug, op, &post).await.unwrap();
        let target = board_store::report_target(public, slug, reply).await.unwrap();
        assert_eq!((target.board.as_str(), target.post_id, target.thread_id), (slug.as_str(), reply, op));
        for worksafe in [false, true] {
            sqlx::query("UPDATE content.boards SET worksafe=$2 WHERE slug=$1")
                .bind(slug).bind(worksafe).execute(owner).await.unwrap();
            assert_eq!(board_store::report_target(public, slug, reply).await.unwrap().worksafe, worksafe);
        }
        rejected(public, slug, -1, "Not found.", sessions).await;
        rejected(public, "missing", op, "Not found.", sessions).await;

        sqlx::query("UPDATE content.threads SET sticky=true,closed=true WHERE id=$1")
            .bind(op).execute(owner).await.unwrap();
        sqlx::query("UPDATE content.posts SET capcode='mod' WHERE id=$1")
            .bind(op).execute(owner).await.unwrap();
        rejected(public, slug, op, STICKY, sessions).await;
        // Sticky and closed are properties of the thread, not reply rejection.
        board_store::report_with_anonymous_session(public, slug, reply, "Eligible sticky-thread reply", &report_identity(slug, 201), sessions.fresh(201)).await.unwrap();
        sqlx::query("UPDATE content.posts SET capcode='mod' WHERE id=$1")
            .bind(reply).execute(owner).await.unwrap();
        rejected(public, slug, reply, CAPCODE, sessions).await;
        sqlx::query("UPDATE content.posts SET capcode=NULL WHERE id=$1")
            .bind(reply).execute(owner).await.unwrap();
        sqlx::query("UPDATE content.threads SET sticky=false WHERE id=$1")
            .bind(op).execute(owner).await.unwrap();
        for capcode in ["mod", "admin", "admin_highlight", "manager", "developer", "founder"] {
            sqlx::query("UPDATE content.posts SET capcode=$2 WHERE id=$1")
                .bind(op).bind(capcode).execute(owner).await.unwrap();
            rejected(public, slug, op, CAPCODE, sessions).await;
        }
        sqlx::query("UPDATE content.boards SET can_report_posts=false WHERE slug=$1")
            .bind(slug).execute(owner).await.unwrap();
        rejected(public, slug, op, DISABLED, sessions).await;
        rejected(public, slug, -1, DISABLED, sessions).await;
        let before: i64 = sqlx::query_scalar("SELECT count(*) FROM content.reports WHERE board=$1")
            .bind(slug).fetch_one(owner).await.unwrap();
        assert_eq!(board_store::report_with_anonymous_session(public, slug, reply, "Rejected anonymous report", &report_identity(slug, 201), session).await.unwrap_err().to_string(), DISABLED);
        let after: i64 = sqlx::query_scalar("SELECT count(*) FROM content.reports WHERE board=$1")
            .bind(slug).fetch_one(owner).await.unwrap();
        assert_eq!(before, after);
        let session_count: i64 = sqlx::query_scalar("SELECT count(*) FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
            .bind(token.as_slice()).fetch_one(owner).await.unwrap();
        assert_eq!(session_count, 0, "Rejected targets do not create anonymous activity");
        sqlx::query("UPDATE content.boards SET staff_only=true WHERE slug=$1")
            .bind(slug).execute(owner).await.unwrap();
        rejected(public, slug, op, "Not found.", sessions).await;
        sqlx::query("UPDATE content.boards SET staff_only=false,can_report_posts=true,archive_retention_seconds=3600 WHERE slug=$1")
            .bind(slug).execute(owner).await.unwrap();
        sqlx::query("UPDATE content.posts SET capcode=NULL WHERE id=$1")
            .bind(op).execute(owner).await.unwrap();
        // Independent eligibility checks use distinct trusted fixture peers.
        board_store::report_with_anonymous_session(public, slug, op, "Eligible closed OP", &report_identity(slug, 202), sessions.fresh(202)).await.unwrap();
        sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp()-interval '1 minute',archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
            .bind(op).execute(owner).await.unwrap();
        board_store::report_with_anonymous_session(public, slug, op, "Eligible retained archive", &report_identity(slug, 203), sessions.fresh(203)).await.unwrap();
        sqlx::query("UPDATE content.threads SET archive_expires_at=clock_timestamp()-interval '1 second' WHERE id=$1")
            .bind(op).execute(owner).await.unwrap();
        rejected(public, slug, op, "Not found.", sessions).await;
        sqlx::query("UPDATE content.threads SET archived_at=NULL,archive_expires_at=NULL WHERE id=$1")
            .bind(op).execute(owner).await.unwrap();
        sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1")
            .bind(reply).execute(owner).await.unwrap();
        rejected(public, slug, reply, "Not found.", sessions).await;
        sqlx::query("UPDATE content.threads SET deleted=true WHERE id=$1")
            .bind(op).execute(owner).await.unwrap();
        rejected(public, slug, op, "Not found.", sessions).await;
        // Fresh erasure is irreversible. Use a new live target for the
        // independent stale-GET admission check rather than reviving a tombstone.
        let op = support::create_post(public, slug, 0, &post).await.unwrap();

        // A previously eligible GET cannot authorize a report after policy
        // changes while the POST is queued on the ordinary board lock.
        board_store::report_target(public, slug, op).await.unwrap();
        let mut lock = owner.begin().await.unwrap();
        sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
            .bind(slug).fetch_one(&mut *lock).await.unwrap();
        let queued_public = public.clone();
        let queued_slug = slug.clone();
        let queued_session = sessions.fresh(204);
        let mut queued = tokio::spawn(async move {
            board_store::report_with_anonymous_session(&queued_public, &queued_slug, op, "Queued stale target", &report_identity(&queued_slug, 204), queued_session).await
        });
        assert!(tokio::time::timeout(std::time::Duration::from_millis(100), &mut queued).await.is_err());
        sqlx::query("UPDATE content.boards SET can_report_posts=false WHERE slug=$1")
            .bind(slug).execute(&mut *lock).await.unwrap();
        lock.commit().await.unwrap();
        assert_eq!(queued.await.unwrap().unwrap_err().to_string(), DISABLED);
        assert!(matches!(board_store::report_with_anonymous_session(public, slug, op, " ", &report_identity(slug, 201), sessions.fresh(201)).await, Err(StoreError::Invalid("Report reason must contain 1 to 1000 bytes."))));
        assert!(sqlx::query("SELECT * FROM content.reports").fetch_all(public).await.is_err());
    }).await;
    support::cleanup_posting(&owner, &slug).await;
    for query in [
        "DELETE FROM content.reports WHERE board=$1",
        "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(query)
            .bind(&slug)
            .execute(&owner)
            .await
            .unwrap();
    }
    let mut tokens = sessions.0.lock().unwrap().clone();
    tokens.push(token);
    for token in tokens {
        sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
            .bind(token.as_slice())
            .execute(&owner)
            .await
            .unwrap();
    }
    public.close().await;
    owner.close().await;
    result.unwrap();
}
