#![cfg(feature = "database-tests")]

use board_domain::anonymous_session::{Activity, Capability, Changes, State};
use sqlx::{PgConnection, PgPool};

const CALL: &str = "SELECT post_secrets.report_known_or_verified($1,$2,$3,$4,$5,$6)";
const NETWORK: [u8; 32] = [21; 32];
const ADDRESS: [u8; 32] = [22; 32];
const ENVIRONMENT: [u8; 32] = [23; 32];

async fn login(variable: &str, expected: &str) -> PgPool {
    let pool = PgPool::connect(&std::env::var(variable).expect("owned database URL required"))
        .await
        .unwrap();
    let actual: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(actual, expected, "use the actual configured login");
    pool
}

async fn now(c: &mut PgConnection) -> u64 {
    let value: i64 = sqlx::query_scalar("SELECT extract(epoch FROM clock_timestamp())::bigint")
        .fetch_one(c)
        .await
        .unwrap();
    value.try_into().unwrap()
}

async fn seed(c: &mut PgConnection, token: &[u8; 32], s: State, expiry: u64) {
    sqlx::query("INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,created_at,network_at,address_at,environment_at,activity_at,action_at,expires_at,verified_level,posts,images,threads,reports,pending,change_score) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18)")
        .bind(token.as_slice()).bind(NETWORK.as_slice()).bind(ADDRESS.as_slice()).bind(ENVIRONMENT.as_slice())
        .bind(s.created_at as i64).bind(s.network_at as i64).bind(s.address_at as i64)
        .bind(s.environment_at as i64).bind(s.activity_at as i64).bind(s.action_at as i64)
        .bind(expiry as i64).bind(i16::from(s.verified_level)).bind(i16::from(s.posts))
        .bind(i16::from(s.images)).bind(i16::from(s.threads)).bind(i16::from(s.reports))
        .bind(i16::from(s.pending)).bind(i16::from(s.change_score))
        .execute(c).await.unwrap();
}

// Includes every session column (hashes, expiry, timestamps and activity), plus
// both membership counts. All fixtures are transaction-local and rolled back.
async fn snapshot(c: &mut PgConnection, token: &[u8; 32]) -> (Option<String>, i64, i64) {
    sqlx::query_as("SELECT (SELECT to_jsonb(s)::text FROM post_secrets.anonymous_sessions s WHERE token_hash=$1),(SELECT count(*) FROM post_secrets.anonymous_posts WHERE token_hash=$1),(SELECT count(*) FROM post_secrets.anonymous_reports WHERE token_hash=$1)")
        .bind(token.as_slice()).fetch_one(c).await.unwrap()
}

async fn known(
    c: &mut PgConnection,
    token: &[u8; 32],
    minted: bool,
    at: u64,
    changes: Changes,
) -> bool {
    sqlx::query("SET LOCAL ROLE board_report_admission_owner")
        .execute(&mut *c)
        .await
        .unwrap();
    let result = sqlx::query_scalar(CALL)
        .bind(token.as_slice())
        .bind(if changes.network { [31; 32] } else { NETWORK }.to_vec())
        .bind(if changes.address { [32; 32] } else { ADDRESS }.to_vec())
        .bind(
            if changes.environment {
                [33; 32]
            } else {
                ENVIRONMENT
            }
            .to_vec(),
        )
        .bind(minted)
        .bind(at as i64)
        .fetch_one(&mut *c)
        .await
        .unwrap();
    sqlx::query("RESET ROLE").execute(c).await.unwrap();
    result
}

#[tokio::test]
async fn pure_helper_matches_resumed_rust_state_at_age_count_idle_and_change_boundaries() {
    let owner = login("MIGRATION_DATABASE_URL", "board_migrator").await;
    let mut tx = owner.begin().await.unwrap();
    let at = now(&mut tx).await;
    let mut base = State::new(at - 7_200);
    base.network_at = at - 1_800;
    base.activity_at = at - 1;
    base.action_at = at - 100;
    let mut cases = Vec::new();
    let unchanged = Changes::default();
    for age in [0, 1_799, 1_800, 3_599, 3_600] {
        for (posts, reports, pending) in [
            (0, 0, 0),
            (2, 0, 0),
            (2, 0, 1),
            (2, 0, 2),
            (2, 0, 4),
            (3, 0, 0),
            (8, 0, 1),
            (9, 0, 0),
            (0, 9, 0),
            (0, 9, 1),
            (0, 9, 2),
            (0, 9, 8),
            (0, 10, 0),
            (0, 19, 8),
            (0, 20, 0),
        ] {
            let mut state = base;
            state.network_at = at - age;
            state.posts = posts;
            state.reports = reports;
            state.pending = pending;
            cases.push((
                format!("network age {age}, posts {posts}, reports {reports}, pending {pending}"),
                state,
                unchanged,
            ));
        }
    }
    for age in [3_599, 3_600] {
        let mut state = base;
        state.created_at = at - age;
        state.posts = 3;
        cases.push((format!("password age {age}"), state, unchanged));
    }
    for score in [9, 10, 31, 32] {
        for age in [1_799, 1_800] {
            let mut state = base;
            state.posts = 9;
            state.change_score = score;
            state.network_at = at - age;
            cases.push((
                format!("churn {score}, network age {age}"),
                state,
                unchanged,
            ));
        }
    }
    for changes in [
        Changes {
            network: true,
            ..unchanged
        },
        Changes {
            address: true,
            ..unchanged
        },
        Changes {
            environment: true,
            ..unchanged
        },
        Changes {
            network: true,
            address: true,
            environment: true,
        },
    ] {
        let mut state = base;
        state.network_at = at - 3_600;
        cases.push((format!("resumed changes {changes:?}"), state, changes));
        state.verified_level = 1;
        state.change_score = 32;
        cases.push((format!("verified with changes {changes:?}"), state, changes));
    }
    for idle in [604_799, 604_800] {
        for activity_present in [false, true] {
            let mut state = State::new(at - idle);
            state.activity_at = if activity_present { at - idle } else { 0 };
            state.verified_level = 255;
            state.posts = 255;
            state.reports = 255;
            state.pending = 15;
            cases.push((
                format!("idle {idle}, activity present {activity_present}"),
                state,
                unchanged,
            ));
        }
    }
    let mut future = State::new(at + 1);
    future.activity_at = at + 1;
    cases.push(("future timestamps saturate".into(), future, unchanged));
    assert_eq!(
        cases.len(),
        98,
        "keep the differential boundary matrix complete"
    );
    for (label, state, changes) in cases {
        let token = Capability::generate().unwrap().storage_hash();
        seed(&mut tx, &token, state, at + 31_536_000).await;
        let before = snapshot(&mut tx, &token).await;
        let mut resumed = state;
        resumed.resume(at, changes);
        assert_eq!(
            known(&mut tx, &token, false, at, changes).await,
            resumed.is_known_or_verified(at, 60, 0),
            "{label}"
        );
        assert_eq!(
            snapshot(&mut tx, &token).await,
            before,
            "read-only helper: {label}"
        );
    }
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn decision_precedes_the_report_activity_that_can_cross_the_known_threshold() {
    let owner = login("MIGRATION_DATABASE_URL", "board_migrator").await;
    let mut tx = owner.begin().await.unwrap();
    let at = now(&mut tx).await;
    let token = Capability::generate().unwrap().storage_hash();
    let mut state = State::new(at - 7_200);
    state.network_at = at - 1_800;
    state.activity_at = at - 1;
    state.action_at = at - 100;
    state.reports = 9;
    seed(&mut tx, &token, state, at + 31_536_000).await;
    let before = snapshot(&mut tx, &token).await;
    assert!(!state.is_known_or_verified(at, 60, 0));
    assert!(!known(&mut tx, &token, false, at, Changes::default()).await);
    assert_eq!(snapshot(&mut tx, &token).await, before);
    // Invoke the real activity writer used by report registration, as its
    // private owner. The decision above must not anticipate this pending bit.
    sqlx::query("SET LOCAL ROLE board_anonymous_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT post_secrets.advance_anonymous_session($1,$2,$3,$4,false,8::smallint,$5)")
        .bind(token.as_slice())
        .bind(NETWORK.as_slice())
        .bind(ADDRESS.as_slice())
        .bind(ENVIRONMENT.as_slice())
        .bind(at as i64)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("RESET ROLE").execute(&mut *tx).await.unwrap();
    state.update(at, Activity::Report, false);
    assert!(state.is_known_or_verified(at, 60, 0));
    let after_activity = snapshot(&mut tx, &token).await;
    assert_ne!(after_activity, before);
    assert!(known(&mut tx, &token, false, at, Changes::default()).await);
    assert_eq!(snapshot(&mut tx, &token).await, after_activity);
    tx.rollback().await.unwrap();
}

fn code(error: &sqlx::Error) -> String {
    error
        .as_database_error()
        .unwrap()
        .code()
        .unwrap()
        .into_owned()
}

#[tokio::test]
async fn minted_unknown_is_unknown_without_allocation_and_changed_authorization_is_rejected() {
    let owner = login("MIGRATION_DATABASE_URL", "board_migrator").await;
    let mut tx = owner.begin().await.unwrap();
    let at = now(&mut tx).await;
    let absent = Capability::generate().unwrap().storage_hash();
    let existing = Capability::generate().unwrap().storage_hash();
    let expired = Capability::generate().unwrap().storage_hash();
    seed(&mut tx, &existing, State::new(at - 7_200), at + 3_600).await;
    seed(&mut tx, &expired, State::new(at - 7_200), at).await;
    let before = snapshot(&mut tx, &absent).await;
    assert_eq!(before, (None, 0, 0));
    assert!(!known(&mut tx, &absent, true, at, Changes::default()).await);
    assert_eq!(snapshot(&mut tx, &absent).await, before);
    for (label, token, minted, request_at, expected) in [
        ("missing resumed token", absent, false, at, "28000"),
        ("minted collision", existing, true, at, "28000"),
        ("expired token", expired, false, at, "28000"),
        ("stale request", existing, false, at - 120, "23514"),
        ("future request", existing, false, at + 120, "23514"),
    ] {
        let before = snapshot(&mut tx, &token).await;
        sqlx::query("SAVEPOINT rejection")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SET LOCAL ROLE board_report_admission_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        let error = sqlx::query_scalar::<_, bool>(CALL)
            .bind(token.as_slice())
            .bind(NETWORK.as_slice())
            .bind(ADDRESS.as_slice())
            .bind(ENVIRONMENT.as_slice())
            .bind(minted)
            .bind(request_at as i64)
            .fetch_one(&mut *tx)
            .await
            .unwrap_err();
        assert_eq!(code(&error), expected, "{label}");
        sqlx::query("ROLLBACK TO SAVEPOINT rejection")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert_eq!(snapshot(&mut tx, &token).await, before, "{label}");
    }
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn helper_is_owned_by_anonymous_owner_and_denied_to_actual_runtime_logins() {
    let owner = login("MIGRATION_DATABASE_URL", "board_migrator").await;
    let metadata: (String, bool, bool, bool) = sqlx::query_as("SELECT pg_get_userbyid(p.proowner)::text,p.prosecdef,has_function_privilege('board_report_admission_owner',p.oid,'EXECUTE'),EXISTS(SELECT 1 FROM aclexplode(coalesce(p.proacl,acldefault('f',p.proowner))) a WHERE a.grantee=0 AND a.privilege_type='EXECUTE') FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='post_secrets' AND p.proname='report_known_or_verified'")
        .fetch_one(&owner).await.unwrap();
    assert_eq!(
        metadata,
        ("board_anonymous_owner".into(), true, true, false)
    );
    for (variable, role) in [
        ("TEST_PUBLIC_DATABASE_URL", "board_public"),
        ("STAFF_DATABASE_URL", "board_staff"),
        ("AUTH_DATABASE_URL", "board_auth"),
    ] {
        let runtime = login(variable, role).await;
        let allowed: bool = sqlx::query_scalar("SELECT has_function_privilege(current_user,p.oid,'EXECUTE') FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='post_secrets' AND p.proname='report_known_or_verified'")
            .fetch_one(&runtime).await.unwrap();
        assert!(!allowed, "{role} must not execute the private helper");
        let error = sqlx::query_scalar::<_, bool>(CALL)
            .bind(NETWORK.as_slice())
            .bind(NETWORK.as_slice())
            .bind(ADDRESS.as_slice())
            .bind(ENVIRONMENT.as_slice())
            .bind(true)
            .bind(1_i64)
            .fetch_one(&runtime)
            .await
            .unwrap_err();
        assert_eq!(code(&error), "42501", "{role}");
    }
}

#[tokio::test]
async fn invalid_context_and_non_read_committed_observation_fail_without_mutation() {
    let owner = login("MIGRATION_DATABASE_URL", "board_migrator").await;
    let mut tx = owner.begin().await.unwrap();
    let at = now(&mut tx).await;
    let token = Capability::generate().unwrap().storage_hash();
    seed(&mut tx, &token, State::new(at - 7_200), at + 3_600).await;
    let before = snapshot(&mut tx, &token).await;
    for field in 0..6 {
        let mut hashes = [
            Some(token.to_vec()),
            Some(NETWORK.to_vec()),
            Some(ADDRESS.to_vec()),
            Some(ENVIRONMENT.to_vec()),
        ];
        if field < 4 {
            hashes[field] = None;
        }
        sqlx::query("SAVEPOINT invalid_context")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SET LOCAL ROLE board_report_admission_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        let error = sqlx::query_scalar::<_, bool>(CALL)
            .bind(hashes[0].as_deref())
            .bind(hashes[1].as_deref())
            .bind(hashes[2].as_deref())
            .bind(hashes[3].as_deref())
            .bind(if field == 4 { None } else { Some(false) })
            .bind(if field == 5 { None } else { Some(at as i64) })
            .fetch_one(&mut *tx)
            .await
            .unwrap_err();
        assert_eq!(code(&error), "23514", "NULL field {field}");
        sqlx::query("ROLLBACK TO SAVEPOINT invalid_context")
            .execute(&mut *tx)
            .await
            .unwrap();
        assert_eq!(snapshot(&mut tx, &token).await, before);
    }
    for field in 0..4 {
        for length in [0, 31, 33] {
            let mut hashes = [
                token.to_vec(),
                NETWORK.to_vec(),
                ADDRESS.to_vec(),
                ENVIRONMENT.to_vec(),
            ];
            hashes[field] = vec![24; length];
            sqlx::query("SAVEPOINT invalid_length")
                .execute(&mut *tx)
                .await
                .unwrap();
            sqlx::query("SET LOCAL ROLE board_report_admission_owner")
                .execute(&mut *tx)
                .await
                .unwrap();
            let error = sqlx::query_scalar::<_, bool>(CALL)
                .bind(&hashes[0])
                .bind(&hashes[1])
                .bind(&hashes[2])
                .bind(&hashes[3])
                .bind(false)
                .bind(at as i64)
                .fetch_one(&mut *tx)
                .await
                .unwrap_err();
            assert_eq!(code(&error), "23514", "field {field}, length {length}");
            sqlx::query("ROLLBACK TO SAVEPOINT invalid_length")
                .execute(&mut *tx)
                .await
                .unwrap();
            assert_eq!(snapshot(&mut tx, &token).await, before);
        }
    }
    tx.rollback().await.unwrap();
    for isolation in [
        "SET TRANSACTION ISOLATION LEVEL REPEATABLE READ",
        "SET TRANSACTION ISOLATION LEVEL SERIALIZABLE",
    ] {
        let mut tx = owner.begin().await.unwrap();
        sqlx::query(isolation).execute(&mut *tx).await.unwrap();
        sqlx::query("SET LOCAL ROLE board_report_admission_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        let error = sqlx::query_scalar::<_, bool>(CALL)
            .bind(token.as_slice())
            .bind(NETWORK.as_slice())
            .bind(ADDRESS.as_slice())
            .bind(ENVIRONMENT.as_slice())
            .bind(true)
            .bind(at as i64)
            .fetch_one(&mut *tx)
            .await
            .unwrap_err();
        assert_eq!(code(&error), "22023", "{isolation}");
        tx.rollback().await.unwrap();
    }
}

#[tokio::test]
async fn session_lock_wait_observes_committed_activity_expiry_and_revocation() {
    assert!(
        std::env::var("BOARD_TEST_CLUSTER").is_ok_and(|path| path
            .strip_prefix("/tmp/board-postgres.")
            .is_some_and(|tag| tag.len() == 8 && tag.bytes().all(|b| b.is_ascii_alphanumeric()))),
        "Committed session races require the explicit fresh disposable cluster marker"
    );
    let owner = login("MIGRATION_DATABASE_URL", "board_migrator").await;
    let board: String =
        sqlx::query_scalar("SELECT 'rk'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    let tokens = [
        Capability::generate().unwrap().storage_hash(),
        Capability::generate().unwrap().storage_hash(),
        Capability::generate().unwrap().storage_hash(),
    ];
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Owned known-report race','Synthetic',2000,100,100,100,10)")
        .bind(&board).execute(&owner).await.unwrap();
    let test_owner = owner.clone();
    let test_board = board.clone();
    // Preserve exact cleanup ownership even if an assertion in the race fails.
    let result = tokio::spawn(async move {
        for (change, token) in ["pending threshold", "expiry", "revocation"].into_iter().zip(tokens) {
            let mut setup = test_owner.begin().await.unwrap();
            let at = now(&mut setup).await;
            let mut state = State::new(at - 7_200);
            state.network_at = at - 1_800;
            state.activity_at = at - 1;
            state.action_at = at - 100;
            state.reports = 9;
            assert!(!state.is_known_or_verified(at, 60, 0));
            seed(&mut setup, &token, state, at + 3_600).await;
            setup.commit().await.unwrap();

            // The independent writer owns only the session row: it never
            // needs the board or gate subsequently held by the waiting reader.
            let mut blocker = test_owner.begin().await.unwrap();
            let blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut *blocker).await.unwrap();
            match change {
                "pending threshold" => {
                    sqlx::query("UPDATE post_secrets.anonymous_sessions SET pending=8 WHERE token_hash=$1")
                        .bind(token.as_slice()).execute(&mut *blocker).await.unwrap();
                    state.pending = 8;
                    assert!(state.is_known_or_verified(at, 60, 0));
                }
                "expiry" => {
                    sqlx::query("UPDATE post_secrets.anonymous_sessions SET expires_at=$2 WHERE token_hash=$1")
                        .bind(token.as_slice()).bind(at as i64).execute(&mut *blocker).await.unwrap();
                }
                "revocation" => {
                    sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
                        .bind(token.as_slice()).execute(&mut *blocker).await.unwrap();
                }
                _ => unreachable!(),
            }
            let writer_state = snapshot(&mut blocker, &token).await;
            let mut waiting = test_owner.begin().await.unwrap();
            sqlx::query("SET LOCAL lock_timeout='10s'").execute(&mut *waiting).await.unwrap();
            sqlx::query("SET LOCAL statement_timeout='15s'").execute(&mut *waiting).await.unwrap();
            let waiter_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut *waiting).await.unwrap();
            sqlx::query("SET LOCAL ROLE board_report_admission_owner")
                .execute(&mut *waiting).await.unwrap();
            // Honor the helper's private caller lock-order prerequisite.
            sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
                .bind(&test_board).fetch_one(&mut *waiting).await.unwrap();
            sqlx::query("SELECT singleton FROM post_secrets.report_admission_gate WHERE singleton FOR UPDATE")
                .fetch_one(&mut *waiting).await.unwrap();
            let observation = tokio::spawn(async move {
                let result = sqlx::query_scalar::<_, bool>(CALL)
                    .bind(token.as_slice()).bind(NETWORK.as_slice()).bind(ADDRESS.as_slice())
                    .bind(ENVIRONMENT.as_slice()).bind(false).bind(at as i64)
                    .fetch_one(&mut *waiting).await;
                if result.is_ok() { waiting.commit().await.unwrap(); }
                else { waiting.rollback().await.unwrap(); }
                result
            });
            let blocked = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    let seen: bool = sqlx::query_scalar("SELECT $2=ANY(pg_blocking_pids($1)) AND EXISTS(SELECT 1 FROM pg_locks WHERE pid=$1 AND NOT granted AND locktype='transactionid')")
                        .bind(waiter_pid).bind(blocker_pid).fetch_one(&test_owner).await.unwrap();
                    if seen { break; }
                    tokio::task::yield_now().await;
                }
            }).await;
            // A timeout still releases the writer and drains the bounded query
            // before failing; elapsed time is never treated as lock evidence.
            if blocked.is_ok() { blocker.commit().await.unwrap(); }
            else { blocker.rollback().await.unwrap(); }
            let observed = tokio::time::timeout(std::time::Duration::from_secs(16), observation)
                .await.expect("bounded session observation did not finish").unwrap();
            blocked.expect("actual session row-lock dependency was not observed");
            if change == "pending threshold" {
                assert_eq!(observed.unwrap(), state.is_known_or_verified(at, 60, 0));
            } else {
                assert_eq!(code(&observed.unwrap_err()), "28000", "{change}");
            }
            let mut inspection = test_owner.acquire().await.unwrap();
            assert_eq!(snapshot(&mut inspection, &token).await, writer_state,
                "helper must not mutate the writer's committed state: {change}");
        }
    }).await;
    for token in tokens {
        sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
            .bind(token.as_slice())
            .execute(&owner)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM content.boards WHERE slug=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    result.unwrap();
}
