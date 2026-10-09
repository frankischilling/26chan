#![cfg(feature = "database-tests")]
mod support;

// Source modes/report.php:110-168, IP OR captured automatic identity. These fixtures do not
// manufacture report categories, legacy password authority, or Pass identity.

use board_domain::{anonymous_session::Capability, poster_id::PublicReportRateIdentity};
use board_store::{NewPost, StoreError, anonymous_session::PostingSession};
use chrono::{DateTime, Utc};
use sqlx::{PgConnection, PgPool, Postgres, Transaction};
use std::{future::Future, sync::Arc};

const DUPLICATE: &str = "You have already reported this post.";
const FLOOD: &str = "You have to wait a while before reporting another post.";

// Unique boards and actors cannot isolate the database-wide admission gate.
// Some cases deliberately hold it while observing other lock dependencies;
// unrelated fixtures must not consume their ordinary production lock timeout
// waiting behind those probes. Concurrency inside each case remains unchanged.
static FIXTURE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone)]
struct Fixture {
    owner: PgPool,
    public: PgPool,
    boards: [String; 2],
    // One OP and three replies per board; all created through real admission.
    posts: [[i64; 4]; 2],
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
        let seed: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
            .fetch_one(&owner)
            .await
            .unwrap();
        let boards = [format!("ra{seed:x}"), format!("rb{seed:x}")];
        let mut posts = [[0; 4]; 2];
        for (index, board) in boards.iter().enumerate() {
            sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,archive_retention_seconds) VALUES($1,'Report admission fixture','Synthetic',2000,100,100,100,10,0,0,0,3600)")
                .bind(board).execute(&owner).await.unwrap();
            let post = NewPost {
                name: "Anonymous".into(),
                subject: "Report admission".into(),
                comment: "Owned report admission target".into(),
                deletion_hash: "report-admission-fixture".into(),
                sage: false,
            };
            posts[index][0] = support::create_post(&public, board, 0, &post)
                .await
                .unwrap();
            for target in 1..4 {
                posts[index][target] = support::create_post(&public, board, posts[index][0], &post)
                    .await
                    .unwrap();
            }
        }
        Self {
            owner,
            public,
            boards,
            posts,
        }
    }

    async fn cleanup(&self) {
        let tokens: Vec<Vec<u8>> = sqlx::query_scalar("SELECT a.token_hash FROM post_secrets.anonymous_reports a JOIN content.reports r ON r.id=a.report_id WHERE r.board=ANY($1)")
            .bind(self.boards.to_vec()).fetch_all(&self.owner).await.unwrap();
        for board in &self.boards {
            support::cleanup_posting(&self.owner, board).await;
            for query in [
                "DELETE FROM content.reports WHERE board=$1",
                "DELETE FROM content.post_media WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
                "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
                "DELETE FROM content.posts WHERE board=$1",
                "DELETE FROM content.threads WHERE board=$1",
                "DELETE FROM content.boards WHERE slug=$1",
            ] {
                sqlx::query(query)
                    .bind(board)
                    .execute(&self.owner)
                    .await
                    .unwrap();
            }
        }
        for token in tokens {
            sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
                .bind(token)
                .execute(&self.owner)
                .await
                .unwrap();
        }
    }

    async fn counts(&self) -> (i64, i64) {
        let reports: i64 =
            sqlx::query_scalar("SELECT count(*) FROM content.reports WHERE board=ANY($1)")
                .bind(self.boards.to_vec())
                .fetch_one(&self.owner)
                .await
                .unwrap();
        let mut tx = self.owner.begin().await.unwrap();
        private_role(&mut tx).await;
        let members: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM post_secrets.report_membership WHERE board=ANY($1)",
        )
        .bind(self.boards.to_vec())
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        tx.rollback().await.unwrap();
        (reports, members)
    }

    async fn member(&self, report: i64) -> bool {
        let mut tx = self.owner.begin().await.unwrap();
        private_role(&mut tx).await;
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM post_secrets.report_membership WHERE report_id=$1)",
        )
        .bind(report)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        tx.rollback().await.unwrap();
        exists
    }

    async fn admit(
        &self,
        board: usize,
        target: usize,
        actor: &PublicReportRateIdentity,
    ) -> Result<i64, sqlx::Error> {
        let mut connection = self.public.acquire().await.unwrap();
        admit(
            &mut connection,
            &self.boards[board],
            self.posts[board][target],
            actor.as_bytes(),
        )
        .await
    }
}

async fn run<F, Fut>(test: F)
where
    F: FnOnce(Fixture) -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
{
    let _fixture_guard = FIXTURE_LOCK.lock().await;
    let fixture = Fixture::new().await;
    let result = tokio::spawn(test(fixture.clone())).await;
    fixture.cleanup().await;
    result.unwrap();
}

fn actor() -> PublicReportRateIdentity {
    support::fresh_key().public_report_rate_identity("198.51.100.7".parse().unwrap())
}

fn code(error: &sqlx::Error) -> String {
    error
        .as_database_error()
        .unwrap()
        .code()
        .unwrap()
        .into_owned()
}

fn rejected(error: sqlx::Error, message: &str) {
    assert_eq!(code(&error), "P0001");
    assert_eq!(error.as_database_error().unwrap().message(), message);
}

async fn private_role(tx: &mut Transaction<'_, Postgres>) {
    sqlx::query("SET LOCAL ROLE board_report_admission_owner")
        .execute(&mut **tx)
        .await
        .unwrap();
}

async fn admit(
    connection: &mut PgConnection,
    board: &str,
    post: i64,
    actor: &[u8],
) -> Result<i64, sqlx::Error> {
    let capability = Capability::generate().unwrap();
    admit_session(
        connection,
        board,
        post,
        Some("Owned admission fixture"),
        Some(actor),
        session(&capability, true, Utc::now()),
    )
    .await
}

fn session(capability: &Capability, minted: bool, now: DateTime<Utc>) -> PostingSession {
    PostingSession {
        fingerprints: capability.fingerprints(Some("198.51.100.7".parse().unwrap()), *b"US"),
        minted,
        now,
    }
}

async fn admit_session(
    connection: &mut PgConnection,
    board: &str,
    post: i64,
    reason: Option<&str>,
    actor: Option<&[u8]>,
    session: PostingSession,
) -> Result<i64, sqlx::Error> {
    let fingerprints = session.fingerprints;
    sqlx::query_scalar("SELECT content.admit_report($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
        .bind(board)
        .bind(post)
        .bind(reason)
        .bind(actor)
        .bind(fingerprints.token.as_slice())
        .bind(fingerprints.network.as_slice())
        .bind(fingerprints.address.as_slice())
        .bind(fingerprints.environment.as_slice())
        .bind(session.minted)
        .bind(session.now.timestamp())
        .fetch_one(connection)
        .await
}

async fn report(
    pool: &PgPool,
    board: &str,
    post: i64,
    reason: &str,
    actor: &PublicReportRateIdentity,
) -> Result<(), StoreError> {
    let capability = Capability::generate().unwrap();
    board_store::report_with_anonymous_session(
        pool,
        board,
        post,
        reason,
        actor,
        session(&capability, true, Utc::now()),
    )
    .await
}

// Historical membership is fixture metadata, never a public clock override.
// Both report and membership disappear with this transaction's rollback.
async fn boundary(f: &Fixture, offsets_us: &[i64], duplicate: bool, expected: Option<&str>) {
    // Preserve the private IP-only contract and repeat every edge for identity
    // only and overlapping IP+UUID predicates. OR must count each row once.
    for mode in 0..4 {
        let actor = actor();
        let row_actor = if mode == 1 { self::actor() } else { actor };
        let now: DateTime<Utc> = "2025-01-15T12:00:00Z".parse().unwrap();
        let mut tx = f.owner.begin().await.unwrap();
        let reports: Vec<i64> = sqlx::query_scalar("INSERT INTO content.reports(board,post_id,reason) SELECT $1,$2,'Historical owned fixture' FROM generate_series(1,$3::integer) RETURNING id")
        .bind(&f.boards[0]).bind(f.posts[0][1]).bind(offsets_us.len() as i32).fetch_all(&mut *tx).await.unwrap();
        private_role(&mut tx).await;
        sqlx::query("INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at) SELECT r,$1,$2,$3,$4,$5::timestamptz+o*interval '1 microsecond' FROM unnest($6::bigint[],$7::bigint[]) AS fixture(r,o)")
        .bind(row_actor.as_bytes().as_slice()).bind(&f.boards[0]).bind(f.posts[0][1]).bind(f.posts[0][0]).bind(now).bind(&reports).bind(offsets_us).execute(&mut *tx).await.unwrap();
        if mode != 0 {
            sqlx::query("UPDATE post_secrets.report_membership SET automatic_identity='00000000-0000-4000-8000-000000000097'::uuid WHERE report_id=ANY($1)")
            .bind(&reports).execute(&mut *tx).await.unwrap();
        }
        if mode == 3 {
            // Disjoint IP-only and UUID-only rows must contribute to one global
            // count, rather than comparing each branch against its own threshold.
            sqlx::query("UPDATE post_secrets.report_membership SET actor_hash=CASE WHEN report_id%2=0 THEN sha256(actor_hash) ELSE actor_hash END,automatic_identity=CASE WHEN report_id%2=0 THEN automatic_identity ELSE NULL END WHERE report_id=ANY($1)")
            .bind(&reports).execute(&mut *tx).await.unwrap();
        }
        let query = if mode == 0 {
            "SELECT post_secrets.check_report_limits($1,$2,$3,$4)"
        } else {
            "SELECT post_secrets.check_report_limits($1,$2,$3,'00000000-0000-4000-8000-000000000097'::uuid,$4)"
        };
        let result = sqlx::query(query)
            .bind(&f.boards[0])
            .bind(f.posts[0][if duplicate { 1 } else { 2 }])
            .bind(actor.as_bytes().as_slice())
            .bind(now)
            .execute(&mut *tx)
            .await;
        match expected {
            Some(message) => rejected(result.unwrap_err(), message),
            None => {
                result.unwrap();
            }
        }
        tx.rollback().await.unwrap();
    }
}

#[tokio::test]
async fn source_strict_edges_counts_duplicate_precedence_and_future_rows() {
    run(|f| async move {
        boundary(&f, &[], false, None).await;
        boundary(&f, &[-15_000_001], false, None).await;
        boundary(&f, &[-15_000_000], false, None).await;
        boundary(&f, &[-14_999_999], false, Some(FLOOD)).await;
        boundary(&f, &[1_000_000], false, Some(FLOOD)).await;
        boundary(&f, &[-60_000_000; 29], false, None).await;
        boundary(&f, &[-60_000_000; 30], false, Some(FLOOD)).await;
        boundary(&f, &[-3_600_000_000; 30], false, None).await;
        boundary(&f, &[-3_599_999_999; 30], false, Some(FLOOD)).await;
        let mut hour = [-3_599_999_999; 30];
        hour[0] = -3_600_000_000;
        boundary(&f, &hour, false, None).await;
        boundary(&f, &[-7_200_000_000; 79], false, None).await;
        boundary(&f, &[-7_200_000_000; 80], false, Some(FLOOD)).await;
        boundary(&f, &[-86_400_000_000; 80], false, None).await;
        boundary(&f, &[-86_399_999_999; 80], false, Some(FLOOD)).await;
        let mut day = [-86_399_999_999; 80];
        day[0] = -86_400_000_000;
        boundary(&f, &day, false, None).await;
        boundary(&f, &[0; 80], true, Some(DUPLICATE)).await;
        boundary(&f, &[-31_536_000_000_000], true, Some(DUPLICATE)).await;
        assert_eq!(f.counts().await, (0, 0));
    })
    .await;
}

#[tokio::test]
async fn typed_ip_identity_is_cross_board_and_get_is_read_only_without_reservation() {
    run(|f| async move {
        let key = support::fresh_key();
        let identity = key.public_report_rate_identity("198.51.100.7".parse().unwrap());
        let same_ip = key.public_report_rate_identity("198.51.100.7".parse().unwrap());
        let other_ip = key.public_report_rate_identity("198.51.100.8".parse().unwrap());
        let mut read = f.public.begin().await.unwrap();
        sqlx::query("SET TRANSACTION READ ONLY")
            .execute(&mut *read)
            .await
            .unwrap();
        for _ in 0..2 {
            sqlx::query("SELECT content.check_report_admission($1,$2,$3)")
                .bind(&f.boards[0])
                .bind(f.posts[0][0])
                .bind(identity.as_bytes().as_slice())
                .execute(&mut *read)
                .await
                .unwrap();
        }
        read.commit().await.unwrap();
        assert_eq!(f.counts().await, (0, 0));
        let mut locked = f.owner.begin().await.unwrap();
        sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
            .bind(&f.boards[0])
            .execute(&mut *locked)
            .await
            .unwrap();
        private_role(&mut locked).await;
        sqlx::query("SELECT singleton FROM post_secrets.report_admission_gate FOR UPDATE")
            .execute(&mut *locked)
            .await
            .unwrap();
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            board_store::report_admission::check(&f.public, &f.boards[0], f.posts[0][0], &identity),
        )
        .await
        .expect("Advisory GET must not wait for the board or gate row")
        .unwrap();
        locked.rollback().await.unwrap();
        board_store::report_admission::check(&f.public, &f.boards[1], f.posts[1][0], &same_ip)
            .await
            .unwrap();
        report(
            &f.public,
            &f.boards[0],
            f.posts[0][0],
            "First report",
            &identity,
        )
        .await
        .unwrap();
        assert_eq!(
            report(
                &f.public,
                &f.boards[0],
                f.posts[0][0],
                "Duplicate",
                &same_ip
            )
            .await
            .unwrap_err()
            .to_string(),
            DUPLICATE
        );
        assert_eq!(
            board_store::report_admission::check(&f.public, &f.boards[1], f.posts[1][0], &same_ip)
                .await
                .unwrap_err()
                .to_string(),
            FLOOD
        );
        assert_eq!(
            report(
                &f.public,
                &f.boards[1],
                f.posts[1][0],
                "Advisory GET is stale",
                &same_ip
            )
            .await
            .unwrap_err()
            .to_string(),
            FLOOD
        );
        report(
            &f.public,
            &f.boards[1],
            f.posts[1][0],
            "Different transport peer",
            &other_ip,
        )
        .await
        .unwrap();
        assert_eq!(f.counts().await, (2, 2));
    })
    .await;
}

#[tokio::test]
async fn concurrent_cross_board_admissions_have_one_committed_winner() {
    run(|f| async move {
        let identity = Arc::new(actor());
        let barrier = Arc::new(tokio::sync::Barrier::new(8));
        let mut tasks = Vec::new();
        for n in 0..8 {
            let f = f.clone();
            let identity = identity.clone();
            let barrier = barrier.clone();
            tasks.push(tokio::spawn(async move {
                barrier.wait().await;
                f.admit(n % 2, n / 2, &identity).await
            }));
        }
        let mut winners = 0;
        for task in tasks {
            match task.await.unwrap() {
                Ok(_) => winners += 1,
                Err(error) => rejected(error, FLOOD),
            }
        }
        assert_eq!(winners, 1);
        assert_eq!(f.counts().await, (1, 1));
    })
    .await;
}

#[tokio::test]
async fn rollback_releases_admission_and_repeatable_read_and_serializable_fail_closed() {
    run(|f| async move {
        let identity = actor();
        for isolation in [
            "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ",
            "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE",
        ] {
            let mut tx = f.public.begin().await.unwrap();
            sqlx::query(isolation).execute(&mut *tx).await.unwrap();
            let error = admit(&mut tx, &f.boards[0], f.posts[0][0], identity.as_bytes())
                .await
                .unwrap_err();
            assert_eq!(code(&error), "22023");
            assert_eq!(
                error.as_database_error().unwrap().message(),
                "Report admission requires Read Committed."
            );
            tx.rollback().await.unwrap();
        }
        let mut tx = f.public.begin().await.unwrap();
        admit(&mut tx, &f.boards[0], f.posts[0][0], identity.as_bytes())
            .await
            .unwrap();
        assert!(sqlx::query("SELECT 1/0").execute(&mut *tx).await.is_err());
        tx.rollback().await.unwrap();
        assert_eq!(f.counts().await, (0, 0));
        f.admit(1, 0, &identity).await.unwrap();
        assert_eq!(f.counts().await, (1, 1));
    })
    .await;
}

#[tokio::test]
async fn default_lock_timeout_fails_closed_without_report_or_session_reservation() {
    run(|f| async move {
        let identity = actor();
        let capability = Capability::generate().unwrap();
        let mut connection = f.public.acquire().await.unwrap();
        let lock_timeout: String = sqlx::query_scalar("SHOW lock_timeout")
            .fetch_one(&mut *connection)
            .await
            .unwrap();
        assert_eq!(lock_timeout, "2s", "Use the unchanged board_public default");

        let mut gate = f.owner.begin().await.unwrap();
        private_role(&mut gate).await;
        sqlx::query(
            "SELECT singleton FROM post_secrets.report_admission_gate WHERE singleton FOR UPDATE",
        )
        .fetch_one(&mut *gate)
        .await
        .unwrap();
        // Hold only the global gate until the real public admission times out.
        // Fixture isolation must not hide or relax production fail-closed behavior.
        let result = admit_session(
            &mut connection,
            &f.boards[0],
            f.posts[0][0],
            Some("Blocked by the admission gate"),
            Some(identity.as_bytes()),
            session(&capability, true, Utc::now()),
        )
        .await;
        gate.rollback().await.unwrap();
        assert_eq!(code(&result.unwrap_err()), "55P03");
        assert_eq!(f.counts().await, (0, 0));
        assert!(
            board_store::anonymous_session::snapshot(&f.public, &capability.storage_hash())
                .await
                .unwrap()
                .is_none()
        );

        admit_session(
            &mut connection,
            &f.boards[0],
            f.posts[0][0],
            Some("Admission after releasing the gate"),
            Some(identity.as_bytes()),
            session(&capability, true, Utc::now()),
        )
        .await
        .unwrap();
        assert_eq!(f.counts().await, (1, 1));
        assert!(
            board_store::anonymous_session::snapshot(&f.public, &capability.storage_hash())
                .await
                .unwrap()
                .is_some()
        );
    })
    .await;
}

#[tokio::test]
async fn anonymous_failure_is_atomic_and_session_collection_does_not_retire_reports() {
    run(|f| async move {
        let identity = actor();
        let capability = Capability::generate().unwrap();
        let token = capability.storage_hash();
        let make_session = |minted| PostingSession {
            fingerprints: capability.fingerprints(Some("198.51.100.7".parse().unwrap()), *b"US"),
            minted,
            now: Utc::now(),
        };
        assert!(matches!(
            board_store::report_with_anonymous_session(
                &f.public,
                &f.boards[0],
                f.posts[0][0],
                "Missing session",
                &identity,
                make_session(false)
            )
            .await,
            Err(StoreError::AuthorizationChanged)
        ));
        assert_eq!(f.counts().await, (0, 0));
        board_store::report_with_anonymous_session(
            &f.public,
            &f.boards[0],
            f.posts[0][0],
            "Committed session report",
            &identity,
            make_session(true),
        )
        .await
        .unwrap();
        let rotated = Capability::generate().unwrap();
        let rotated_session = PostingSession {
            fingerprints: rotated.fingerprints(Some("198.51.100.7".parse().unwrap()), *b"US"),
            minted: true,
            now: Utc::now(),
        };
        assert_eq!(
            board_store::report_with_anonymous_session(
                &f.public,
                &f.boards[0],
                f.posts[0][0],
                "A new cookie is not a new reporting peer",
                &identity,
                rotated_session
            )
            .await
            .unwrap_err()
            .to_string(),
            DUPLICATE
        );
        assert!(
            board_store::anonymous_session::snapshot(&f.public, &rotated.storage_hash())
                .await
                .unwrap()
                .is_none()
        );
        let report: i64 = sqlx::query_scalar("SELECT id FROM content.reports WHERE board=$1")
            .bind(&f.boards[0])
            .fetch_one(&f.owner)
            .await
            .unwrap();
        // This is the same FK cascade used when the bounded session collector
        // removes an expired row. No membership has a session FK or expiry.
        sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
            .bind(token.as_slice())
            .execute(&f.owner)
            .await
            .unwrap();
        let anonymous: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM post_secrets.anonymous_reports WHERE report_id=$1",
        )
        .bind(report)
        .fetch_one(&f.owner)
        .await
        .unwrap();
        assert_eq!(anonymous, 0);
        for state in ["resolved", "dismissed"] {
            sqlx::query("UPDATE content.reports SET state=$2 WHERE id=$1")
                .bind(report)
                .bind(state)
                .execute(&f.owner)
                .await
                .unwrap();
            assert!(f.member(report).await);
            rejected(f.admit(0, 0, &identity).await.unwrap_err(), DUPLICATE);
            rejected(f.admit(1, 0, &identity).await.unwrap_err(), FLOOD);
        }
        assert_eq!(f.counts().await, (1, 1));
    })
    .await;
}

#[tokio::test]
async fn public_cannot_bypass_admission_or_read_private_membership() {
    run(|f| async move {
        for query in [
            "SELECT content.admit_report('missing',1,'Denied old mutation',decode(repeat('00',32),'hex'))",
            "SELECT post_secrets.check_report_limits('missing',1,decode(repeat('00',32),'hex'),NULL::uuid,clock_timestamp())",
            "SELECT * FROM post_secrets.report_membership",
            "SELECT * FROM post_secrets.report_admission_gate",
            "DELETE FROM post_secrets.report_membership WHERE false",
            "SELECT post_secrets.check_report_limits('missing',1,decode(repeat('00',32),'hex'),clock_timestamp())",
            "SELECT post_secrets.retire_staff_file_report_membership('missing',1)",
            "SELECT content.staff_delete_post_attachment('missing',1)",
            "SET ROLE board_report_admission_owner",
        ] {
            assert_eq!(code(&sqlx::query(query).execute(&f.public).await.unwrap_err()), "42501", "{query}");
        }
        let error = sqlx::query("INSERT INTO content.reports(board,post_id,reason) VALUES($1,$2,'Bypass')")
            .bind(&f.boards[0]).bind(f.posts[0][0]).execute(&f.public).await.unwrap_err();
        assert_eq!(code(&error), "42501");
        for role in ["board_public", "board_staff", "board_auth", "board_media", "board_media_read", "board_monitor", "board_media_intake"] {
            for table in ["post_secrets.report_membership", "post_secrets.report_admission_gate"] {
                let privilege: bool = sqlx::query_scalar("SELECT has_table_privilege($1,$2,'SELECT,INSERT,UPDATE,DELETE,TRUNCATE,REFERENCES,TRIGGER')")
                    .bind(role).bind(table).fetch_one(&f.owner).await.unwrap();
                assert!(!privilege, "{role} must not access {table}");
            }
        }
        for function in ["content.admit_report(text,bigint,text,bytea,bytea,bytea,bytea,bytea,boolean,bigint)", "content.check_report_admission(text,bigint,bytea)", "content.check_report_admission(text,bigint,bytea,bytea,bigint)"] {
            let locked: bool = sqlx::query_scalar("SELECT p.prosecdef AND p.proconfig=ARRAY['search_path=pg_catalog, pg_temp'] AND r.rolname='board_report_admission_owner' AND NOT(r.rolcanlogin OR r.rolsuper OR r.rolcreatedb OR r.rolcreaterole OR r.rolreplication OR r.rolbypassrls) AND NOT has_schema_privilege(r.oid,'content','CREATE') AND NOT has_schema_privilege(r.oid,'post_secrets','CREATE') AND has_function_privilege('board_public',p.oid,'EXECUTE') AND NOT EXISTS(SELECT 1 FROM aclexplode(p.proacl) a WHERE a.grantee=0 AND a.privilege_type='EXECUTE') FROM pg_proc p JOIN pg_roles r ON r.oid=p.proowner WHERE p.oid=$1::regprocedure")
                .bind(function).fetch_one(&f.owner).await.unwrap();
            assert!(locked, "{function}");
        }
        let membership: bool = sqlx::query_scalar("SELECT m.set_option AND NOT m.inherit_option AND NOT m.admin_option FROM pg_auth_members m JOIN pg_roles r ON r.oid=m.roleid JOIN pg_roles u ON u.oid=m.member WHERE r.rolname='board_report_admission_owner' AND u.rolname='board_migrator'")
            .fetch_one(&f.owner).await.unwrap();
        assert!(membership);
        for value in [None, Some(vec![]), Some(vec![0_u8;31]), Some(vec![0_u8;33])] {
            let mut connection = f.public.acquire().await.unwrap();
            let capability = Capability::generate().unwrap();
            let error = admit_session(&mut connection, &f.boards[0], f.posts[0][0], Some("Malformed identity"), value.as_deref(), session(&capability, true, Utc::now())).await.unwrap_err();
            assert_eq!(code(&error), "22023");
            let error = sqlx::query("SELECT content.check_report_admission($1,$2,$3)")
                .bind(&f.boards[0]).bind(f.posts[0][0]).bind(value.as_deref()).execute(&f.public).await.unwrap_err();
            assert_eq!(code(&error), "22023");
        }
        assert_eq!(f.counts().await, (0, 0));
    }).await;
}

#[tokio::test]
async fn post_and_thread_deletion_remove_only_matching_reports_and_archive_only_retains() {
    run(|f| async move {
        let mut reports = Vec::new();
        for board in 0..2 {
            for target in 0..3 { reports.push(f.admit(board, target, &actor()).await.unwrap()); }
        }
        let mut incompatible = f.owner.begin().await.unwrap();
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *incompatible).await.unwrap();
        let error = sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1")
            .bind(f.posts[0][1]).execute(&mut *incompatible).await.unwrap_err();
        assert_eq!(code(&error), "22023");
        assert_eq!(error.as_database_error().unwrap().message(), "Report retirement requires Read Committed.");
        incompatible.rollback().await.unwrap();
        let mut tx = f.owner.begin().await.unwrap();
        sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1").bind(f.posts[0][1]).execute(&mut *tx).await.unwrap();
        // A rolled-back deletion must roll back its trigger retirement too.
        tx.rollback().await.unwrap();
        assert!(f.member(reports[1]).await);
        sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1").bind(f.posts[0][1]).execute(&f.owner).await.unwrap();
        assert!(!f.member(reports[1]).await);
        assert!(f.member(reports[0]).await);
        assert!(f.member(reports[2]).await);
        sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
            .bind(f.posts[0][0]).execute(&f.owner).await.unwrap();
        assert!(f.member(reports[0]).await);
        assert!(f.member(reports[2]).await);
        sqlx::query("UPDATE content.threads SET deleted=true WHERE id=$1").bind(f.posts[0][0]).execute(&f.owner).await.unwrap();
        assert!(!f.member(reports[0]).await);
        assert!(!f.member(reports[2]).await);
        for report in &reports[3..] { assert!(f.member(*report).await); }
        assert_eq!(f.counts().await, (3, 3), "Whole deletion removes report records, unlike archive-only retirement");
    }).await;
}

#[tokio::test]
async fn public_file_only_retains_staff_fresh_removal_retires_and_repeat_does_not() {
    run(|f| async move {
        let staff = PgPool::connect(&std::env::var("STAFF_DATABASE_URL").unwrap()).await.unwrap();
        // Owned metadata fixture, not a forged upload capability or a new media
        // approval. The production file-removal functions need only this row.
        for target in [1, 2] {
            sqlx::query("INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler) VALUES($1,replace(gen_random_uuid()::text,'-',''),replace(gen_random_uuid()::text,'-',''),'owned.png',1,1,1,false)")
                .bind(f.posts[0][target]).execute(&f.owner).await.unwrap();
        }
        let public_report = f.admit(0, 1, &actor()).await.unwrap();
        let staff_report = f.admit(0, 2, &actor()).await.unwrap();
        sqlx::query("SELECT content.delete_post_attachment($1,$2)").bind(&f.boards[0]).bind(f.posts[0][1]).execute(&f.public).await.unwrap();
        assert!(f.member(public_report).await);
        let mut tx = staff.begin().await.unwrap();
        sqlx::query("SELECT content.staff_delete_post_attachment($1,$2)").bind(&f.boards[0]).bind(f.posts[0][2]).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        assert!(!f.member(staff_report).await);
        assert!(f.member(public_report).await);
        // New reports on the surviving post must not disappear on a repeated
        // staff click, nor on staff removal of an already publicly removed file.
        let after_removal = f.admit(0, 2, &actor()).await.unwrap();
        for target in [1, 2] {
            let mut tx = staff.begin().await.unwrap();
            let error = sqlx::query("SELECT content.staff_delete_post_attachment($1,$2)").bind(&f.boards[0]).bind(f.posts[0][target]).execute(&mut *tx).await.unwrap_err();
            assert_eq!(code(&error), "P0002");
            tx.rollback().await.unwrap();
        }
        assert!(f.member(public_report).await);
        assert!(f.member(after_removal).await);
        assert_eq!(f.counts().await, (3, 2));
    }).await;
}

#[tokio::test]
async fn admission_preserves_private_board_visibility_and_validates_reason_without_rows() {
    run(|f| async move {
        let identity = actor();
        for reason in [
            None,
            Some(String::new()),
            Some(" ".into()),
            Some("x".repeat(1001)),
        ] {
            let mut connection = f.public.acquire().await.unwrap();
            let capability = Capability::generate().unwrap();
            let error = admit_session(
                &mut connection,
                &f.boards[0],
                f.posts[0][0],
                reason.as_deref(),
                Some(identity.as_bytes().as_slice()),
                session(&capability, true, Utc::now()),
            )
            .await
            .unwrap_err();
            assert_eq!(code(&error), "22023");
        }
        sqlx::query("UPDATE content.boards SET staff_only=true WHERE slug=$1")
            .bind(&f.boards[0])
            .execute(&f.owner)
            .await
            .unwrap();
        assert!(matches!(
            board_store::report_admission::check(&f.public, &f.boards[0], f.posts[0][0], &identity)
                .await,
            Err(StoreError::NotFound)
        ));
        assert!(matches!(
            report(
                &f.public,
                &f.boards[0],
                f.posts[0][0],
                "Hidden board",
                &identity
            )
            .await,
            Err(StoreError::NotFound)
        ));
        assert_eq!(f.counts().await, (0, 0));
        let staff = PgPool::connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
            .await
            .unwrap();
        board_store::report_admission::check(&staff, &f.boards[0], f.posts[0][0], &identity)
            .await
            .unwrap();
        board_store::report(
            &staff,
            &f.boards[0],
            f.posts[0][0],
            "Staff-visible board",
            &identity,
        )
        .await
        .unwrap();
        assert_eq!(f.counts().await, (1, 1));
        let hidden: bool =
            sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM content.posts WHERE board=$1)")
                .bind(&f.boards[0])
                .fetch_one(&f.public)
                .await
                .unwrap();
        assert!(hidden);
    })
    .await;
}

#[tokio::test]
async fn rust_report_overrides_stronger_connection_default_isolation() {
    run(|f| async move {
        for (target, isolation) in ["repeatable read", "serializable"].into_iter().enumerate() {
            let public = sqlx::postgres::PgPoolOptions::new()
                .max_connections(1)
                .after_connect(move |connection, _| {
                    Box::pin(async move {
                        sqlx::query("SELECT set_config('default_transaction_isolation',$1,false)")
                            .bind(isolation)
                            .execute(connection)
                            .await?;
                        Ok(())
                    })
                })
                .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
                .await
                .unwrap();
            let mut control = public.begin().await.unwrap();
            let actual: String = sqlx::query_scalar("SHOW transaction_isolation")
                .fetch_one(&mut *control)
                .await
                .unwrap();
            assert_eq!(actual, isolation, "The connection really starts stronger transactions");
            control.rollback().await.unwrap();
            report(
                &public,
                &f.boards[0],
                f.posts[0][target],
                "Rust sets transaction-local Read Committed before admission",
                &actor(),
            )
            .await
            .unwrap();
            let defaults: (String, String) = sqlx::query_as(
                "SELECT current_setting('default_transaction_isolation'),current_setting('transaction_isolation')",
            )
            .fetch_one(&public)
            .await
            .unwrap();
            assert_eq!(defaults, (isolation.into(), isolation.into()), "The override must not change pooled connection defaults");
            public.close().await;
        }
        assert_eq!(f.counts().await, (2, 2));
    })
    .await;
}

// Observe a real, ungranted row-lock dependency on one particular backend.
// Polling yields between catalog reads; elapsed time alone is never evidence
// that a worker reached the intended point in its production transaction.
async fn wait_for_row_blocker(observer: &PgPool, waiter: i32, blocker: i32) {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        loop {
            let blocked: bool = sqlx::query_scalar(
                "SELECT $2=ANY(pg_blocking_pids($1)) AND EXISTS(SELECT 1 FROM pg_locks WHERE pid=$1 AND NOT granted AND locktype='transactionid')",
            )
            .bind(waiter)
            .bind(blocker)
            .fetch_one(observer)
            .await
            .unwrap();
            if blocked {
                return;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("The expected backend did not reach its exact row-lock dependency");
}

#[tokio::test]
async fn retirement_does_not_invert_gate_and_anonymous_session_lock_order() {
    run(|f| async move {
        let capability = Capability::generate().unwrap();
        let token = capability.storage_hash();
        let key = support::fresh_key();
        let previous_peer = "198.51.100.8".parse().unwrap();
        let current_peer = "198.51.100.7".parse().unwrap();
        let previous_actor = key.public_report_rate_identity(previous_peer);
        let current_actor = key.public_report_rate_identity(current_peer);
        board_store::report_with_anonymous_session(
            &f.public,
            &f.boards[1],
            f.posts[1][0],
            "Prior report to retire on board B",
            &previous_actor,
            PostingSession {
                fingerprints: capability.fingerprints(Some(previous_peer), *b"US"),
                minted: true,
                now: Utc::now(),
            },
        )
        .await
        .unwrap();
        let old_report: i64 = sqlx::query_scalar("SELECT id FROM content.reports WHERE board=$1")
            .bind(&f.boards[1])
            .fetch_one(&f.owner)
            .await
            .unwrap();

        let public = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .after_connect(|connection, _| {
                Box::pin(async move {
                    sqlx::query("SET lock_timeout='10s'")
                        .execute(&mut *connection)
                        .await?;
                    sqlx::query("SET statement_timeout='15s'")
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
            .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let admission_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&public)
            .await
            .unwrap();
        let mut deletion = f.owner.begin().await.unwrap();
        let deletion_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *deletion)
            .await
            .unwrap();
        // The real deletion order is board, then anonymous authority. The
        // trigger below is the actual post-deletion retirement implementation.
        sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
            .bind(&f.boards[1])
            .execute(&mut *deletion)
            .await
            .unwrap();
        sqlx::query("SELECT token_hash FROM post_secrets.anonymous_sessions WHERE token_hash=$1 FOR UPDATE")
            .bind(token.as_slice())
            .fetch_one(&mut *deletion)
            .await
            .unwrap();
        let board_a = f.boards[0].clone();
        let target_a = f.posts[0][0];
        let session = PostingSession {
            // A legitimate address change gives a different IP rate identity
            // while retaining the same anonymous session lock dependency.
            fingerprints: capability.fingerprints(Some(current_peer), *b"US"),
            minted: false,
            now: Utc::now(),
        };
        let admission = tokio::spawn(async move {
            let result = board_store::report_with_anonymous_session(
                &public,
                &board_a,
                target_a,
                "Admission owns board A and gate before waiting for session S",
                &current_actor,
                session,
            )
            .await;
            public.close().await;
            result
        });
        wait_for_row_blocker(&f.owner, admission_pid, deletion_pid).await;

        // Independently prove that the blocked production admission already
        // owns the gate. This probe never locks another board or any session.
        let mut gate = f.owner.begin().await.unwrap();
        let gate_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *gate)
            .await
            .unwrap();
        private_role(&mut gate).await;
        sqlx::query("SET LOCAL lock_timeout='10s'")
            .execute(&mut *gate)
            .await
            .unwrap();
        let gate_probe = tokio::spawn(async move {
            sqlx::query("SELECT singleton FROM post_secrets.report_admission_gate WHERE singleton FOR UPDATE")
                .fetch_one(&mut *gate)
                .await
                .unwrap();
            gate.rollback().await.unwrap();
        });
        wait_for_row_blocker(&f.owner, gate_pid, admission_pid).await;

        // At this point the observed graph is gate probe -> admission ->
        // deletion/session S. Retirement must not add deletion -> gate.
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1")
                .bind(f.posts[1][0])
                .execute(&mut *deletion),
        )
        .await
        .expect("Retirement waited for the gate while holding session S")
        .unwrap();
        private_role(&mut deletion).await;
        let retired: bool = sqlx::query_scalar(
            "SELECT NOT EXISTS(SELECT 1 FROM post_secrets.report_membership WHERE report_id=$1)",
        )
        .bind(old_report)
        .fetch_one(&mut *deletion)
        .await
        .unwrap();
        assert!(retired, "Retirement completed before releasing session S");
        deletion.commit().await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), admission)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(5), gate_probe)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(f.counts().await, (1, 1));
        assert!(!f.member(old_report).await);
        let linked: i64 = sqlx::query_scalar("SELECT count(*) FROM post_secrets.anonymous_reports WHERE token_hash=$1")
            .bind(token.as_slice())
            .fetch_one(&f.owner)
            .await
            .unwrap();
        assert_eq!(linked, 1, "Deletion cascaded old registration; the unblocked admission committed its new one");
        sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
            .bind(token.as_slice())
            .execute(&f.owner)
            .await
            .unwrap();
    })
    .await;
}

#[tokio::test]
async fn changed_ip_same_capability_enforces_identity_even_when_source_new() {
    run(|f| async move {
        let capability = Capability::generate().unwrap();
        let token = capability.storage_hash();
        let key = support::fresh_key();
        let first = key.public_report_rate_identity("198.51.100.7".parse().unwrap());
        let changed = key.public_report_rate_identity("203.0.113.7".parse().unwrap());
        let now = Utc::now();
        board_store::report_with_anonymous_session(&f.public, &f.boards[0], f.posts[0][0], "First identity report", &first, session(&capability, true, now)).await.unwrap();
        for idle in [false, true] {
            let request_at = Utc::now();
            sqlx::query("UPDATE post_secrets.anonymous_sessions SET created_at=$2,activity_at=$3,expires_at=$2+31536000 WHERE token_hash=$1")
                .bind(token.as_slice()).bind(if idle { request_at.timestamp()-604801 } else { request_at.timestamp() })
                .bind(if idle { request_at.timestamp()-604800 } else { request_at.timestamp() })
                .execute(&f.owner).await.unwrap();
            let before: serde_json::Value = sqlx::query_scalar("SELECT to_jsonb(s) FROM post_secrets.anonymous_sessions s WHERE token_hash=$1")
                .bind(token.as_slice()).fetch_one(&f.owner).await.unwrap();
            for (board, expected) in [(0, DUPLICATE), (1, FLOOD)] {
                let mut changed_session = session(&capability, false, request_at);
                changed_session.fingerprints = capability.fingerprints(Some("203.0.113.7".parse().unwrap()), *b"CA");
                assert_eq!(board_store::report_with_anonymous_session(&f.public, &f.boards[board], f.posts[board][0], "Changed peer cannot reset identity", &changed, changed_session).await.unwrap_err().to_string(), expected);
                assert_eq!(board_store::report_admission::check_with_session(&f.public, &f.boards[board], f.posts[board][0], &changed, Some(&token), request_at.timestamp()).await.unwrap_err().to_string(), expected);
            }
            let after: serde_json::Value = sqlx::query_scalar("SELECT to_jsonb(s) FROM post_secrets.anonymous_sessions s WHERE token_hash=$1")
                .bind(token.as_slice()).fetch_one(&f.owner).await.unwrap();
            assert_eq!(before, after, "Rejected writes and advisory reads must preserve activity and identity");
        }
        assert_eq!(f.counts().await, (1, 1));
    }).await;
}

#[tokio::test]
async fn registration_failure_rolls_back_report_membership_and_session() {
    run(|f| async move {
        let capability = Capability::generate().unwrap();
        let fingerprints = capability.fingerprints(Some("198.51.100.7".parse().unwrap()), *b"US");
        let identity = actor();
        // Token resolution succeeds. Invalid registration fingerprint fails only
        // after admission has attempted the report and private membership inserts.
        let error = sqlx::query(
            "SELECT content.admit_report($1,$2,'Registration rollback',$3,$4,$5,$6,$7,true,$8)",
        )
        .bind(&f.boards[0])
        .bind(f.posts[0][0])
        .bind(identity.as_bytes().as_slice())
        .bind(fingerprints.token.as_slice())
        .bind(Vec::<u8>::new())
        .bind(fingerprints.address.as_slice())
        .bind(fingerprints.environment.as_slice())
        .bind(Utc::now().timestamp())
        .execute(&f.public)
        .await
        .unwrap_err();
        assert_eq!(code(&error), "23514");
        assert_eq!(f.counts().await, (0, 0));
        assert!(
            board_store::anonymous_session::snapshot(&f.public, &capability.storage_hash())
                .await
                .unwrap()
                .is_none()
        );
        board_store::report_with_anonymous_session(
            &f.public,
            &f.boards[0],
            f.posts[0][0],
            "Retry after rollback",
            &identity,
            session(&capability, true, Utc::now()),
        )
        .await
        .unwrap();
        assert_eq!(f.counts().await, (1, 1));
    })
    .await;
}

#[tokio::test]
async fn session_lock_wait_rechecks_fresh_archive_expiry_without_activity() {
    run(|f| async move {
        let capability = Capability::generate().unwrap();
        let token = capability.storage_hash();
        // A different board establishes the capability without using the target.
        board_store::report_with_anonymous_session(&f.public, &f.boards[1], f.posts[1][0], "Establish session", &actor(), session(&capability, true, Utc::now())).await.unwrap();
        // Retire the first report's rate membership while keeping its session.
        sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1").bind(f.posts[1][0]).execute(&f.owner).await.unwrap();
        sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
            .bind(f.posts[0][0]).execute(&f.owner).await.unwrap();
        let before: serde_json::Value = sqlx::query_scalar("SELECT to_jsonb(s) FROM post_secrets.anonymous_sessions s WHERE token_hash=$1")
            .bind(token.as_slice()).fetch_one(&f.owner).await.unwrap();
        let mut lock = f.owner.begin().await.unwrap();
        let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *lock).await.unwrap();
        sqlx::query("SELECT token_hash FROM post_secrets.anonymous_sessions WHERE token_hash=$1 FOR UPDATE").bind(token.as_slice()).fetch_one(&mut *lock).await.unwrap();
        let public = sqlx::postgres::PgPoolOptions::new().max_connections(1).connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap()).await.unwrap();
        let waiter: i32 = sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&public).await.unwrap();
        let board = f.boards[0].clone();
        let post = f.posts[0][0];
        let admission = tokio::spawn(async move {
            let result = board_store::report_with_anonymous_session(&public, &board, post, "Archive expires while blocked", &actor(), session(&capability, false, Utc::now())).await;
            public.close().await;
            result
        });
        wait_for_row_blocker(&f.owner, waiter, blocker).await;
        // Set expiry after observing the actual session dependency. Admission's
        // pre-wait target was visible, but fresh post-wait time must reject it.
        sqlx::query("UPDATE content.threads SET archive_expires_at=clock_timestamp()-interval '1 microsecond' WHERE id=$1")
            .bind(post).execute(&f.owner).await.unwrap();
        lock.commit().await.unwrap();
        assert!(matches!(tokio::time::timeout(std::time::Duration::from_secs(5), admission).await.unwrap().unwrap(), Err(StoreError::NotFound)));
        let after: serde_json::Value = sqlx::query_scalar("SELECT to_jsonb(s) FROM post_secrets.anonymous_sessions s WHERE token_hash=$1")
            .bind(token.as_slice()).fetch_one(&f.owner).await.unwrap();
        assert_eq!(before, after);
        assert_eq!(f.counts().await, (0, 0));
    }).await;
}

// Capture rows, not merely counts: rollback and scope must preserve every
// retained report/evidence field, including moderation dispositions.
async fn report_snapshot(f: &Fixture) -> serde_json::Value {
    let mut tx = f.owner.begin().await.unwrap();
    let reports: serde_json::Value = sqlx::query_scalar("SELECT coalesce(jsonb_agg(to_jsonb(r) ORDER BY id),'[]') FROM content.reports r WHERE board=ANY($1)")
        .bind(f.boards.to_vec()).fetch_one(&mut *tx).await.unwrap();
    let anonymous: serde_json::Value = sqlx::query_scalar("SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY a.report_id),'[]') FROM post_secrets.anonymous_reports a JOIN content.reports r ON r.id=a.report_id WHERE r.board=ANY($1)")
        .bind(f.boards.to_vec()).fetch_one(&mut *tx).await.unwrap();
    private_role(&mut tx).await;
    let private: serde_json::Value = sqlx::query_scalar("SELECT jsonb_build_object('members',(SELECT coalesce(jsonb_agg(to_jsonb(m) ORDER BY report_id),'[]') FROM post_secrets.report_membership m WHERE board=ANY($1)),'groups',(SELECT coalesce(jsonb_agg(to_jsonb(g) ORDER BY board,post_id),'[]') FROM post_secrets.report_group g WHERE board=ANY($1)),'evidence',(SELECT coalesce(jsonb_agg(to_jsonb(e) ORDER BY report_id),'[]') FROM post_secrets.report_weight_evidence e JOIN content.reports r ON r.id=e.report_id WHERE r.board=ANY($1)))")
        .bind(f.boards.to_vec()).fetch_one(&mut *tx).await.unwrap();
    tx.rollback().await.unwrap();
    serde_json::json!({"reports": reports, "private": private, "anonymous": anonymous})
}

#[tokio::test]
async fn whole_deletion_cascades_all_states_and_unmembered_history_without_erasing_posts() {
    run(|f| async move {
        let mut removed = Vec::new();
        for target in 0..3 {
            for state in ["open", "resolved", "dismissed"] {
                let current = f.admit(0, target, &actor()).await.unwrap();
                sqlx::query("UPDATE content.reports SET state=$2 WHERE id=$1")
                    .bind(current).bind(state).execute(&f.owner).await.unwrap();
                removed.push(current);
                // Pre-admission history has neither membership nor a group.
                // Cleared history must be physically removed as well.
                let historical: i64 = sqlx::query_scalar("INSERT INTO content.reports(board,post_id,reason,state,reporter_cleared_at,group_cleared_at,group_cleared_by,group_clear_inherited) VALUES($1,$2,'Historical report without membership',$3,clock_timestamp(),clock_timestamp(),42,false) RETURNING id")
                    .bind(&f.boards[0]).bind(f.posts[0][target]).bind(state).fetch_one(&f.owner).await.unwrap();
                assert!(!f.member(historical).await);
                removed.push(historical);
            }
        }
        let unrelated = f.admit(1, 1, &actor()).await.unwrap();
        // A second thread on the same board must not be caught by thread scope.
        let post = NewPost {name:"Anonymous".into(),subject:"Unrelated thread".into(),comment:"Unrelated content".into(),deletion_hash:"fixture".into(),sage:false};
        let other_thread = support::create_post(&f.public,&f.boards[0],0,&post).await.unwrap();
        let mut connection=f.public.acquire().await.unwrap();
        let other_report=admit(&mut connection,&f.boards[0],other_thread,actor().as_bytes()).await.unwrap();
        drop(connection);
        let before = report_snapshot(&f).await;
        assert_eq!(before["private"]["evidence"].as_array().unwrap().len(),11,
            "Actual anonymous admissions must create evidence before cascade coverage");
        let tokens: Vec<Vec<u8>> = sqlx::query_scalar("SELECT token_hash FROM post_secrets.anonymous_reports WHERE report_id=ANY($1)")
            .bind(&removed).fetch_all(&f.owner).await.unwrap();
        let mut tx=f.public.begin().await.unwrap();
        sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE").bind(&f.boards[0]).execute(&mut *tx).await.unwrap();
        sqlx::query("UPDATE content.posts SET deleted=true WHERE board=$1 AND id=$2").bind(&f.boards[0]).bind(f.posts[0][1]).execute(&mut *tx).await.unwrap();
        tx.rollback().await.unwrap();
        assert_eq!(report_snapshot(&f).await,before,"Rollback restores reports and all cascaded private records");
        board_store::delete_post(&f.public,&f.boards[0],f.posts[0][1]).await.unwrap();
        let remaining: Vec<i64>=sqlx::query_scalar("SELECT id FROM content.reports WHERE board=$1 ORDER BY id")
            .bind(&f.boards[0]).fetch_all(&f.owner).await.unwrap();
        assert_eq!(remaining.len(),13);
        assert!(removed[6..12].iter().all(|id| !remaining.contains(id)),"Reply cleanup includes historical reports");
        board_store::delete_post(&f.public,&f.boards[0],f.posts[0][0]).await.unwrap();
        assert_eq!(f.counts().await,(2,2));
        assert!(f.member(unrelated).await && f.member(other_report).await);
        let mut tx=f.owner.begin().await.unwrap();
        let anonymous: i64=sqlx::query_scalar("SELECT count(*) FROM post_secrets.anonymous_reports WHERE report_id=ANY($1)").bind(&removed).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(anonymous,0);
        private_role(&mut tx).await;
        let evidence:i64=sqlx::query_scalar("SELECT count(*) FROM post_secrets.report_weight_evidence WHERE report_id=ANY($1)").bind(&removed).fetch_one(&mut *tx).await.unwrap();
        let groups:i64=sqlx::query_scalar("SELECT count(*) FROM post_secrets.report_group WHERE board=$1 AND post_id=ANY($2)").bind(&f.boards[0]).bind(f.posts[0].to_vec()).fetch_one(&mut *tx).await.unwrap();
        assert_eq!((evidence,groups),(0,0));
        tx.rollback().await.unwrap();
        let comments:Vec<String>=sqlx::query_scalar("SELECT comment FROM content.posts WHERE board=$1 AND thread_id=$2 ORDER BY id")
            .bind(&f.boards[0]).bind(f.posts[0][0]).fetch_all(&f.owner).await.unwrap();
        assert_eq!(comments,vec!["Owned report admission target";4],"Deletion retains post content");
        for token in tokens {
            sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1").bind(token).execute(&f.owner).await.unwrap();
        }
    }).await;
}

#[tokio::test]
async fn whole_deletion_serializes_with_admission_in_both_board_lock_orders() {
    run(|f| async move {
        let public = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let waiter: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&public)
            .await
            .unwrap();
        // Admission commits first: the waiting deletion sees and removes it.
        let mut first = f.public.begin().await.unwrap();
        let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *first)
            .await
            .unwrap();
        let admitted = admit(&mut first, &f.boards[0], f.posts[0][1], actor().as_bytes())
            .await
            .unwrap();
        let pending = tokio::spawn({
            let public = public.clone();
            let board = f.boards[0].clone();
            let post = f.posts[0][1];
            async move { board_store::delete_post(&public, &board, post).await }
        });
        wait_for_row_blocker(&f.owner, waiter, blocker).await;
        first.commit().await.unwrap();
        pending.await.unwrap().unwrap();
        assert_eq!(f.counts().await, (0, 0));
        assert!(!f.member(admitted).await);
        // Deletion owns the board first: the waiting admission rechecks target
        // visibility after commit rather than recreating a deleted report.
        let mut deletion = f.public.begin().await.unwrap();
        let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *deletion)
            .await
            .unwrap();
        sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
            .bind(&f.boards[0])
            .execute(&mut *deletion)
            .await
            .unwrap();
        sqlx::query("UPDATE content.posts SET deleted=true WHERE board=$1 AND id=$2")
            .bind(&f.boards[0])
            .bind(f.posts[0][2])
            .execute(&mut *deletion)
            .await
            .unwrap();
        let pending = tokio::spawn({
            let public = public.clone();
            let board = f.boards[0].clone();
            let post = f.posts[0][2];
            async move { report(&public, &board, post, "Blocked report", &actor()).await }
        });
        wait_for_row_blocker(&f.owner, waiter, blocker).await;
        deletion.commit().await.unwrap();
        assert!(matches!(pending.await.unwrap(), Err(StoreError::NotFound)));
        assert_eq!(f.counts().await, (0, 0));
        public.close().await;
    })
    .await;
}

#[tokio::test]
async fn whole_report_cleanup_does_not_grant_runtime_report_delete_authority() {
    run(|f| async move {
        let staff = PgPool::connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
            .await
            .unwrap();
        for (pool, role) in [(&f.public, "board_public"), (&staff, "board_staff")] {
            let actual: String = sqlx::query_scalar("SELECT current_user::text")
                .fetch_one(pool)
                .await
                .unwrap();
            assert_eq!(actual, role);
            for query in [
                "DELETE FROM content.reports WHERE false",
                "TRUNCATE content.reports",
                "SELECT post_secrets.delete_reports_for_deleted_target()",
                "SET ROLE board_report_admission_owner",
            ] {
                assert_eq!(
                    code(&sqlx::query(query).execute(pool).await.unwrap_err()),
                    "42501",
                    "{role}: {query}"
                );
            }
        }
        staff.close().await;
    })
    .await;
}
