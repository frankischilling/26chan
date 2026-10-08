#![cfg(feature = "database-tests")]
mod support;

use board_domain::anonymous_session::{Activity, Capability, State};
use board_store::anonymous_session::PostingSession;
use chrono::Utc;
use serde_json::{Value, json};
use sqlx::{PgConnection, PgPool};
use std::{
    future::Future,
    sync::{Arc, OnceLock},
};

static TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
// Exactly one immutable synthetic revision remains until disposable-cluster
// teardown. Mutable activation, owned content, sessions and policy are restored.
static CATALOG: OnceLock<i64> = OnceLock::new();
const BASES: [f64; 5] = [0.5, 0.0, -2.25, 1.0, 2.5];

async fn login(variable: &str, expected: &str) -> PgPool {
    let pool = PgPool::connect(&std::env::var(variable).expect("owned database URL required"))
        .await
        .unwrap();
    let actual: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(actual, expected);
    pool
}
async fn reset(c: &mut PgConnection) {
    sqlx::query("RESET ROLE").execute(c).await.unwrap();
}
async fn private(c: &mut PgConnection) {
    sqlx::query("SET LOCAL ROLE board_report_admission_owner")
        .execute(c)
        .await
        .unwrap();
}
fn code(error: &sqlx::Error) -> String {
    error
        .as_database_error()
        .unwrap()
        .code()
        .unwrap()
        .into_owned()
}
fn session(cap: &Capability, minted: bool) -> PostingSession {
    PostingSession {
        fingerprints: cap.fingerprints(Some("198.51.100.7".parse().unwrap()), *b"US"),
        minted,
        now: Utc::now(),
    }
}

struct Fixture {
    owner: PgPool,
    public: PgPool,
    staff: PgPool,
    board: String,
    op: i64,
    reply: i64,
    category: i64,
    revision: Option<i64>,
    previous_revision: Option<i64>,
    previous_limit: i32,
    actor: [u8; 32],
    cap: Capability,
    reserve: Capability,
}
impl Fixture {
    async fn new(base: Option<f64>) -> Self {
        assert!(
            std::env::var("BOARD_TEST_CLUSTER").is_ok_and(|path| path
                .strip_prefix("/tmp/board-postgres.")
                .is_some_and(
                    |tag| tag.len() == 8 && tag.bytes().all(|b| b.is_ascii_alphanumeric())
                )),
            "Committed evidence fixtures require the explicit disposable cluster marker"
        );
        let owner = login("MIGRATION_DATABASE_URL", "board_migrator").await;
        let public = login("TEST_PUBLIC_DATABASE_URL", "board_public").await;
        let staff = login("STAFF_DATABASE_URL", "board_staff").await;
        let mut tx = owner.begin().await.unwrap();
        private(&mut tx).await;
        let previous_revision = sqlx::query_scalar("SELECT active_catalog_revision FROM post_secrets.report_admission_gate WHERE singleton")
            .fetch_one(&mut *tx).await.unwrap();
        reset(&mut tx).await;
        let previous_limit = sqlx::query_scalar(
            "SELECT session_limit FROM post_secrets.anonymous_policy WHERE singleton",
        )
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        let op: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        let board = format!("we{op:x}");
        sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Owned weight evidence','Synthetic',2000,100,100,100,10)")
            .bind(&board).execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO content.threads(id,board) VALUES($1,$2)")
            .bind(op)
            .bind(&board)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','','Synthetic evidence OP')")
            .bind(op).bind(&board).execute(&mut *tx).await.unwrap();
        let reply = sqlx::query_scalar("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,$2,'Anonymous','','Synthetic evidence reply') RETURNING id")
            .bind(&board).bind(op).fetch_one(&mut *tx).await.unwrap();
        let mut imported = None;
        let revision = if base.is_some() {
            let revision = if let Some(revision) = CATALOG.get() {
                *revision
            } else {
                let rows: Vec<Value> = BASES.iter().enumerate().map(|(index,weight)|json!({"id":index+7,"board":"","op_only":false,"reply_only":false,"image_only":false,"exclude_boards":null,"title":"Synthetic evidence category","weight":weight,"filtered":0})).collect();
                let revision: i64 =
                    sqlx::query_scalar("SELECT content.import_report_catalog($1::text::jsonb)")
                        .bind(json!({"version":1,"categories":rows}).to_string())
                        .fetch_one(&mut *tx)
                        .await
                        .unwrap();
                imported = Some(revision);
                revision
            };
            Some(revision)
        } else {
            None
        };
        sqlx::query("SELECT content.set_report_catalog_active($1)")
            .bind(revision)
            .execute(&mut *tx)
            .await
            .unwrap();
        tx.commit().await.unwrap();
        if let Some(revision) = imported {
            CATALOG.set(revision).unwrap();
        }
        let category = base.map_or(7, |weight| {
            BASES.iter().position(|value| *value == weight).unwrap() as i64 + 7
        });
        let actor = *support::fresh_key()
            .public_report_rate_identity("198.51.100.7".parse().unwrap())
            .as_bytes();
        Self {
            owner,
            public,
            staff,
            board,
            op,
            reply,
            category,
            revision,
            previous_revision,
            previous_limit,
            actor,
            cap: Capability::generate().unwrap(),
            reserve: Capability::generate().unwrap(),
        }
    }
    async fn cleanup(&self) {
        let mut tx = self.owner.begin().await.unwrap();
        sqlx::query("SELECT content.set_report_catalog_active($1)")
            .bind(self.previous_revision)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("UPDATE post_secrets.anonymous_policy SET session_limit=$1 WHERE singleton")
            .bind(self.previous_limit)
            .execute(&mut *tx)
            .await
            .unwrap();
        for query in [
            "DELETE FROM content.reports WHERE board=$1",
            "DELETE FROM content.posts WHERE board=$1",
            "DELETE FROM content.threads WHERE board=$1",
            "DELETE FROM content.boards WHERE slug=$1",
        ] {
            sqlx::query(query)
                .bind(&self.board)
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        for token in [self.cap.storage_hash(), self.reserve.storage_hash()] {
            sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
                .bind(token.as_slice())
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        tx.commit().await.unwrap();
    }
    async fn admit(
        &self,
        target: i64,
        category: i64,
        revision: Option<i64>,
        request: PostingSession,
    ) -> Result<i64, sqlx::Error> {
        let f = request.fingerprints;
        let mut tx = self.public.begin().await?;
        let result = if self.revision.is_some() {
            sqlx::query_scalar(
                "SELECT content.admit_categorical_report($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
            )
            .bind(&self.board)
            .bind(target)
            .bind(category)
            .bind(revision)
            .bind(self.actor.as_slice())
            .bind(f.token.as_slice())
            .bind(f.network.as_slice())
            .bind(f.address.as_slice())
            .bind(f.environment.as_slice())
            .bind(request.minted)
            .bind(request.now.timestamp())
            .fetch_one(&mut *tx)
            .await
        } else {
            sqlx::query_scalar("SELECT content.admit_report($1,$2,'Synthetic free-text reason',$3,$4,$5,$6,$7,$8,$9)")
                .bind(&self.board).bind(target).bind(self.actor.as_slice())
                .bind(f.token.as_slice()).bind(f.network.as_slice()).bind(f.address.as_slice()).bind(f.environment.as_slice())
                .bind(request.minted).bind(request.now.timestamp()).fetch_one(&mut *tx).await
        };
        if result.is_ok() {
            tx.commit().await?;
        } else {
            tx.rollback().await?;
        }
        result
    }
    async fn evidence(&self, report: i64) -> Option<Value> {
        let mut tx = self.owner.begin().await.unwrap();
        private(&mut tx).await;
        let raw:Option<String>=sqlx::query_scalar("SELECT to_jsonb(e)::text FROM post_secrets.report_weight_evidence e WHERE report_id=$1")
            .bind(report).fetch_optional(&mut *tx).await.unwrap();
        tx.rollback().await.unwrap();
        raw.map(|value| serde_json::from_str(&value).unwrap())
    }
    async fn snapshot(&self) -> Value {
        let mut tx = self.owner.begin().await.unwrap();
        let raw:String=sqlx::query_scalar("SELECT jsonb_build_object('reports',(SELECT coalesce(jsonb_agg(to_jsonb(r) ORDER BY id),'[]') FROM content.reports r WHERE board=$1),'session',(SELECT to_jsonb(s) FROM post_secrets.anonymous_sessions s WHERE token_hash=$2),'links',(SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY report_id),'[]') FROM post_secrets.anonymous_reports a WHERE token_hash=$2))::text")
            .bind(&self.board).bind(self.cap.storage_hash().as_slice()).fetch_one(&mut *tx).await.unwrap();
        let mut value: Value = serde_json::from_str(&raw).unwrap();
        let ids: Vec<i64> = sqlx::query_scalar("SELECT id FROM content.reports WHERE board=$1")
            .bind(&self.board)
            .fetch_all(&mut *tx)
            .await
            .unwrap();
        private(&mut tx).await;
        let raw:String=sqlx::query_scalar("SELECT coalesce(jsonb_agg(to_jsonb(m) ORDER BY report_id),'[]')::text FROM post_secrets.report_membership m WHERE board=$1")
            .bind(&self.board).fetch_one(&mut *tx).await.unwrap();
        value["members"] = serde_json::from_str(&raw).unwrap();
        let raw:String=sqlx::query_scalar("SELECT coalesce(jsonb_agg(to_jsonb(e) ORDER BY report_id),'[]')::text FROM post_secrets.report_weight_evidence e WHERE report_id=ANY($1::bigint[])")
            .bind(ids).fetch_one(&mut *tx).await.unwrap();
        value["evidence"] = serde_json::from_str(&raw).unwrap();
        tx.rollback().await.unwrap();
        value
    }
}
async fn run_case<F, Fut>(base: Option<f64>, body: F)
where
    F: FnOnce(Arc<Fixture>) -> Fut + Send + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    let fixture = Arc::new(Fixture::new(base).await);
    let work = Arc::clone(&fixture);
    let result = tokio::spawn(async move {
        body(work).await;
    })
    .await;
    fixture.cleanup().await;
    result.unwrap();
}
fn assert_unknowns(e: &Value) {
    assert_eq!(e["evaluator_version"], 1);
    for field in [
        "authenticated_janitor_or_higher",
        "threat_at_least_point_four",
        "history_filtered",
        "source_reason",
    ] {
        assert_eq!(
            e[field],
            Value::Null,
            "unsupported source evidence must remain Unknown: {field}"
        );
    }
    assert!(e["evaluated_at"].as_str().is_some());
}
async fn seed(c: &mut PgConnection, request: PostingSession, state: State) {
    let f = request.fingerprints;
    sqlx::query("INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,created_at,network_at,address_at,environment_at,activity_at,action_at,expires_at,verified_level,posts,reports,pending) VALUES($1,$2,$3,$4,$5,$6,$5,$5,$7,$8,$9,$10,$11,$12,$13)")
        .bind(f.token.as_slice()).bind(f.network.as_slice()).bind(f.address.as_slice()).bind(f.environment.as_slice())
        .bind(state.created_at as i64).bind(state.network_at as i64).bind(state.activity_at as i64).bind(state.action_at as i64)
        .bind(request.now.timestamp()+31_536_000).bind(i16::from(state.verified_level)).bind(i16::from(state.posts))
        .bind(i16::from(state.reports)).bind(i16::from(state.pending)).execute(c).await.unwrap();
}

#[tokio::test]
async fn only_base_equals_fallback_has_numeric_proof_and_unknown_source_reason_is_preserved() {
    let _serial = TEST.lock().await;
    for base in [0.5, 0.0, -0.0, -2.25, 1.0, 2.5] {
        run_case(Some(base), move |f| async move {
            let before: chrono::DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
                .fetch_one(&f.owner)
                .await
                .unwrap();
            let report = f
                .admit(f.op, f.category, f.revision, session(&f.cap, true))
                .await
                .unwrap();
            let e = f.evidence(report).await.unwrap();
            assert_unknowns(&e);
            assert_eq!(e["report_id"], report);
            assert_eq!(e["known_or_verified"], false);
            assert_eq!(
                e["effective_weight"],
                if base == 0.5 { json!(0.5) } else { Value::Null }
            );
            assert_eq!(
                e["numeric_proof"],
                if base == 0.5 {
                    json!("BaseEqualsFallback")
                } else {
                    Value::Null
                }
            );
            let evaluated =
                chrono::DateTime::parse_from_rfc3339(e["evaluated_at"].as_str().unwrap()).unwrap();
            let after: chrono::DateTime<Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
                .fetch_one(&f.owner)
                .await
                .unwrap();
            assert!(evaluated >= before && evaluated <= after);
            let stored: (f64, String) = sqlx::query_as(
                "SELECT category_base_weight,reason FROM content.reports WHERE id=$1",
            )
            .bind(report)
            .fetch_one(&f.owner)
            .await
            .unwrap();
            assert_eq!(stored, (base, "Synthetic evidence category".into()));
            let snap = f.snapshot().await;
            assert_eq!(snap["links"][0]["report_id"], report);
            assert_eq!(
                snap["members"][0]["automatic_identity"],
                snap["session"]["automatic_identity"]
            );
            assert!(!snap["session"]["automatic_identity"].is_null());
        })
        .await;
    }
}
#[tokio::test]
async fn both_anonymous_admissions_capture_pre_report_state_not_the_new_pending_report() {
    let _serial = TEST.lock().await;
    for base in [None, Some(0.5)] {
        for initially_known in [false, true] {
            run_case(base, move |f| async move {
                let request = session(&f.cap, false);
                let at = request.now.timestamp() as u64;
                let mut state = State::new(at - 7_200);
                state.network_at = at - 1_800;
                state.activity_at = at - 1;
                state.action_at = at - 100;
                state.reports = if initially_known { 10 } else { 9 };
                assert_eq!(state.is_known_or_verified(at, 60, 0), initially_known);
                let mut tx = f.owner.begin().await.unwrap();
                seed(&mut tx, request, state).await;
                tx.commit().await.unwrap();
                let report = f
                    .admit(f.op, f.category, f.revision, request)
                    .await
                    .unwrap();
                let e = f.evidence(report).await.unwrap();
                assert_unknowns(&e);
                assert_eq!(e["known_or_verified"], initially_known);
                let pending: i16 = sqlx::query_scalar(
                    "SELECT pending FROM post_secrets.anonymous_sessions WHERE token_hash=$1",
                )
                .bind(f.cap.storage_hash().as_slice())
                .fetch_one(&f.owner)
                .await
                .unwrap();
                assert_eq!(pending & 8, 8);
                state.update(at, Activity::Report, false);
                assert!(state.is_known_or_verified(at, 60, 0));
                if base.is_none() {
                    assert!(e["effective_weight"].is_null());
                    assert!(e["numeric_proof"].is_null());
                }
            })
            .await;
        }
    }
}
#[tokio::test]
async fn category_quota_and_late_registration_failures_leave_no_partial_evidence_or_activity() {
    let _serial = TEST.lock().await;
    run_case(Some(0.5), |f| async move {
        let before = f.snapshot().await;
        for (category, revision, message) in [
            (999, f.revision, "Invalid category selected."),
            (
                f.category,
                f.revision.map(|r| r + 1),
                "Report categories changed. Please reload the report form.",
            ),
        ] {
            let error = f
                .admit(f.op, category, revision, session(&f.cap, true))
                .await
                .unwrap_err();
            assert_eq!(error.as_database_error().unwrap().message(), message);
            assert_eq!(f.snapshot().await, before);
        }
        let mut stale = session(&f.cap, true);
        stale.now -= chrono::Duration::seconds(120);
        assert_eq!(
            code(
                &f.admit(f.op, f.category, f.revision, stale)
                    .await
                    .unwrap_err()
            ),
            "23514"
        );
        assert_eq!(f.snapshot().await, before);
        let report = f
            .admit(f.op, f.category, f.revision, session(&f.cap, true))
            .await
            .unwrap();
        let admitted = f.snapshot().await;
        assert_eq!(admitted["evidence"][0]["report_id"], report);
        for (target, message) in [
            (f.op, "You have already reported this post."),
            (
                f.reply,
                "You have to wait a while before reporting another post.",
            ),
        ] {
            let error = f
                .admit(target, f.category, f.revision, session(&f.cap, false))
                .await
                .unwrap_err();
            assert_eq!(error.as_database_error().unwrap().message(), message);
            assert_eq!(f.snapshot().await, admitted);
        }
    })
    .await;
    for base in [None, Some(0.5)] {
        run_case(base, |f| async move {
            let mut tx = f.owner.begin().await.unwrap();
            let request = session(&f.reserve, false);
            seed(&mut tx, request, State::new(request.now.timestamp() as u64)).await;
            sqlx::query("UPDATE post_secrets.anonymous_policy SET session_limit=1 WHERE singleton")
                .execute(&mut *tx)
                .await
                .unwrap();
            tx.commit().await.unwrap();
            let before = f.snapshot().await;
            let error = f
                .admit(f.op, f.category, f.revision, session(&f.cap, true))
                .await
                .unwrap_err();
            assert_eq!(code(&error), "53300");
            assert_eq!(f.snapshot().await, before);
        })
        .await;
    }
}
#[tokio::test]
async fn preexisting_and_legacy_ip_only_reports_stay_without_evidence() {
    let _serial = TEST.lock().await;
    run_case(None,|f|async move {
        // Tests later admissions; migration-time no-backfill has a separate upgrade fixture.
        let historical:i64=sqlx::query_scalar("INSERT INTO content.reports(board,post_id,reason) VALUES($1,$2,'Synthetic retained report') RETURNING id")
            .bind(&f.board).bind(f.op).fetch_one(&f.owner).await.unwrap();
        let report=f.admit(f.op,f.category,None,session(&f.cap,true)).await.unwrap();
        assert!(f.evidence(report).await.is_some()); assert!(f.evidence(historical).await.is_none());
    }).await;
    run_case(None, |f| async move {
        let mut tx = f.staff.begin().await.unwrap();
        let report: i64 = sqlx::query_scalar(
            "SELECT content.admit_report($1,$2,'Synthetic legacy IP report',$3)",
        )
        .bind(&f.board)
        .bind(f.op)
        .bind(f.actor.as_slice())
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
        assert!(f.evidence(report).await.is_none());
    })
    .await;
}
#[tokio::test]
async fn evidence_unknowns_and_numeric_proofs_are_constrained_even_for_private_owner() {
    let _serial = TEST.lock().await;
    run_case(Some(0.5),|f|async move {
        let report=f.admit(f.op,f.category,f.revision,session(&f.cap,true)).await.unwrap();
        let before=f.evidence(report).await;
        let mut tx=f.owner.begin().await.unwrap(); private(&mut tx).await;
        for query in [
            "UPDATE post_secrets.report_weight_evidence SET evaluator_version=2 WHERE report_id=$1",
            "UPDATE post_secrets.report_weight_evidence SET authenticated_janitor_or_higher=true WHERE report_id=$1",
            "UPDATE post_secrets.report_weight_evidence SET authenticated_janitor_or_higher=false WHERE report_id=$1",
            "UPDATE post_secrets.report_weight_evidence SET threat_at_least_point_four=false WHERE report_id=$1",
            "UPDATE post_secrets.report_weight_evidence SET history_filtered=false WHERE report_id=$1",
            "UPDATE post_secrets.report_weight_evidence SET source_reason='known' WHERE report_id=$1",
            "UPDATE post_secrets.report_weight_evidence SET effective_weight=1.0 WHERE report_id=$1",
            "UPDATE post_secrets.report_weight_evidence SET effective_weight=NULL WHERE report_id=$1",
            "UPDATE post_secrets.report_weight_evidence SET numeric_proof=NULL WHERE report_id=$1",
            "UPDATE post_secrets.report_weight_evidence SET numeric_proof='Staff' WHERE report_id=$1",
            "UPDATE post_secrets.report_weight_evidence SET effective_weight='NaN'::double precision WHERE report_id=$1",
        ] {
            sqlx::query("SAVEPOINT invalid_evidence").execute(&mut *tx).await.unwrap();
            assert_eq!(code(&sqlx::query(query).bind(report).execute(&mut *tx).await.unwrap_err()),"23514","{query}");
            sqlx::query("ROLLBACK TO SAVEPOINT invalid_evidence").execute(&mut *tx).await.unwrap();
        }
        tx.rollback().await.unwrap(); assert_eq!(f.evidence(report).await,before);
    }).await;
}
#[tokio::test]
async fn actual_runtime_logins_cannot_read_write_or_call_private_evidence_authority() {
    let _serial = TEST.lock().await;
    for (variable, role) in [
        ("TEST_PUBLIC_DATABASE_URL", "board_public"),
        ("STAFF_DATABASE_URL", "board_staff"),
        ("AUTH_DATABASE_URL", "board_auth"),
    ] {
        let pool = login(variable, role).await;
        let privileges:(bool,bool,bool,bool)=sqlx::query_as("SELECT has_table_privilege(current_user,c.oid,'SELECT'),has_table_privilege(current_user,c.oid,'INSERT'),has_table_privilege(current_user,c.oid,'UPDATE'),has_table_privilege(current_user,c.oid,'DELETE') FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace WHERE n.nspname='post_secrets' AND c.relname='report_weight_evidence'")
            .fetch_one(&pool).await.unwrap();
        assert_eq!(privileges, (false, false, false, false), "{role}");
        for query in [
            "SELECT * FROM post_secrets.report_weight_evidence",
            "UPDATE post_secrets.report_weight_evidence SET known_or_verified=true WHERE false",
            "DELETE FROM post_secrets.report_weight_evidence WHERE false",
            "INSERT INTO post_secrets.report_weight_evidence(report_id,evaluator_version,evaluated_at) VALUES(1,1,clock_timestamp())",
            "SELECT post_secrets.report_known_or_verified(NULL,NULL,NULL,NULL,true,1)",
        ] {
            assert_eq!(
                code(&sqlx::query(query).execute(&pool).await.unwrap_err()),
                "42501",
                "{role}: {query}"
            );
        }
    }
}
