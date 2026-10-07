#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use board_staff::{AppState, Config, Limits, auth, router};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use tower::ServiceExt;
use webauthn_rs::prelude::*;

// Each test owns its boards, account, and failure-injection predicate.
static TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone)]
struct Fixture {
    owner: PgPool,
    auth: PgPool,
    staff: PgPool,
    app: Router,
    account: i64,
    token: String,
    csrf: String,
    boards: [String; 2],
    posts: [i64; 2],
    revision: i64,
}

impl Fixture {
    async fn new() -> Self {
        Self::with_staff_pool(None).await
    }

    async fn with_staff_pool(staff_override: Option<PgPool>) -> Self {
        assert!(std::env::var("BOARD_TEST_CLUSTER").is_ok_and(|path| {
            path.strip_prefix("/tmp/board-postgres.")
                .is_some_and(|tag| tag.len() == 8 && tag.bytes().all(|b| b.is_ascii_alphanumeric()))
        }));
        async fn pool(key: &str) -> PgPool {
            PgPool::connect(&std::env::var(key).unwrap()).await.unwrap()
        }
        let owner = pool("MIGRATION_DATABASE_URL").await;
        let auth_pool = pool("AUTH_DATABASE_URL").await;
        let staff = match staff_override {
            Some(staff) => staff,
            None => pool("STAFF_DATABASE_URL").await,
        };
        let state = Arc::new(AppState {
            config: Config {
                proxy: None,
                poster_id_key: None,
                country_database: None,
                origin: "http://localhost:3001".into(),
                public_origin: "http://localhost:3000".into(),
                media_origin: "http://127.0.0.1:3002".into(),
                bind: "127.0.0.1:3001".parse().unwrap(),
                production: false,
                auth_database: String::new(),
                staff_database: String::new(),
                idle_timeout: Duration::from_secs(60),
                tripcode_key: None,
            },
            auth: auth_pool.clone(),
            staff: staff.clone(),
            webauthn: WebauthnBuilder::new(
                "localhost",
                &Url::parse("http://localhost:3001").unwrap(),
            )
            .unwrap()
            .build()
            .unwrap(),
            limits: Limits::default(),
        });
        let boards =
            [0, 1].map(|_| format!("gc{}", &uuid::Uuid::new_v4().simple().to_string()[..8]));
        let mut posts = [0; 2];
        for (index, board) in boards.iter().enumerate() {
            sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Owned scope board','',1000,100,100,100,10)")
                .bind(board).execute(&owner).await.unwrap();
            posts[index] =
                sqlx::query_scalar("INSERT INTO content.threads(board) VALUES($1) RETURNING id")
                    .bind(board)
                    .fetch_one(&owner)
                    .await
                    .unwrap();
            sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','Owned thread','Scope fixture')")
                .bind(posts[index]).bind(board).execute(&owner).await.unwrap();
        }
        let account = sqlx::query_scalar("INSERT INTO staff_identity.accounts(role,allow_boards,deny_boards) VALUES('moderator',$1,$2) RETURNING id")
            .bind(vec!["all".to_owned()]).bind(Vec::<String>::new()).fetch_one(&owner).await.unwrap();
        let credential = uuid::Uuid::new_v4().as_bytes().to_vec();
        sqlx::query(
            "INSERT INTO staff_identity.credentials(id,account_id,credential) VALUES($1,$2,'{}')",
        )
        .bind(&credential)
        .bind(account)
        .execute(&owner)
        .await
        .unwrap();
        let token = auth::token();
        let csrf = auth::token();
        sqlx::query("INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id) VALUES($1,$2,$3,$4)")
            .bind(auth::hash(&token)).bind(auth::hash(&csrf)).bind(account).bind(credential)
            .execute(&owner).await.unwrap();
        let revision = sqlx::query_scalar("SELECT content.import_report_catalog($1::text::jsonb)")
            .bind(serde_json::json!({"version":1,"categories":[{"id":31,"board":"","op_only":false,"reply_only":false,"image_only":false,"exclude_boards":null,"title":"Owned group clear category","weight":0.5,"filtered":0}]}).to_string())
            .fetch_one(&owner).await.unwrap();
        Self {
            revision,
            owner,
            auth: auth_pool,
            staff,
            app: router(state),
            account,
            token,
            csrf,
            boards,
            posts,
        }
    }

    async fn request(&self, path: &str, form: Option<String>) -> (StatusCode, String) {
        let mut request = Request::builder().uri(path).header(
            "cookie",
            format!("staff={}; staff-csrf={}", self.token, self.csrf),
        );
        let body = if let Some(form) = form {
            request = request
                .method(Method::POST)
                .header("content-type", "application/x-www-form-urlencoded")
                .header("origin", "http://localhost:3001")
                .header("sec-fetch-site", "same-origin");
            Body::from(form)
        } else {
            Body::empty()
        };
        let response = self
            .app
            .clone()
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap();
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        let status = response.status();
        let text = String::from_utf8(
            to_bytes(response.into_body(), 1_000_000)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        (status, text)
    }

    async fn role(&self, role: &str, allow: &[String], deny: &[String]) {
        sqlx::query("UPDATE staff_identity.accounts SET role=$2,allow_boards=$3,deny_boards=$4,public_capcode=NULL,revoked_at=NULL WHERE id=$1")
            .bind(self.account).bind(role).bind(allow).bind(deny).execute(&self.owner).await.unwrap();
    }

    async fn audit_count(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM content.moderation_audit WHERE account_id=$1")
            .bind(self.account)
            .fetch_one(&self.owner)
            .await
            .unwrap()
    }

    async fn cleanup(self) {
        let mut cleanup = self.owner.begin().await.unwrap();
        sqlx::query("SELECT slug FROM content.boards WHERE slug=ANY($1) ORDER BY slug FOR UPDATE")
            .bind(self.boards.to_vec())
            .execute(&mut *cleanup)
            .await
            .unwrap();
        sqlx::query(
            "SELECT id FROM content.threads WHERE board=ANY($1) ORDER BY board,id FOR UPDATE",
        )
        .bind(self.boards.to_vec())
        .execute(&mut *cleanup)
        .await
        .unwrap();
        sqlx::query("SET LOCAL ROLE board_anonymous_owner")
            .execute(&mut *cleanup)
            .await
            .unwrap();
        sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
            .bind(auth::hash(&self.token))
            .execute(&mut *cleanup)
            .await
            .unwrap();
        sqlx::query("RESET ROLE")
            .execute(&mut *cleanup)
            .await
            .unwrap();
        for statement in [
            "DELETE FROM content.moderation_audit WHERE board=ANY($1)",
            "DELETE FROM content.reports WHERE board=ANY($1)",
            "DELETE FROM post_secrets.posting_thread_actions WHERE board=ANY($1)",
            "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=ANY($1))",
            "DELETE FROM content.post_media WHERE post_id IN(SELECT id FROM content.posts WHERE board=ANY($1))",
            "DELETE FROM content.posts WHERE board=ANY($1)",
            "DELETE FROM content.threads WHERE board=ANY($1)",
            "DELETE FROM content.boards WHERE slug=ANY($1)",
        ] {
            sqlx::query(statement)
                .bind(self.boards.to_vec())
                .execute(&mut *cleanup)
                .await
                .unwrap();
        }
        cleanup.commit().await.unwrap();
        for statement in [
            "DELETE FROM staff_identity.sessions WHERE account_id=$1",
            "DELETE FROM staff_identity.credentials WHERE account_id=$1",
            "DELETE FROM staff_identity.accounts WHERE id=$1",
        ] {
            sqlx::query(statement)
                .bind(self.account)
                .execute(&self.owner)
                .await
                .unwrap();
        }
        self.auth.close().await;
        self.staff.close().await;
        self.owner.close().await;
    }
}

impl Fixture {
    fn form(&self, board: usize, post: i64) -> String {
        format!(
            "csrf={}&board={}&post_id={post}",
            self.csrf, self.boards[board]
        )
    }
    async fn clear(&self, board: usize) -> (StatusCode, String) {
        self.request(
            "/report-group-clear",
            Some(self.form(board, self.posts[board])),
        )
        .await
    }
    async fn report(&self, board: usize, state: &str, actor: u8, known: bool, member: bool) -> i64 {
        let mut tx = self.owner.begin().await.unwrap();
        sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
            .bind(&self.boards[board])
            .execute(&mut *tx)
            .await
            .unwrap();
        let id:i64=sqlx::query_scalar("INSERT INTO content.reports(board,post_id,reason,state,category_revision,category_id,category_kind,category_base_weight) VALUES($1,$2,$3,$4,$5,31,2,0.5) RETURNING id")
            .bind(&self.boards[board]).bind(self.posts[board]).bind(format!("Owned group report {actor}")) .bind(state).bind(self.revision).fetch_one(&mut *tx).await.unwrap();
        sqlx::query("SET LOCAL ROLE board_report_admission_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        if known {
            sqlx::query("INSERT INTO post_secrets.report_weight_evidence(report_id,evaluator_version,known_or_verified,effective_weight,numeric_proof,evaluated_at) VALUES($1,1,false,0.5,'BaseEqualsFallback',clock_timestamp())")
                .bind(id).execute(&mut *tx).await.unwrap();
        }
        if member {
            sqlx::query("INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at) VALUES($1,$2,$3,$4,$4,clock_timestamp())")
                .bind(id).bind(auth::hash(&format!("{}:{actor}",self.token))).bind(&self.boards[board]).bind(self.posts[board]).execute(&mut *tx).await.unwrap();
        }
        tx.commit().await.unwrap();
        id
    }
    async fn snapshot(&self, omit_clear: bool) -> String {
        let mut tx = self.owner.begin().await.unwrap();
        let reports:String=sqlx::query_scalar("SELECT coalesce(jsonb_agg(CASE WHEN $2 THEN to_jsonb(r)-'group_cleared_at'-'group_cleared_by'-'group_clear_inherited' ELSE to_jsonb(r) END ORDER BY id),'[]')::text FROM content.reports r WHERE board=ANY($1)")
            .bind(self.boards.to_vec()).bind(omit_clear).fetch_one(&mut *tx).await.unwrap();
        sqlx::query("SET LOCAL ROLE board_report_admission_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        let private:String=sqlx::query_scalar("SELECT jsonb_build_object('members',(SELECT coalesce(jsonb_agg(to_jsonb(m) ORDER BY report_id),'[]') FROM post_secrets.report_membership m WHERE board=ANY($1)),'evidence',(SELECT coalesce(jsonb_agg(to_jsonb(e) ORDER BY e.report_id),'[]') FROM post_secrets.report_weight_evidence e JOIN post_secrets.report_membership m ON m.report_id=e.report_id WHERE m.board=ANY($1)),'counts',(SELECT coalesce(jsonb_agg(jsonb_build_array(board,post_id,illegal_count,incomplete) ORDER BY board,post_id),'[]') FROM post_secrets.report_group WHERE board=ANY($1)))::text")
            .bind(self.boards.to_vec()).fetch_one(&mut *tx).await.unwrap();
        tx.rollback().await.unwrap();
        format!("{reports}\n{private}")
    }
    async fn marked(&self) -> Vec<i64> {
        sqlx::query_scalar("SELECT id FROM content.reports WHERE board=ANY($1) AND group_cleared_at IS NOT NULL ORDER BY id")
            .bind(self.boards.to_vec()).fetch_all(&self.owner).await.unwrap()
    }
}

#[tokio::test]
async fn janitor_clears_one_group_atomically_preserving_original_dispositions_and_membership() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result=tokio::spawn({let f=f.clone();async move {
        f.role("janitor",&[f.boards[0].clone()],&[]).await;
        let mut ids=Vec::new();
        for (actor,state) in [(1,"open"),(2,"resolved"),(3,"dismissed")] {
            ids.push(f.report(0,state,actor,true,true).await);
        }
        let retired=f.report(0,"open",4,false,false).await;
        let other=f.report(1,"open",5,true,true).await;
        let before=f.snapshot(true).await;
        let (status,html)=f.clear(0).await;
        assert_eq!(status,StatusCode::OK,"{html}");
        assert!(html.contains("Cleared 3 reports"),"{html}");
        assert_eq!(f.marked().await,ids);
        assert_eq!(f.snapshot(true).await,before);
        let audit:(String,i64,i64)=sqlx::query_as("SELECT action,target_id,group_clear_count FROM content.moderation_audit WHERE account_id=$1")
            .bind(f.account).fetch_one(&f.owner).await.unwrap();
        assert_eq!(audit,("report-group-clear".into(),f.posts[0],3));
        let origins:Vec<(i64,bool)>=sqlx::query_as("SELECT group_cleared_by,group_clear_inherited FROM content.reports WHERE id=ANY($1) ORDER BY id")
            .bind(&ids).fetch_all(&f.owner).await.unwrap();
        assert_eq!(origins,vec![(f.account,false);3]);
        let timestamps:i64=sqlx::query_scalar("SELECT count(DISTINCT group_cleared_at) FROM content.reports WHERE id=ANY($1)")
            .bind(&ids).fetch_one(&f.owner).await.unwrap();
        assert_eq!(timestamps,1);
        let stable=f.snapshot(false).await;
        let again=f.clear(0).await;
        assert_eq!(again.0,StatusCode::CONFLICT);
        assert!(again.1.contains("Report group is already cleared."));
        assert_eq!(f.snapshot(false).await,stable);
        assert_eq!(f.audit_count().await,1);
        let queue=f.request("/reports",None).await;
        assert_eq!(queue.0,StatusCode::OK);
        for id in &ids {
            assert!(!queue.1.contains(&format!("id=\"report-{id}\"")));
            for action in ["resolve","dismiss"] {
                assert_eq!(f.request("/moderate",Some(format!("csrf={}&board={}&target={id}&action={action}",f.csrf,f.boards[0]))).await.0,StatusCode::NOT_FOUND);
            }
        }
        let history=f.request(&format!("/reports/cleared?board={}",f.boards[0]),None).await;
        assert_eq!(history.0,StatusCode::OK,"{}",history.1);
        for word in ["resolved","dismissed","Originating clear account:","inherited clear:"] {assert!(history.1.contains(word),"{word}: {}",history.1);}
        assert!(!history.1.contains("Scope fixture"));
        assert!(!history.1.contains("Owned group report 4"));
        assert!(!history.1.contains("Owned group report 5"));
        assert!(!f.marked().await.contains(&retired));
        assert!(!f.marked().await.contains(&other));
    }}).await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn exact_form_origin_csrf_board_scope_and_recent_auth_fail_without_effects() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result=tokio::spawn({let f=f.clone();async move {
        f.report(0,"open",11,true,true).await;
        let before=f.snapshot(false).await;
        for (allow,deny) in [(vec![f.boards[1].clone()],vec![]),(vec!["all".into()],vec![f.boards[0].clone()])] {
            f.role("janitor",&allow,&deny).await;
            assert_eq!(f.clear(0).await.0,StatusCode::FORBIDDEN);
            assert_eq!(f.request(&format!("/reports/cleared?board={}",f.boards[0]),None).await.0,StatusCode::FORBIDDEN);
        }
        f.role("janitor",&[f.boards[0].clone()],&[]).await;
        for field in ["actor_hash=00","report_id=1","group_clear_count=2","csrf=duplicate","post_id=1","board=other"] {
            assert_eq!(f.request("/report-group-clear",Some(format!("{}&{field}",f.form(0,f.posts[0])))).await.0,StatusCode::BAD_REQUEST,"{field}");
        }
        for body in [format!("board={}&post_id={}",f.boards[0],f.posts[0]),format!("csrf={}&board={}",f.csrf,f.boards[0]),format!("csrf={}&board={}&post_id=0",f.csrf,f.boards[0])] {
            assert_eq!(f.request("/report-group-clear",Some(body)).await.0,StatusCode::BAD_REQUEST);
        }
        assert_eq!(f.request("/report-group-clear",Some(f.form(0,f.posts[0]).replace(&f.csrf,&auth::token()))).await.0,StatusCode::FORBIDDEN);
        for origin in [None,Some("https://untrusted.invalid")] {
            let mut request=Request::builder().method(Method::POST).uri("/report-group-clear").header("cookie",format!("staff={}; staff-csrf={}",f.token,f.csrf)).header("content-type","application/x-www-form-urlencoded").header("sec-fetch-site","same-origin");
            if let Some(origin)=origin {request=request.header("origin",origin);}
            assert_eq!(f.app.clone().oneshot(request.body(Body::from(f.form(0,f.posts[0]))).unwrap()).await.unwrap().status(),StatusCode::FORBIDDEN);
        }
        for query in ["", "?board=a&board=b", "?board=a&extra=b"] {
            assert_eq!(f.request(&format!("/reports/cleared{query}"),None).await.0,StatusCode::BAD_REQUEST);
        }
        assert_eq!(f.request("/report-group-clear",Some(f.form(0,i64::MAX))).await.0,StatusCode::NOT_FOUND);
        sqlx::query("UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp()-interval '11 minutes' WHERE token_hash=$1").bind(auth::hash(&f.token)).execute(&f.owner).await.unwrap();
        assert_eq!(f.clear(0).await.0,StatusCode::FORBIDDEN);
        sqlx::query("UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp(),expires_at=clock_timestamp()-interval '1 second' WHERE token_hash=$1").bind(auth::hash(&f.token)).execute(&f.owner).await.unwrap();
        assert_eq!(f.clear(0).await.0,StatusCode::UNAUTHORIZED);
        assert_eq!(f.snapshot(false).await,before);
        assert_eq!(f.audit_count().await,0);
    }}).await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn mixed_unknown_weight_rejects_the_entire_group() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result = tokio::spawn({
        let f = f.clone();
        async move {
            f.report(0, "open", 21, true, true).await;
            f.report(0, "dismissed", 22, false, true).await;
            let before = f.snapshot(false).await;
            let response = f.clear(0).await;
            assert_eq!(response.0, StatusCode::CONFLICT);
            assert!(response.1.contains(
                "Report group cannot be cleared because report weights are unavailable."
            ));
            assert_eq!(f.snapshot(false).await, before);
            assert_eq!(f.audit_count().await, 0);
        }
    })
    .await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn audit_failure_rolls_back_every_marker_and_private_group_state() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let function = format!("group_clear_fault_{}", f.boards[0]);
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE FUNCTION content.{function}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'owned group clear audit failure'; END $$; CREATE TRIGGER {function} BEFORE INSERT ON content.moderation_audit FOR EACH ROW WHEN (NEW.board='{}' AND NEW.action='report-group-clear') EXECUTE FUNCTION content.{function}()",f.boards[0]))).execute(&f.owner).await.unwrap();
    let result = tokio::spawn({
        let f = f.clone();
        async move {
            f.report(0, "open", 31, true, true).await;
            let before = f.snapshot(false).await;
            assert_eq!(f.clear(0).await.0, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(f.snapshot(false).await, before);
            assert!(f.marked().await.is_empty());
            assert_eq!(f.audit_count().await, 0);
        }
    })
    .await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP TRIGGER {function} ON content.moderation_audit; DROP FUNCTION content.{function}()"
    )))
    .execute(&f.owner)
    .await
    .unwrap();
    let retry = f.clear(0).await;
    assert_eq!(retry.0, StatusCode::OK, "{}", retry.1);
    f.cleanup().await;
    result.unwrap();
}

async fn wait_for_content_lock(owner: &PgPool, blocker: i32) {
    tokio::time::timeout(Duration::from_secs(5),async {
        loop {
            let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE usename='board_staff' AND $1=ANY(pg_blocking_pids(pid)))").bind(blocker).fetch_one(owner).await.unwrap();
            if waiting {break;}
            tokio::task::yield_now().await;
        }
    }).await.expect("clear must reach the held board lock");
}

#[tokio::test]
async fn admission_before_clear_is_included_and_competing_clears_audit_once() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result=tokio::spawn({let f=f.clone();async move {
        let seed=f.report(0,"open",41,true,true).await;
        let mut admission=f.owner.begin().await.unwrap();
        let pid:i32=sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *admission).await.unwrap();
        sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE").bind(&f.boards[0]).execute(&mut *admission).await.unwrap();
        let pending=tokio::spawn({let f=f.clone();async move {f.clear(0).await}});
        wait_for_content_lock(&f.owner,pid).await;
        let later:i64=sqlx::query_scalar("INSERT INTO content.reports(board,post_id,reason,category_revision,category_id,category_kind,category_base_weight) VALUES($1,$2,'Concurrent owned categorical',$3,31,2,0.5) RETURNING id")
            .bind(&f.boards[0]).bind(f.posts[0]).bind(f.revision).fetch_one(&mut *admission).await.unwrap();
        sqlx::query("SET LOCAL ROLE board_report_admission_owner").execute(&mut *admission).await.unwrap();
        sqlx::query("INSERT INTO post_secrets.report_weight_evidence(report_id,evaluator_version,effective_weight,numeric_proof,evaluated_at) VALUES($1,1,0.5,'BaseEqualsFallback',clock_timestamp())").bind(later).execute(&mut *admission).await.unwrap();
        sqlx::query("INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at) VALUES($1,$2,$3,$4,$4,clock_timestamp())").bind(later).bind(auth::hash("owned-concurrent-group")).bind(&f.boards[0]).bind(f.posts[0]).execute(&mut *admission).await.unwrap();
        admission.commit().await.unwrap();
        assert_eq!(pending.await.unwrap().0,StatusCode::OK);
        assert_eq!(f.marked().await,vec![seed,later]);
        assert_eq!(f.audit_count().await,1);
        // A separate lifetime lets both calls compete for the first clear.
        f.report(1,"open",42,true,true).await;
        let (a,b)=tokio::join!(f.clear(1),f.clear(1));
        assert!((a.0==StatusCode::OK && b.0==StatusCode::CONFLICT)||(a.0==StatusCode::CONFLICT && b.0==StatusCode::OK),"{a:?}; {b:?}");
        assert_eq!(f.audit_count().await,2);
    }}).await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn authority_expiry_while_waiting_rolls_back_clear_and_audit() {
    let _serial = TEST.lock().await;
    for recent in [false, true] {
        let staff = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .after_connect(|c, _| {
                Box::pin(async move {
                    sqlx::query("SET lock_timeout='10s'")
                        .execute(&mut *c)
                        .await?;
                    sqlx::query("SET statement_timeout='15s'")
                        .execute(c)
                        .await?;
                    Ok(())
                })
            })
            .connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let f = Fixture::with_staff_pool(Some(staff)).await;
        let result=tokio::spawn({let f=f.clone();async move {
            f.report(0,"open",51,true,true).await;
            let before=f.snapshot(false).await;
            let mut blocker=f.owner.begin().await.unwrap();
            let pid:i32=sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *blocker).await.unwrap();
            sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE").bind(&f.boards[0]).execute(&mut *blocker).await.unwrap();
            let statement=if recent {"UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp()-interval '10 minutes'+interval '2 seconds' WHERE token_hash=$1"} else {"UPDATE staff_identity.sessions SET expires_at=clock_timestamp()+interval '2 seconds' WHERE token_hash=$1"};
            sqlx::query(statement).bind(auth::hash(&f.token)).execute(&f.owner).await.unwrap();
            let pending=tokio::spawn({let f=f.clone();async move {f.clear(0).await}});
            wait_for_content_lock(&f.owner,pid).await;
            // Observe the real database clock boundary, rather than hoping a
            // fixed sleep crossed it. Session/account rows stay guard-locked.
            tokio::time::timeout(Duration::from_secs(5),async {
                loop {
                    let expired:bool=sqlx::query_scalar("SELECT CASE WHEN $2 THEN authenticated_at+interval '10 minutes'<=clock_timestamp() ELSE expires_at<=clock_timestamp() END FROM staff_identity.sessions WHERE token_hash=$1")
                        .bind(auth::hash(&f.token)).bind(recent).fetch_one(&f.owner).await.unwrap();
                    if expired {break;}
                    tokio::task::yield_now().await;
                }
            }).await.expect("database authority deadline must pass while clear is blocked");
            blocker.commit().await.unwrap();
            assert_eq!(pending.await.unwrap().0,if recent {StatusCode::FORBIDDEN} else {StatusCode::UNAUTHORIZED});
            assert_eq!(f.snapshot(false).await,before);
            assert_eq!(f.audit_count().await,0);
            // The failed action did not commit private lifetime clear state.
            sqlx::query("UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp(),expires_at=clock_timestamp()+interval '1 hour' WHERE token_hash=$1").bind(auth::hash(&f.token)).execute(&f.owner).await.unwrap();
            assert_eq!(f.clear(0).await.0,StatusCode::OK);
        }}).await;
        f.cleanup().await;
        result.unwrap();
    }
}

#[tokio::test]
async fn cleared_history_is_board_scoped_bounded_and_ignores_live_post_state() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result=tokio::spawn({let f=f.clone();async move {
        let mut ids=Vec::new();
        for _ in 0..101 {ids.push(f.report(0,"open",61,true,true).await);}
        assert_eq!(f.clear(0).await.0,StatusCode::OK);
        sqlx::query("UPDATE content.posts SET comment='PRIVATE LIVE POST CONTENT',deleted=true WHERE id=$1").bind(f.posts[0]).execute(&f.owner).await.unwrap();
        let response=f.request(&format!("/reports/cleared?board={}",f.boards[0]),None).await;
        assert_eq!(response.0,StatusCode::OK);
        assert!(!response.1.contains("PRIVATE LIVE POST CONTENT"));
        assert_eq!(response.1.matches("Originating clear account:").count(),100);
        // Use unique report IDs, not repeated reasons, to prove newest-first cap.
        assert!(!response.1.contains(&format!("id=\"report-{}\"",ids[0])));
        assert!(response.1.contains(&format!("id=\"report-{}\"",ids[100])));
    }}).await;
    f.cleanup().await;
    result.unwrap();
}
