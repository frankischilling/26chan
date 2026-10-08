#![cfg(feature = "database-tests")]

use board_domain::anonymous_session::Capability;
use chrono::Utc;
use sqlx::{PgConnection, PgPool};

// Catalog activation is singleton state. Keep independent fixtures serialized;
// each case restores activation and cleans only its own content and sessions.
// Immutable catalog revisions remain in the disposable cluster until teardown.
static TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn owner() -> PgPool {
    PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap()
}
async fn private(c: &mut PgConnection) {
    sqlx::query("SET ROLE board_report_admission_owner")
        .execute(c)
        .await
        .unwrap();
}
async fn reset(c: &mut PgConnection) {
    sqlx::query("RESET ROLE").execute(c).await.unwrap();
}
fn code(error: &sqlx::Error) -> String {
    error
        .as_database_error()
        .unwrap()
        .code()
        .unwrap()
        .into_owned()
}
async fn fixture(c: &mut PgConnection) -> (String, i64, i64) {
    let post: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&mut *c)
        .await
        .unwrap();
    let board = format!("gc{post:x}");
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Owned group clear','Synthetic',2000,100,100,100,10)").bind(&board).execute(&mut *c).await.unwrap();
    sqlx::query("INSERT INTO content.threads(id,board) VALUES($1,$2)")
        .bind(post)
        .bind(&board)
        .execute(&mut *c)
        .await
        .unwrap();
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','','Synthetic group clear OP')").bind(post).bind(&board).execute(&mut *c).await.unwrap();
    let revision=sqlx::query_scalar("SELECT content.import_report_catalog($1::text::jsonb)")
        .bind(serde_json::json!({"version":1,"categories":[{"id":31,"board":"","op_only":false,"reply_only":false,"image_only":false,"exclude_boards":null,"title":"Owned category","weight":0.5,"filtered":0}]}).to_string()).fetch_one(&mut *c).await.unwrap();
    activate(c, Some(revision)).await;
    (board, post, revision)
}
async fn activate(c: &mut PgConnection, revision: Option<i64>) {
    sqlx::query("SELECT content.set_report_catalog_active($1)")
        .bind(revision)
        .execute(c)
        .await
        .unwrap();
}
async fn admit(
    _c: &mut PgConnection,
    board: &str,
    post: i64,
    revision: Option<i64>,
    legacy: bool,
) -> i64 {
    let cap = Capability::generate().unwrap();
    let actor = Capability::generate().unwrap().storage_hash();
    let (variable, expected) = if legacy {
        ("STAFF_DATABASE_URL", "board_staff")
    } else {
        ("TEST_PUBLIC_DATABASE_URL", "board_public")
    };
    let pool = PgPool::connect(&std::env::var(variable).unwrap())
        .await
        .unwrap();
    let actual: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(actual, expected);
    let f = cap.fingerprints(Some("198.51.100.77".parse().unwrap()), *b"US");
    let result = if legacy {
        sqlx::query_scalar("SELECT content.admit_report($1,$2,'Owned legacy reason',$3)")
            .bind(board)
            .bind(post)
            .bind(actor.as_slice())
            .fetch_one(&pool)
            .await
    } else if let Some(revision) = revision {
        sqlx::query_scalar(
            "SELECT content.admit_categorical_report($1,$2,31,$3,$4,$5,$6,$7,$8,true,$9)",
        )
        .bind(board)
        .bind(post)
        .bind(revision)
        .bind(actor.as_slice())
        .bind(f.token.as_slice())
        .bind(f.network.as_slice())
        .bind(f.address.as_slice())
        .bind(f.environment.as_slice())
        .bind(Utc::now().timestamp())
        .fetch_one(&pool)
        .await
    } else {
        sqlx::query_scalar(
            "SELECT content.admit_report($1,$2,'Owned session reason',$3,$4,$5,$6,$7,true,$8)",
        )
        .bind(board)
        .bind(post)
        .bind(actor.as_slice())
        .bind(f.token.as_slice())
        .bind(f.network.as_slice())
        .bind(f.address.as_slice())
        .bind(f.environment.as_slice())
        .bind(Utc::now().timestamp())
        .fetch_one(&pool)
        .await
    };
    pool.close().await;
    result.unwrap()
}
async fn clear(_c: &mut PgConnection, board: &str, post: i64) -> Result<Option<i64>, sqlx::Error> {
    let pool = PgPool::connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let result = sqlx::query_scalar("SELECT content.clear_report_group($1,$2,77)")
        .bind(board)
        .bind(post)
        .fetch_one(&pool)
        .await;
    pool.close().await;
    result
}
async fn purge(board: &str, report: i64) -> Option<i64> {
    let pool = PgPool::connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let result = sqlx::query_scalar("SELECT content.clear_reporter($1,$2)")
        .bind(board)
        .bind(report)
        .fetch_one(&pool)
        .await
        .unwrap();
    pool.close().await;
    result
}
async fn run_case<F, Fut>(body: F)
where
    F: FnOnce(PgPool, String, i64, i64) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = ()> + Send + 'static,
{
    assert!(std::env::var("BOARD_TEST_CLUSTER").is_ok_and(|path| {
        path.strip_prefix("/tmp/board-postgres.")
            .is_some_and(|tag| tag.len() == 8 && tag.bytes().all(|b| b.is_ascii_alphanumeric()))
    }));
    let pool = owner().await;
    let mut c = pool.acquire().await.unwrap();
    private(&mut c).await;
    let previous: Option<i64> = sqlx::query_scalar(
        "SELECT active_catalog_revision FROM post_secrets.report_admission_gate WHERE singleton",
    )
    .fetch_one(&mut *c)
    .await
    .unwrap();
    reset(&mut c).await;
    let (board, post, revision) = fixture(&mut c).await;
    drop(c);
    let work = pool.clone();
    let target = board.clone();
    let result = tokio::spawn(async move { body(work, target, post, revision).await }).await;
    let mut tx = pool.begin().await.unwrap();
    reset(&mut tx).await;
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(&board)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM content.threads WHERE board=$1 ORDER BY id FOR UPDATE")
        .bind(&board)
        .execute(&mut *tx)
        .await
        .unwrap();
    activate(&mut tx, previous).await;
    let tokens:Vec<Vec<u8>>=sqlx::query_scalar("SELECT a.token_hash FROM post_secrets.anonymous_reports a JOIN content.reports r ON r.id=a.report_id WHERE r.board=$1").bind(&board).fetch_all(&mut *tx).await.unwrap();
    // Match expired-session collection's session -> membership lock order.
    sqlx::query("SELECT token_hash FROM post_secrets.anonymous_sessions WHERE token_hash=ANY($1::bytea[]) ORDER BY token_hash FOR UPDATE")
        .bind(&tokens)
        .fetch_all(&mut *tx)
        .await
        .unwrap();
    for statement in [
        "DELETE FROM content.moderation_audit WHERE board=$1",
        "DELETE FROM content.reports WHERE board=$1",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(statement)
            .bind(&board)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    for token in tokens {
        sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
            .bind(token)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();
    pool.close().await;
    result.unwrap();
}
async fn snapshot(c: &mut PgConnection, board: &str) -> String {
    let reports:String=sqlx::query_scalar("SELECT coalesce(jsonb_agg(to_jsonb(r)-'group_cleared_at'-'group_cleared_by'-'group_clear_inherited' ORDER BY id),'[]')::text FROM content.reports r WHERE board=$1").bind(board).fetch_one(&mut *c).await.unwrap();
    private(c).await;
    let private:String=sqlx::query_scalar("SELECT jsonb_build_object('members',(SELECT coalesce(jsonb_agg(to_jsonb(m) ORDER BY report_id),'[]') FROM post_secrets.report_membership m WHERE board=$1),'counts',(SELECT jsonb_build_array(illegal_count,incomplete) FROM post_secrets.report_group WHERE board=$1),'evidence',(SELECT coalesce(jsonb_agg(to_jsonb(e) ORDER BY e.report_id),'[]') FROM post_secrets.report_weight_evidence e JOIN post_secrets.report_membership m ON m.report_id=e.report_id WHERE m.board=$1))::text").bind(board).fetch_one(&mut *c).await.unwrap();
    reset(c).await;
    let sessions:String=sqlx::query_scalar("SELECT coalesce(jsonb_agg(to_jsonb(s) ORDER BY s.token_hash),'[]')::text FROM post_secrets.anonymous_sessions s WHERE token_hash IN(SELECT token_hash FROM post_secrets.anonymous_reports a JOIN content.reports r ON r.id=a.report_id WHERE r.board=$1)").bind(board).fetch_one(&mut *c).await.unwrap();
    format!("{reports}\n{private}\n{sessions}")
}
async fn cleared(
    c: &mut PgConnection,
    board: &str,
    post: i64,
) -> Option<(chrono::DateTime<Utc>, i64)> {
    private(c).await;
    let state=sqlx::query_as("SELECT cleared_at,cleared_by FROM post_secrets.report_group WHERE board=$1 AND post_id=$2 AND cleared_at IS NOT NULL").bind(board).bind(post).fetch_optional(&mut *c).await.unwrap();
    reset(c).await;
    state
}

#[tokio::test]
async fn all_admission_overloads_inherit_origin_without_changing_quota_or_report_disposition() {
    let _serial = TEST.lock().await;
    run_case(|pool,board,post,revision| async move {
    let mut tx=pool.acquire().await.unwrap();
    let a=admit(&mut tx,&board,post,Some(revision),false).await;
    let b=admit(&mut tx,&board,post,Some(revision),false).await;
    sqlx::query("UPDATE content.reports SET state='resolved' WHERE id=$1").bind(b).execute(&mut *tx).await.unwrap();
    let before=snapshot(&mut tx,&board).await;
    assert_eq!(clear(&mut tx,&board,post).await.unwrap(),Some(2));
    assert_eq!(snapshot(&mut tx,&board).await,before);
    let origin=cleared(&mut tx,&board,post).await.unwrap();
    let c=admit(&mut tx,&board,post,Some(revision),false).await;
    activate(&mut tx,None).await;
    let d=admit(&mut tx,&board,post,None,false).await;
    let e=admit(&mut tx,&board,post,None,true).await;
    for id in [a,b,c,d,e] {
        let row:(chrono::DateTime<Utc>,i64,bool,String)=sqlx::query_as("SELECT group_cleared_at,group_cleared_by,group_clear_inherited,state FROM content.reports WHERE id=$1").bind(id).fetch_one(&mut *tx).await.unwrap();
        assert_eq!((row.0,row.1),origin);
        assert_eq!(row.2,[c,d,e].contains(&id));
        assert_eq!(row.3,if id==b {"resolved"} else {"open"});
    }
    assert_eq!(clear(&mut tx,&board,post).await.unwrap(),Some(0),"unknown inherited reports do not reopen a cleared lifetime");
    private(&mut tx).await;
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM post_secrets.report_membership WHERE board=$1").bind(&board).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(count,5);
    reset(&mut tx).await;
    assert_eq!(clear(&mut tx,&board,post+1).await.unwrap(),None);
    }).await;
}

#[tokio::test]
async fn partial_reporter_purge_preserves_clear_last_member_resets_and_retired_history_is_excluded()
{
    let _serial = TEST.lock().await;
    run_case(|pool,board,post,revision| async move {
    let mut tx=pool.acquire().await.unwrap();
    let a=admit(&mut tx,&board,post,Some(revision),false).await;
    let b=admit(&mut tx,&board,post,Some(revision),false).await;
    assert_eq!(clear(&mut tx,&board,post).await.unwrap(),Some(2));
    let origin=cleared(&mut tx,&board,post).await.unwrap();
    let removed=purge(&board,a).await;
    assert_eq!(removed,Some(1));
    reset(&mut tx).await;
    assert_eq!(cleared(&mut tx,&board,post).await,Some(origin));
    let removed=purge(&board,b).await;
    assert_eq!(removed,Some(1));
    reset(&mut tx).await;
    assert_eq!(clear(&mut tx,&board,post).await.unwrap(),None);
    let fresh=admit(&mut tx,&board,post,Some(revision),false).await;
    let unmarked:bool=sqlx::query_scalar("SELECT group_cleared_at IS NULL AND group_cleared_by IS NULL AND group_clear_inherited IS NULL FROM content.reports WHERE id=$1").bind(fresh).fetch_one(&mut *tx).await.unwrap();
    assert!(unmarked);
    assert_eq!(cleared(&mut tx,&board,post).await,None);
    assert_eq!(clear(&mut tx,&board,post).await.unwrap(),Some(1));
    let retained:i64=sqlx::query_scalar("SELECT count(*) FROM content.reports WHERE id=ANY($1)").bind(vec![a,b,fresh]).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(retained,3);
    }).await;
}

#[tokio::test]
async fn unknown_weights_fail_closed_and_inconsistent_marker_pair_is_rejected() {
    let _serial = TEST.lock().await;
    run_case(|pool, board, post, revision| async move {
        let mut tx = pool.acquire().await.unwrap();
        admit(&mut tx, &board, post, Some(revision), false).await;
        activate(&mut tx, None).await;
        let unknown = admit(&mut tx, &board, post, None, false).await;
        let before = snapshot(&mut tx, &board).await;
        let error = clear(&mut tx, &board, post).await.unwrap_err();
        assert_eq!(code(&error), "P0108");
        reset(&mut tx).await;
        assert_eq!(snapshot(&mut tx, &board).await, before);
        assert_eq!(cleared(&mut tx, &board, post).await, None);
        // Keep 0102's numeric-pair constraint intact: unsupported negative/zero
        // weights cannot be fabricated into known evidence for this test.
        let error = sqlx::query(
            "UPDATE content.reports SET group_cleared_at=clock_timestamp() WHERE id=$1",
        )
        .bind(unknown)
        .execute(&mut *tx)
        .await
        .unwrap_err();
        assert_eq!(code(&error), "23514");
    })
    .await;
}

#[tokio::test]
async fn minimal_runtime_grants_and_readiness() {
    let _serial = TEST.lock().await;
    for (variable, role) in [
        ("TEST_PUBLIC_DATABASE_URL", "board_public"),
        ("AUTH_DATABASE_URL", "board_auth"),
        ("STAFF_DATABASE_URL", "board_staff"),
    ] {
        let pool = PgPool::connect(&std::env::var(variable).unwrap())
            .await
            .unwrap();
        let actual: String = sqlx::query_scalar("SELECT current_user::text")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(actual, role);
        if role != "board_staff" {
            let error = sqlx::query("SELECT content.clear_report_group('missing',1,1)")
                .execute(&pool)
                .await
                .unwrap_err();
            assert_eq!(code(&error), "42501");
        }
        for statement in [
            "SELECT * FROM post_secrets.report_group LIMIT 0",
            "SELECT * FROM post_secrets.report_membership LIMIT 0",
            "SELECT * FROM post_secrets.report_weight_evidence LIMIT 0",
            "UPDATE content.reports SET group_cleared_at=clock_timestamp(),group_cleared_by=1,group_clear_inherited=false WHERE false",
        ] {
            assert_eq!(
                code(&sqlx::query(statement).execute(&pool).await.unwrap_err()),
                "42501",
                "{role}: {statement}"
            );
        }
        if role != "board_auth" {
            let ready: bool = sqlx::query_scalar(board_store::report_admission::READINESS_SQL)
                .fetch_one(&pool)
                .await
                .unwrap();
            assert!(ready, "{role}");
        }
        pool.close().await;
    }
}

#[tokio::test]
async fn helper_rejects_mismatched_group_markers_and_enforces_ten_thousand_member_bound() {
    let _serial = TEST.lock().await;
    run_case(|pool,board,post,revision| async move {
    let mut tx=pool.acquire().await.unwrap();
    let report=admit(&mut tx,&board,post,Some(revision),false).await;
    // All marker fields are populated, so the row constraint accepts this;
    // the helper must independently reject a mismatch with private lifetime.
    sqlx::query("UPDATE content.reports SET group_cleared_at=clock_timestamp(),group_cleared_by=77,group_clear_inherited=false WHERE id=$1").bind(report).execute(&mut *tx).await.unwrap();
    assert_eq!(code(&clear(&mut tx,&board,post).await.unwrap_err()),"23514");
    reset(&mut tx).await;
    sqlx::query("UPDATE content.reports SET group_cleared_at=NULL,group_cleared_by=NULL,group_clear_inherited=NULL WHERE id=$1").bind(report).execute(&mut *tx).await.unwrap();
    let ids:Vec<i64>=sqlx::query_scalar("INSERT INTO content.reports(board,post_id,reason) SELECT $1,$2,'Owned capacity row' FROM generate_series(1,10000) RETURNING id").bind(&board).bind(post).fetch_all(&mut *tx).await.unwrap();
    let mut bulk=pool.begin().await.unwrap();
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE").bind(&board).execute(&mut *bulk).await.unwrap();
    private(&mut bulk).await;
    sqlx::query("INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at) SELECT id,decode(repeat('bc',32),'hex'),$2,$3,$3,clock_timestamp() FROM unnest($1::bigint[]) id")
        .bind(&ids).bind(&board).bind(post).execute(&mut *bulk).await.unwrap();
    reset(&mut bulk).await;
    bulk.commit().await.unwrap();
    assert_eq!(code(&clear(&mut tx,&board,post).await.unwrap_err()),"54000");
    reset(&mut tx).await;
    assert_eq!(cleared(&mut tx,&board,post).await,None);
    let marked:i64=sqlx::query_scalar("SELECT count(*) FROM content.reports WHERE board=$1 AND group_cleared_at IS NOT NULL").bind(&board).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(marked,0);
    }).await;
}

#[tokio::test]
async fn audit_count_is_positive_bounded_and_exclusive_to_group_clear() {
    let _serial = TEST.lock().await;
    run_case(|pool,board,post,_| async move {
    let mut tx=pool.acquire().await.unwrap();
    for (action,count) in [("report-group-clear",None),("report-group-clear",Some(0_i64)),("report-group-clear",Some(10001)),("resolve",Some(1))] {
            let error=sqlx::query("INSERT INTO content.moderation_audit(account_id,board,target_id,action,group_clear_count) VALUES(77,$1,$2,$3,$4)").bind(&board).bind(post).bind(action).bind(count).execute(&mut *tx).await.unwrap_err();
        assert_eq!(code(&error),"23514","{action}: {count:?}");
        }
    }).await;
}

#[tokio::test]
async fn archive_and_post_deletion_retire_cleared_lifetimes_before_waiting_clear_resumes() {
    let _serial = TEST.lock().await;
    for archive in [true, false] {
        run_case(move |pool, board, post, revision| async move {
            let mut observer = pool.acquire().await.unwrap();
            let report = admit(&mut observer, &board, post, Some(revision), false).await;
            assert_eq!(clear(&mut observer, &board, post).await.unwrap(), Some(1));
            let before: String = sqlx::query_scalar(
                "SELECT to_jsonb(r)::text FROM content.reports r WHERE id=$1",
            )
            .bind(report)
            .fetch_one(&mut *observer)
            .await
            .unwrap();
            assert!(cleared(&mut observer, &board, post).await.is_some());

            let staff = PgPool::connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
                .await
                .unwrap();
            let mut waiting_connection = staff.acquire().await.unwrap();
            let (role, waiter): (String, i32) =
                sqlx::query_as("SELECT current_user::text,pg_backend_pid()")
                    .fetch_one(&mut *waiting_connection)
                    .await
                    .unwrap();
            assert_eq!(role, "board_staff");
            let mut retirement = pool.begin().await.unwrap();
            let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut *retirement)
                .await
                .unwrap();
            sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
                .bind(&board)
                .execute(&mut *retirement)
                .await
                .unwrap();
            let target = board.clone();
            let pending = tokio::spawn(async move {
                sqlx::query_scalar::<_, Option<i64>>(
                    "SELECT content.clear_report_group($1,$2,77)",
                )
                .bind(target)
                .bind(post)
                .fetch_one(&mut *waiting_connection)
                .await
            });
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    let blocked: bool = sqlx::query_scalar(
                        "SELECT $2=ANY(pg_blocking_pids($1))",
                    )
                    .bind(waiter)
                    .bind(blocker)
                    .fetch_one(&mut *observer)
                    .await
                    .unwrap();
                    if blocked {
                        break;
                    }
                    tokio::task::yield_now().await;
                }
            })
            .await
            .expect("actual staff helper must wait on the owned target board");

            // Exercise the real lifetime-retirement triggers under their
            // supported board-first operator ordering. This does not invoke
            // either HTTP moderation endpoint or claim endpoint authority.
            if archive {
                sqlx::query("UPDATE content.boards SET archive_retention_seconds=3600 WHERE slug=$1")
                    .bind(&board).execute(&mut *retirement).await.unwrap();
                sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
                    .bind(post).execute(&mut *retirement).await.unwrap();
            } else {
                sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1")
                    .bind(post)
                    .execute(&mut *retirement)
                    .await
                    .unwrap();
            }
            retirement.commit().await.unwrap();
            assert_eq!(pending.await.unwrap().unwrap(), None);
            staff.close().await;

            assert_eq!(cleared(&mut observer, &board, post).await, None);
            private(&mut observer).await;
            let remaining: (i64, i64) = sqlx::query_as(
                "SELECT (SELECT count(*) FROM post_secrets.report_membership WHERE board=$1 AND post_id=$2),(SELECT count(*) FROM post_secrets.report_group WHERE board=$1 AND post_id=$2)",
            )
            .bind(&board)
            .bind(post)
            .fetch_one(&mut *observer)
            .await
            .unwrap();
            reset(&mut observer).await;
            assert_eq!(remaining, (0, 0));
            let after: String = sqlx::query_scalar(
                "SELECT to_jsonb(r)::text FROM content.reports r WHERE id=$1",
            )
            .bind(report)
            .fetch_one(&mut *observer)
            .await
            .unwrap();
            assert_eq!(after, before, "retirement must retain original clear evidence");
        })
        .await;
    }
}
