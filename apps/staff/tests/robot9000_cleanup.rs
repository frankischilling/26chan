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
                media: None,
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
            [0, 1].map(|_| format!("rc{}", &uuid::Uuid::new_v4().simple().to_string()[..8]));
        for board in &boards {
            sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,robot9000) VALUES($1,'Owned cleanup board','',1000,100,100,100,10,true)")
                .bind(board).execute(&owner).await.unwrap();
        }
        let account = sqlx::query_scalar("INSERT INTO staff_identity.accounts(role,allow_boards,deny_boards) VALUES('manager',$1,$2) RETURNING id")
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
        Self {
            owner,
            auth: auth_pool,
            staff,
            app: router(state),
            account,
            token,
            csrf,
            boards,
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

    fn form(&self, board: usize) -> String {
        format!("csrf={}&board={}", self.csrf, self.boards[board])
    }

    async fn clear(&self, board: usize) -> (StatusCode, String) {
        self.request("/robot9000-cleanup", Some(self.form(board)))
            .await
    }

    async fn review(&self, board: usize) -> (StatusCode, String) {
        self.request(
            &format!("/robot9000-cleanup?board={}", self.boards[board]),
            None,
        )
        .await
    }

    async fn role(&self, role: &str, allow: &[String], deny: &[String], developer: bool) {
        sqlx::query("UPDATE staff_identity.accounts SET role=$2,allow_boards=$3,deny_boards=$4,flags=$5,public_capcode=NULL,revoked_at=NULL WHERE id=$1")
            .bind(self.account).bind(role).bind(allow).bind(deny)
            .bind(if developer {vec!["developer"]} else {vec![]})
            .execute(&self.owner).await.unwrap();
    }

    async fn seed(&self, board: usize, count: i32) {
        sqlx::query("INSERT INTO post_secrets.robot9000_texts(board,digest,seen_at) SELECT $1,decode(lpad(to_hex(n),64,'0'),'hex'),clock_timestamp()-interval '3 years' FROM generate_series(1,$2) n")
            .bind(&self.boards[board]).bind(count).execute(&self.owner).await.unwrap();
    }

    async fn snapshot(&self) -> String {
        sqlx::query_scalar("SELECT jsonb_build_object('texts',(SELECT coalesce(jsonb_agg(to_jsonb(t) ORDER BY board,digest),'[]') FROM post_secrets.robot9000_texts t WHERE board=ANY($1)),'mutes',(SELECT coalesce(jsonb_agg(to_jsonb(m) ORDER BY board,actor),'[]') FROM post_secrets.robot9000_mutes m WHERE board=ANY($1)))::text")
            .bind(self.boards.to_vec()).fetch_one(&self.owner).await.unwrap()
    }

    async fn audit_count(&self) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM content.board_cleanup_audit WHERE account_id=$1")
            .bind(self.account)
            .fetch_one(&self.owner)
            .await
            .unwrap()
    }

    async fn count(&self, board: usize) -> i64 {
        sqlx::query_scalar("SELECT count(*) FROM post_secrets.robot9000_texts WHERE board=$1")
            .bind(&self.boards[board])
            .fetch_one(&self.owner)
            .await
            .unwrap()
    }

    async fn cleanup(self) {
        let mut tx = self.owner.begin().await.unwrap();
        for statement in [
            "DELETE FROM content.board_cleanup_audit WHERE board=ANY($1)",
            "DELETE FROM post_secrets.robot9000_texts WHERE board=ANY($1)",
            "DELETE FROM post_secrets.robot9000_mutes WHERE board=ANY($1)",
            "DELETE FROM content.boards WHERE slug=ANY($1)",
        ] {
            sqlx::query(statement)
                .bind(self.boards.to_vec())
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        for statement in [
            "DELETE FROM staff_identity.sessions WHERE account_id=$1",
            "DELETE FROM staff_identity.credentials WHERE account_id=$1",
            "DELETE FROM staff_identity.accounts WHERE id=$1",
        ] {
            sqlx::query(statement)
                .bind(self.account)
                .execute(&mut *tx)
                .await
                .unwrap();
        }
        tx.commit().await.unwrap();
        self.auth.close().await;
        self.staff.close().await;
        self.owner.close().await;
    }
}

#[tokio::test]
async fn cleanup_is_bounded_board_scoped_and_preserves_recent_texts_and_mutes() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result = tokio::spawn({let f=f.clone();async move {
        f.seed(0,1001).await;
        f.seed(1,1).await;
        sqlx::query("INSERT INTO post_secrets.robot9000_texts(board,digest,seen_at) VALUES($1,$2,clock_timestamp()-interval '1 year')")
            .bind(&f.boards[0]).bind(auth::hash("retained recent text")).execute(&f.owner).await.unwrap();
        sqlx::query("INSERT INTO post_secrets.robot9000_mutes(board,actor,timeout_power,mute_until,next_expire) VALUES($1,$2,3,clock_timestamp()-interval '3 years',clock_timestamp()-interval '3 years')")
            .bind(&f.boards[0]).bind(auth::hash("retained expired mute")).execute(&f.owner).await.unwrap();
        let mutes:String=sqlx::query_scalar("SELECT to_jsonb(m)::text FROM post_secrets.robot9000_mutes m WHERE board=$1").bind(&f.boards[0]).fetch_one(&f.owner).await.unwrap();
        // Legacy history remains maintainable after Robot9000 is disabled.
        sqlx::query("UPDATE content.boards SET robot9000=false WHERE slug=$1").bind(&f.boards[0]).execute(&f.owner).await.unwrap();
        let before=f.snapshot().await;
        let review=f.review(0).await;
        assert_eq!(review.0,StatusCode::OK,"{}",review.1);
        assert_eq!(f.snapshot().await,before);
        assert_eq!(f.audit_count().await,0);
        let lower:chrono::DateTime<chrono::Utc>=sqlx::query_scalar("SELECT ((clock_timestamp() AT TIME ZONE 'UTC')-interval '2 years') AT TIME ZONE 'UTC'").fetch_one(&f.owner).await.unwrap();
        let first=f.clear(0).await;
        assert_eq!(first.0,StatusCode::OK,"{}",first.1);
        assert!(first.1.contains("Removed 1000"),"{}",first.1);
        assert!(first.1.contains("More eligible records remain"),"{}",first.1);
        assert_eq!(f.count(0).await,2);
        assert_eq!(f.count(1).await,1);
        let audit:(String,String,i64,chrono::DateTime<chrono::Utc>)=sqlx::query_as("SELECT board,action,removed,cutoff FROM content.board_cleanup_audit WHERE account_id=$1").bind(f.account).fetch_one(&f.owner).await.unwrap();
        assert_eq!((&audit.0,audit.1.as_str(),audit.2),(&f.boards[0],"robot9000-texts",1000));
        let upper:chrono::DateTime<chrono::Utc>=sqlx::query_scalar("SELECT ((clock_timestamp() AT TIME ZONE 'UTC')-interval '2 years') AT TIME ZONE 'UTC'").fetch_one(&f.owner).await.unwrap();
        assert!(audit.3>=lower && audit.3<=upper);
        let final_batch=f.clear(0).await;
        assert_eq!(final_batch.0,StatusCode::OK);
        assert!(final_batch.1.contains("No eligible records remain"));
        assert_eq!(f.count(0).await,1);
        assert_eq!(f.clear(0).await.0,StatusCode::OK);
        let counts:Vec<i64>=sqlx::query_scalar("SELECT removed FROM content.board_cleanup_audit WHERE account_id=$1 ORDER BY id").bind(f.account).fetch_all(&f.owner).await.unwrap();
        assert_eq!(counts,vec![1000,1,0]);
        let after:String=sqlx::query_scalar("SELECT to_jsonb(m)::text FROM post_secrets.robot9000_mutes m WHERE board=$1").bind(&f.boards[0]).fetch_one(&f.owner).await.unwrap();
        assert_eq!(mutes,after);
    }}).await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn authorization_matrix_requires_board_scope_moderator_floor_and_management_or_global_developer()
 {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result = tokio::spawn({
        let f = f.clone();
        async move {
            for (role, allow, deny, developer, allowed) in [
                ("janitor", vec!["all".into()], vec![], true, false),
                ("moderator", vec!["all".into()], vec![], false, false),
                ("moderator", vec![f.boards[0].clone()], vec![], true, false),
                (
                    "moderator",
                    vec!["all".into()],
                    vec!["noboard".into()],
                    true,
                    false,
                ),
                (
                    "moderator",
                    vec!["all".into()],
                    vec![f.boards[0].clone()],
                    true,
                    false,
                ),
                ("manager", vec![f.boards[1].clone()], vec![], false, false),
                (
                    "admin",
                    vec!["all".into()],
                    vec![f.boards[0].clone()],
                    true,
                    false,
                ),
                ("manager", vec![f.boards[0].clone()], vec![], false, true),
                ("moderator", vec!["all".into()], vec![], true, true),
                ("admin", vec![f.boards[0].clone()], vec![], false, true),
            ] {
                f.role(role, &allow, &deny, developer).await;
                let expected = if allowed {
                    StatusCode::OK
                } else {
                    StatusCode::FORBIDDEN
                };
                assert_eq!(
                    f.review(0).await.0,
                    expected,
                    "GET {role} {allow:?} {deny:?} developer={developer}"
                );
                assert_eq!(
                    f.clear(0).await.0,
                    expected,
                    "POST {role} {allow:?} {deny:?} developer={developer}"
                );
            }
            assert_eq!(f.audit_count().await, 3);
        }
    })
    .await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn exact_fields_origin_csrf_recent_auth_and_missing_boards_fail_without_mutation() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result=tokio::spawn({let f=f.clone();async move {
        f.seed(0,1).await;
        let before=f.snapshot().await;
        let oversized=format!("{}&cutoff={}",f.form(0),"x".repeat(4097));
        assert_eq!(f.request("/robot9000-cleanup",Some(oversized)).await.0,StatusCode::PAYLOAD_TOO_LARGE);
        for field in ["digest=00","actor=00","cutoff=2000-01-01","limit=100000","removed=1","csrf=duplicate","board=other"] {
            assert_eq!(f.request("/robot9000-cleanup",Some(format!("{}&{field}",f.form(0)))).await.0,StatusCode::BAD_REQUEST,"{field}");
        }
        for body in [format!("board={}",f.boards[0]),format!("csrf={}",f.csrf),format!("csrf={}&board=",f.csrf)] {
            assert_eq!(f.request("/robot9000-cleanup",Some(body)).await.0,StatusCode::BAD_REQUEST);
        }
        for query in ["", "?board=a&board=b", "?board=a&cutoff=old"] {
            assert_eq!(f.request(&format!("/robot9000-cleanup{query}"),None).await.0,StatusCode::BAD_REQUEST);
        }
        assert_eq!(f.request("/robot9000-cleanup",Some(f.form(0).replace(&f.csrf,&auth::token()))).await.0,StatusCode::FORBIDDEN);
        for origin in [None,Some("https://untrusted.invalid")] {
            let mut request=Request::builder().method(Method::POST).uri("/robot9000-cleanup").header("cookie",format!("staff={}; staff-csrf={}",f.token,f.csrf)).header("content-type","application/x-www-form-urlencoded").header("sec-fetch-site","same-origin");
            if let Some(origin)=origin {request=request.header("origin",origin);}
            assert_eq!(f.app.clone().oneshot(request.body(Body::from(f.form(0))).unwrap()).await.unwrap().status(),StatusCode::FORBIDDEN);
        }
        for method in [Method::GET,Method::POST] {
            let uri=if method==Method::GET {format!("/robot9000-cleanup?board={}",f.boards[0])} else {"/robot9000-cleanup".into()};
            assert_eq!(f.app.clone().oneshot(Request::builder().method(method).uri(uri).header("origin","http://localhost:3001").header("sec-fetch-site","same-origin").header("content-type","application/x-www-form-urlencoded").body(Body::from(f.form(0))).unwrap()).await.unwrap().status(),StatusCode::UNAUTHORIZED);
        }
        let missing=format!("zz{}",&uuid::Uuid::new_v4().simple().to_string()[..8]);
        assert_eq!(f.request(&format!("/robot9000-cleanup?board={missing}"),None).await.0,StatusCode::NOT_FOUND);
        assert_eq!(f.request("/robot9000-cleanup",Some(format!("csrf={}&board={missing}",f.csrf))).await.0,StatusCode::NOT_FOUND);
        sqlx::query("UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp()-interval '11 minutes' WHERE token_hash=$1").bind(auth::hash(&f.token)).execute(&f.owner).await.unwrap();
        assert_eq!(f.review(0).await.0,StatusCode::OK);
        assert_eq!(f.clear(0).await.0,StatusCode::FORBIDDEN);
        sqlx::query("UPDATE staff_identity.sessions SET expires_at=clock_timestamp()-interval '1 second' WHERE token_hash=$1").bind(auth::hash(&f.token)).execute(&f.owner).await.unwrap();
        assert_eq!(f.clear(0).await.0,StatusCode::UNAUTHORIZED);
        assert_eq!(f.snapshot().await,before);
        assert_eq!(f.audit_count().await,0);
    }}).await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn audit_insert_failure_rolls_back_deletions_and_allows_retry() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let function = format!("cleanup_fault_{}", f.boards[0]);
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE FUNCTION content.{function}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'owned cleanup audit failure'; END $$; CREATE TRIGGER {function} BEFORE INSERT ON content.board_cleanup_audit FOR EACH ROW WHEN (NEW.board='{}') EXECUTE FUNCTION content.{function}()",f.boards[0])))
        .execute(&f.owner).await.unwrap();
    let result = tokio::spawn({
        let f = f.clone();
        async move {
            f.seed(0, 2).await;
            let before = f.snapshot().await;
            assert_eq!(f.clear(0).await.0, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(f.snapshot().await, before);
            assert_eq!(f.audit_count().await, 0);
        }
    })
    .await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP TRIGGER {function} ON content.board_cleanup_audit; DROP FUNCTION content.{function}()"
    )))
    .execute(&f.owner)
    .await
    .unwrap();
    let retry = f.clear(0).await;
    let count = f.count(0).await;
    f.cleanup().await;
    result.unwrap();
    assert_eq!(retry.0, StatusCode::OK, "{}", retry.1);
    assert_eq!(count, 0);
}

async fn wait_for_content_lock(owner: &PgPool, blocker: i32) {
    tokio::time::timeout(Duration::from_secs(5),async {
        loop {
            let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE usename='board_staff' AND $1=ANY(pg_blocking_pids(pid)))")
                .bind(blocker).fetch_one(owner).await.unwrap();
            if waiting {break;}
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("cleanup must wait for the board lock");
}

#[tokio::test]
async fn board_lock_observes_refreshed_history_and_serializes_competing_batches() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result = tokio::spawn({
        let f = f.clone();
        async move {
            f.seed(0, 1).await;
            let mut posting = f.owner.begin().await.unwrap();
            let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut *posting)
                .await
                .unwrap();
            sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
                .bind(&f.boards[0])
                .execute(&mut *posting)
                .await
                .unwrap();
            let pending = tokio::spawn({
                let f = f.clone();
                async move { f.clear(0).await }
            });
            wait_for_content_lock(&f.owner, pid).await;
            // A duplicate detected before cleanup updates its timestamp while
            // holding the same board lock; cleanup must retain that digest.
            sqlx::query(
                "UPDATE post_secrets.robot9000_texts SET seen_at=clock_timestamp() WHERE board=$1",
            )
            .bind(&f.boards[0])
            .execute(&mut *posting)
            .await
            .unwrap();
            posting.commit().await.unwrap();
            assert_eq!(pending.await.unwrap().0, StatusCode::OK);
            assert_eq!(f.count(0).await, 1);
            f.seed(1, 1001).await;
            // Separate content connections bypass session-guard serialization.
            let mut blocker=f.owner.begin().await.unwrap();
            sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE").bind(&f.boards[1]).execute(&mut *blocker).await.unwrap();
            let mut pending_batches=Vec::new();
            let mut pids=Vec::new();
            for _ in 0..2 {
                let mut connection=f.staff.acquire().await.unwrap();
                let pid:i32=sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *connection).await.unwrap();
                pids.push(pid);
                let board=f.boards[1].clone();
                let account=f.account;
                pending_batches.push(tokio::spawn(async move {
                    sqlx::query_as::<_,(i64,bool)>("SELECT removed,has_more FROM content.cleanup_robot9000($1,$2)")
                        .bind(board).bind(account).fetch_one(&mut *connection).await.unwrap()
                }));
            }
            tokio::time::timeout(Duration::from_secs(5),async {
                loop {
                    let waiting:bool=sqlx::query_scalar("SELECT bool_and(cardinality(pg_blocking_pids(pid))>0) FROM unnest($1::integer[]) pid")
                        .bind(&pids).fetch_one(&f.owner).await.unwrap();
                    if waiting {break;}
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }).await.expect("both independent cleanup calls must contend for the board lock");
            blocker.commit().await.unwrap();
            let mut batches=Vec::new();
            for pending in pending_batches {batches.push(pending.await.unwrap());}
            batches.sort();
            assert_eq!(batches,vec![(1,false),(1000,true)]);
            assert_eq!(f.count(1).await, 0);
            let removed: Vec<i64> = sqlx::query_scalar(
                "SELECT removed FROM content.board_cleanup_audit WHERE account_id=$1 ORDER BY id",
            )
            .bind(f.account)
            .fetch_all(&f.owner)
            .await
            .unwrap();
            assert_eq!(removed, vec![0, 1000, 1]);
        }
    })
    .await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn cancelled_waiting_statement_keeps_history_and_audit_unchanged() {
    let _serial = TEST.lock().await;
    let staff = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .after_connect(|c, _| {
            Box::pin(async move {
                sqlx::query("SET lock_timeout='10s'")
                    .execute(&mut *c)
                    .await?;
                sqlx::query("SET statement_timeout='1500ms'")
                    .execute(c)
                    .await?;
                Ok(())
            })
        })
        .connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let f = Fixture::with_staff_pool(Some(staff)).await;
    let result = tokio::spawn({
        let f = f.clone();
        async move {
            f.seed(0, 1).await;
            let before = f.snapshot().await;
            let mut blocker = f.owner.begin().await.unwrap();
            let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut *blocker)
                .await
                .unwrap();
            sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
                .bind(&f.boards[0])
                .execute(&mut *blocker)
                .await
                .unwrap();
            let pending = tokio::spawn({
                let f = f.clone();
                async move { f.clear(0).await }
            });
            wait_for_content_lock(&f.owner, pid).await;
            let response = tokio::time::timeout(Duration::from_secs(5), pending)
                .await
                .unwrap()
                .unwrap();
            blocker.rollback().await.unwrap();
            assert_eq!(response.0, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(f.snapshot().await, before);
            assert_eq!(f.audit_count().await, 0);
            assert_eq!(f.clear(0).await.0, StatusCode::OK);
        }
    })
    .await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn authority_expiring_behind_board_lock_rolls_back_deletion_and_audit() {
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
            f.seed(0,1).await;
            let before=f.snapshot().await;
            let mut blocker=f.owner.begin().await.unwrap();
            let pid:i32=sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *blocker).await.unwrap();
            sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE").bind(&f.boards[0]).execute(&mut *blocker).await.unwrap();
            let statement=if recent {"UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp()-interval '10 minutes'+interval '2 seconds' WHERE token_hash=$1"} else {"UPDATE staff_identity.sessions SET expires_at=clock_timestamp()+interval '2 seconds' WHERE token_hash=$1"};
            sqlx::query(statement).bind(auth::hash(&f.token)).execute(&f.owner).await.unwrap();
            let pending=tokio::spawn({let f=f.clone();async move {f.clear(0).await}});
            wait_for_content_lock(&f.owner,pid).await;
            tokio::time::timeout(Duration::from_secs(5),async {
                loop {
                    let expired:bool=sqlx::query_scalar("SELECT CASE WHEN $2 THEN authenticated_at+interval '10 minutes'<=clock_timestamp() ELSE expires_at<=clock_timestamp() END FROM staff_identity.sessions WHERE token_hash=$1")
                        .bind(auth::hash(&f.token)).bind(recent).fetch_one(&f.owner).await.unwrap();
                    if expired {break;}
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }).await.unwrap();
            blocker.commit().await.unwrap();
            assert_eq!(pending.await.unwrap().0,if recent {StatusCode::FORBIDDEN} else {StatusCode::UNAUTHORIZED});
            assert_eq!(f.snapshot().await,before);
            assert_eq!(f.audit_count().await,0);
        }}).await;
        f.cleanup().await;
        result.unwrap();
    }
}

#[tokio::test]
async fn database_roles_cannot_bypass_private_storage_or_audit_boundaries() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result=tokio::spawn({let f=f.clone();async move {
        f.seed(0,1).await;
        for key in ["TEST_PUBLIC_DATABASE_URL","AUTH_DATABASE_URL"] {
            let pool=PgPool::connect(&std::env::var(key).unwrap()).await.unwrap();
            let error=sqlx::query("SELECT * FROM content.cleanup_robot9000($1,$2)").bind(&f.boards[0]).bind(f.account).execute(&pool).await.unwrap_err();
            assert_eq!(error.as_database_error().and_then(|e|e.code()).as_deref(),Some("42501"),"{key}: {error}");
            pool.close().await;
        }
        for statement in [
            "SELECT * FROM post_secrets.robot9000_texts LIMIT 0",
            "SELECT * FROM post_secrets.robot9000_mutes LIMIT 0",
            "DELETE FROM post_secrets.robot9000_texts WHERE false",
            "UPDATE content.board_cleanup_audit SET removed=0 WHERE false",
            "DELETE FROM content.board_cleanup_audit WHERE false",
            "INSERT INTO content.board_cleanup_audit(account_id,board,action,cutoff,removed) SELECT 1,'x','robot9000-texts',clock_timestamp(),0 WHERE false",
        ] {
            let error=sqlx::query(statement).execute(&f.staff).await.unwrap_err();
            assert_eq!(error.as_database_error().and_then(|e|e.code()).as_deref(),Some("42501"),"{statement}: {error}");
        }
        // The definer may append audit entries, never rewrite audit history.
        for statement in [
            "UPDATE content.board_cleanup_audit SET removed=0 WHERE false",
            "DELETE FROM content.board_cleanup_audit WHERE false",
        ] {
            let mut tx=f.owner.begin().await.unwrap();
            sqlx::query("SET LOCAL ROLE board_robot9000_owner").execute(&mut *tx).await.unwrap();
            let error=sqlx::query(statement).execute(&mut *tx).await.unwrap_err();
            assert_eq!(error.as_database_error().and_then(|e|e.code()).as_deref(),Some("42501"));
            tx.rollback().await.unwrap();
        }
        assert_eq!(f.count(0).await,1);
        assert_eq!(f.audit_count().await,0);
        // Session timezone must not select a different retention policy.
        let mut tx=f.staff.begin().await.unwrap();
        sqlx::query("SET LOCAL TIME ZONE 'Pacific/Kiritimati'").execute(&mut *tx).await.unwrap();
        let lower:chrono::DateTime<chrono::Utc>=sqlx::query_scalar("SELECT ((clock_timestamp() AT TIME ZONE 'UTC')-interval '2 years') AT TIME ZONE 'UTC'").fetch_one(&mut *tx).await.unwrap();
        let batch:(i64,bool,chrono::DateTime<chrono::Utc>)=sqlx::query_as("SELECT removed,has_more,cutoff FROM content.cleanup_robot9000($1,$2)").bind(&f.boards[0]).bind(f.account).fetch_one(&mut *tx).await.unwrap();
        let upper:chrono::DateTime<chrono::Utc>=sqlx::query_scalar("SELECT ((clock_timestamp() AT TIME ZONE 'UTC')-interval '2 years') AT TIME ZONE 'UTC'").fetch_one(&mut *tx).await.unwrap();
        assert_eq!((batch.0,batch.1),(1,false));
        assert!(batch.2>=lower && batch.2<=upper);
        tx.rollback().await.unwrap();
        assert_eq!(f.count(0).await,1);
        assert_eq!(f.audit_count().await,0);
    }}).await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn readiness_rejects_required_privilege_loss_and_excess_definer_authority() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result = tokio::spawn({
        let f = f.clone();
        async move {
            auth::check_identity(&f.staff, "board_staff").await.unwrap();
            for (drift, restore) in [
                (
                    "REVOKE DELETE ON post_secrets.robot9000_texts FROM board_robot9000_owner",
                    "GRANT DELETE ON post_secrets.robot9000_texts TO board_robot9000_owner",
                ),
                (
                    "REVOKE INSERT(cutoff) ON content.board_cleanup_audit FROM board_robot9000_owner",
                    "GRANT INSERT(cutoff) ON content.board_cleanup_audit TO board_robot9000_owner",
                ),
                (
                    "REVOKE USAGE ON SEQUENCE content.board_cleanup_audit_id_seq FROM board_robot9000_owner",
                    "GRANT USAGE ON SEQUENCE content.board_cleanup_audit_id_seq TO board_robot9000_owner",
                ),
                (
                    "GRANT UPDATE ON content.board_cleanup_audit TO board_robot9000_owner",
                    "REVOKE UPDATE ON content.board_cleanup_audit FROM board_robot9000_owner",
                ),
                (
                    "GRANT DELETE ON content.board_cleanup_audit TO board_robot9000_owner",
                    "REVOKE DELETE ON content.board_cleanup_audit FROM board_robot9000_owner",
                ),
                (
                    "GRANT DELETE ON post_secrets.robot9000_mutes TO board_robot9000_owner",
                    "REVOKE DELETE ON post_secrets.robot9000_mutes FROM board_robot9000_owner",
                ),
            ] {
                // Commit the drift so the independent readiness connection sees it.
                sqlx::query(drift).execute(&f.owner).await.unwrap();
                let checked = tokio::spawn({
                    let staff = f.staff.clone();
                    async move {
                        assert!(auth::check_identity(&staff, "board_staff").await.is_err(),
                            "readiness accepted privilege drift: {drift}");
                    }
                }).await;
                // Restore before propagating assertion failures or panics.
                sqlx::query(restore).execute(&f.owner).await.unwrap();
                checked.unwrap();
                auth::check_identity(&f.staff, "board_staff").await.unwrap();
            }
        }
    }).await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn utc_calendar_expression_handles_leap_days_timezones_and_strict_cutoff_edges() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result = tokio::spawn({
        let f = f.clone();
        async move {
            // This is deterministic coverage of the calendar expression and
            // comparison boundary, not an injected clock in the live function.
            // Live-function cutoff tests above bracket its real database clock.
            let definition: String = sqlx::query_scalar(
                "SELECT pg_get_functiondef('content.cleanup_robot9000(text,bigint)'::regprocedure)",
            )
            .fetch_one(&f.owner)
            .await
            .unwrap();
            let compact: String = definition.chars().filter(|c| !c.is_whitespace()).collect();
            assert!(compact.contains("v_cutoff:=((clock_timestamp()ATTIMEZONE'UTC')-interval'2years')ATTIMEZONE'UTC';"));
            assert!(compact.contains("old.seen_at<v_cutoff"));
            for zone in [
                "SET LOCAL TIME ZONE 'UTC'",
                "SET LOCAL TIME ZONE 'America/New_York'",
                "SET LOCAL TIME ZONE 'Pacific/Kiritimati'",
            ] {
                let mut tx = f.staff.begin().await.unwrap();
                sqlx::query(zone).execute(&mut *tx).await.unwrap();
                for (now, expected) in [
                    ("2024-02-29T12:34:56Z", "2022-02-28T12:34:56Z"),
                    ("2026-03-01T00:00:00Z", "2024-03-01T00:00:00Z"),
                    ("2024-03-01T00:00:00Z", "2022-03-01T00:00:00Z"),
                    // Local leap-day labels cannot change the UTC calendar date.
                    ("2024-03-01T00:30:00+14:00", "2022-02-28T10:30:00Z"),
                    ("2024-02-29T23:30:00-05:00", "2022-03-01T04:30:00Z"),
                    // DST offsets at these instants differ in the target year.
                    ("2026-03-09T12:00:00-04:00", "2024-03-09T16:00:00Z"),
                ] {
                    let now = chrono::DateTime::parse_from_rfc3339(now).unwrap().with_timezone(&chrono::Utc);
                    let expected = chrono::DateTime::parse_from_rfc3339(expected).unwrap().with_timezone(&chrono::Utc);
                    let actual: (chrono::DateTime<chrono::Utc>, bool, bool, bool) = sqlx::query_as(
                        "WITH boundary AS (SELECT (($1::timestamptz AT TIME ZONE 'UTC') - interval '2 years') AT TIME ZONE 'UTC' AS cutoff) SELECT cutoff,cutoff-interval '1 microsecond'<cutoff,cutoff<cutoff,cutoff+interval '1 microsecond'<cutoff FROM boundary",
                    )
                    .bind(now)
                    .fetch_one(&mut *tx)
                    .await
                    .unwrap();
                    assert_eq!(actual, (expected, true, false, false), "{zone}; input={now}");
                }
                tx.rollback().await.unwrap();
            }
        }
    }).await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn scoped_manager_cleans_retained_private_board_history_without_public_exposure() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result = tokio::spawn({
        let f = f.clone();
        async move {
            f.seed(0, 2).await;
            f.seed(1, 1).await;
            sqlx::query("UPDATE content.boards SET staff_only=true WHERE slug=$1")
                .bind(&f.boards[0]).execute(&f.owner).await.unwrap();
            let board_before: String = sqlx::query_scalar("SELECT to_jsonb(b)::text FROM content.boards b WHERE slug=$1")
                .bind(&f.boards[0]).fetch_one(&f.owner).await.unwrap();
            let public = PgPool::connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap()).await.unwrap();
            f.role("manager", &[f.boards[1].clone()], &[], false).await;
            assert_eq!(f.review(0).await.0, StatusCode::FORBIDDEN);
            assert_eq!(f.clear(0).await.0, StatusCode::FORBIDDEN);
            f.role("manager", &[f.boards[0].clone()], &[], false).await;
            for after_cleanup in [false, true] {
                if after_cleanup {
                    assert_eq!(f.review(0).await.0, StatusCode::OK);
                    assert_eq!(f.count(0).await, 2);
                    let response = f.clear(0).await;
                    assert_eq!(response.0, StatusCode::OK, "{}", response.1);
                    assert!(response.1.contains("Removed 2"));
                }
                let visible: i64 = sqlx::query_scalar("SELECT count(*) FROM content.boards WHERE slug=$1")
                    .bind(&f.boards[0]).fetch_one(&public).await.unwrap();
                assert_eq!(visible, 0, "private board must remain hidden from public database role");
                let error = sqlx::query("SELECT * FROM content.check_robot9000($1,$2,$3,NULL,date_trunc('second',clock_timestamp()))")
                    .bind(&f.boards[0]).bind(auth::hash("private actor")).bind(auth::hash("private posting text"))
                    .execute(&public).await.unwrap_err();
                assert_eq!(error.as_database_error().and_then(|e| e.code()).as_deref(), Some("23514"),
                    "public posting admission must still reject the private board");
            }
            public.close().await;
            let board_after: String = sqlx::query_scalar("SELECT to_jsonb(b)::text FROM content.boards b WHERE slug=$1")
                .bind(&f.boards[0]).fetch_one(&f.owner).await.unwrap();
            assert_eq!(board_after, board_before);
            assert_eq!(f.count(0).await, 0);
            assert_eq!(f.count(1).await, 1);
            assert_eq!(f.audit_count().await, 1);
        }
    }).await;
    f.cleanup().await;
    result.unwrap();
}
