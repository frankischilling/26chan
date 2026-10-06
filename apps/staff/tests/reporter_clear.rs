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
}

impl Fixture {
    async fn new() -> Self {
        Self::with_staff_pool(None).await
    }

    async fn with_staff_pool(staff_override: Option<PgPool>) -> Self {
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
            [0, 1].map(|_| format!("rc{}", &uuid::Uuid::new_v4().simple().to_string()[..8]));
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
        Self {
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
    fn form(&self, board: usize, report: i64) -> String {
        format!(
            "csrf={}&board={}&report_id={report}",
            self.csrf, self.boards[board]
        )
    }

    async fn clear(&self, board: usize, report: i64) -> (StatusCode, String) {
        self.request("/reporter-clear", Some(self.form(board, report)))
            .await
    }

    async fn report(
        &self,
        board: usize,
        state: &str,
        actor: u8,
        automatic: Option<&str>,
        member: bool,
    ) -> i64 {
        let mut tx = self.owner.begin().await.unwrap();
        sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
            .bind(&self.boards[board])
            .execute(&mut *tx)
            .await
            .unwrap();
        let id: i64 = sqlx::query_scalar("INSERT INTO content.reports(board,post_id,reason,state) VALUES($1,$2,$3,$4) RETURNING id")
            .bind(&self.boards[board]).bind(self.posts[board])
            .bind(format!("Owned report {} {actor}", self.boards[board])).bind(state)
            .fetch_one(&mut *tx).await.unwrap();
        sqlx::query("SET LOCAL ROLE board_report_admission_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        if member {
            sqlx::query("INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at,automatic_identity) VALUES($1,$2,$3,$4,$4,clock_timestamp(),$5::text::uuid)")
                .bind(id).bind(auth::hash(&format!("{}:{actor}", self.token))).bind(&self.boards[board]).bind(self.posts[board]).bind(automatic)
                .execute(&mut *tx).await.unwrap();
        }
        sqlx::query("INSERT INTO post_secrets.report_weight_evidence(report_id,evaluator_version,known_or_verified,effective_weight,numeric_proof,evaluated_at) VALUES($1,1,true,0.5,'BaseEqualsFallback',clock_timestamp())")
            .bind(id).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        id
    }

    async fn history(&self) -> String {
        let mut tx = self.owner.begin().await.unwrap();
        let reports: String = sqlx::query_scalar("SELECT coalesce(jsonb_agg(to_jsonb(r)-'reporter_cleared_at' ORDER BY id),'[]')::text FROM content.reports r WHERE board=ANY($1)")
            .bind(self.boards.to_vec()).fetch_one(&mut *tx).await.unwrap();
        let ids: Vec<i64> =
            sqlx::query_scalar("SELECT id FROM content.reports WHERE board=ANY($1)")
                .bind(self.boards.to_vec())
                .fetch_all(&mut *tx)
                .await
                .unwrap();
        sqlx::query("SET LOCAL ROLE board_report_admission_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        let evidence: String = sqlx::query_scalar("SELECT coalesce(jsonb_agg(to_jsonb(e) ORDER BY report_id),'[]')::text FROM post_secrets.report_weight_evidence e WHERE report_id=ANY($1)")
            .bind(ids).fetch_one(&mut *tx).await.unwrap();
        sqlx::query("SET LOCAL ROLE board_anonymous_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        let sessions: String = sqlx::query_scalar("SELECT jsonb_build_object('session',(SELECT to_jsonb(s) FROM post_secrets.anonymous_sessions s WHERE token_hash=$1),'reports',(SELECT coalesce(jsonb_agg(to_jsonb(r) ORDER BY report_id),'[]') FROM post_secrets.anonymous_reports r WHERE token_hash=$1))::text")
            .bind(auth::hash(&self.token)).fetch_one(&mut *tx).await.unwrap();
        tx.rollback().await.unwrap();
        format!("{reports}\n{evidence}\n{sessions}")
    }

    async fn marked(&self) -> Vec<i64> {
        sqlx::query_scalar("SELECT id FROM content.reports WHERE board=ANY($1) AND reporter_cleared_at IS NOT NULL ORDER BY id")
            .bind(self.boards.to_vec()).fetch_all(&self.owner).await.unwrap()
    }

    async fn membership(&self) -> Vec<i64> {
        let mut tx = self.owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_report_admission_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        let ids = sqlx::query_scalar("SELECT report_id FROM post_secrets.report_membership WHERE board=ANY($1) ORDER BY report_id")
            .bind(self.boards.to_vec()).fetch_all(&mut *tx).await.unwrap();
        tx.rollback().await.unwrap();
        ids
    }

    async fn group(&self, board: usize) -> Option<(i64, bool)> {
        let mut tx = self.owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_report_admission_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        let result = sqlx::query_as("SELECT illegal_count,incomplete FROM post_secrets.report_group WHERE board=$1 AND post_id=$2")
            .bind(&self.boards[board]).bind(self.posts[board]).fetch_optional(&mut *tx).await.unwrap();
        tx.rollback().await.unwrap();
        result
    }
}

#[tokio::test]
async fn clear_matches_flat_identity_union_across_boards_without_rewriting_history() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result = tokio::spawn({ let f = f.clone(); async move {
        let automatic_a = uuid::Uuid::new_v4().to_string();
        let automatic_b = uuid::Uuid::new_v4().to_string();
        sqlx::query("UPDATE content.boards SET staff_only=true,can_report_posts=false WHERE slug=$1")
            .bind(&f.boards[1]).execute(&f.owner).await.unwrap();
        let seed = f.report(0,"open",1,Some(&automatic_a),true).await;
        let same_actor = f.report(1,"resolved",1,Some(&automatic_b),true).await;
        let same_automatic = f.report(1,"dismissed",2,Some(&automatic_a),true).await;
        let transitive_actor = f.report(0,"open",2,None,true).await;
        let transitive_automatic = f.report(1,"open",3,Some(&automatic_b),true).await;
        let null_identity = f.report(0,"open",4,None,true).await;
        let historical = f.report(0,"open",1,None,false).await;
        let mut age = f.owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_report_admission_owner").execute(&mut *age).await.unwrap();
        sqlx::query("UPDATE post_secrets.report_membership SET reported_at=clock_timestamp()-interval '400 days' WHERE report_id=$1")
            .bind(same_actor).execute(&mut *age).await.unwrap();
        age.commit().await.unwrap();
        let mut session = f.owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_anonymous_owner").execute(&mut *session).await.unwrap();
        sqlx::query("INSERT INTO post_secrets.anonymous_sessions(token_hash,network_hash,address_hash,environment_hash,created_at,network_at,address_at,environment_at,expires_at,reports,automatic_identity) VALUES($1,$1,$1,$1,1,1,1,1,2,7,$2::text::uuid)")
            .bind(auth::hash(&f.token)).bind(&automatic_a).execute(&mut *session).await.unwrap();
        sqlx::query("INSERT INTO post_secrets.anonymous_reports(report_id,token_hash) VALUES($1,$2)")
            .bind(seed).bind(auth::hash(&f.token)).execute(&mut *session).await.unwrap();
        session.commit().await.unwrap();
        let before = f.history().await;
        let (status, html) = f.clear(0,seed).await;
        assert_eq!(status,StatusCode::OK,"{html}");
        assert!(html.contains("<h1>Cleared 3 reports</h1>"),"{html}");
        assert_eq!(f.marked().await,vec![seed,same_actor,same_automatic]);
        assert_eq!(f.membership().await,vec![transitive_actor,transitive_automatic,null_identity]);
        assert_eq!(f.history().await,before);
        assert_eq!(f.group(0).await,Some((0,true)));
        assert_eq!(f.group(1).await,Some((0,true)));
        let audit: String = sqlx::query_scalar("SELECT to_jsonb(a)::text FROM content.moderation_audit a WHERE account_id=$1")
            .bind(f.account).fetch_one(&f.owner).await.unwrap();
        let audit: serde_json::Value = serde_json::from_str(&audit).unwrap();
        assert_eq!(audit["action"],"reporter-clear");
        assert_eq!(audit["target_id"],seed);
        assert_eq!(audit["reporter_clear_count"],3);
        for (key,value) in audit.as_object().unwrap() {
            if key.starts_with("snapshot_") || key.ends_with("_mask") {
                assert!(value.is_null(),"unexpected audit field {key}: {value}");
            }
        }
        assert_eq!(f.audit_count().await,1);
        let marked_before: String = sqlx::query_scalar("SELECT jsonb_agg(to_jsonb(r) ORDER BY id)::text FROM content.reports r WHERE board=ANY($1)")
            .bind(f.boards.to_vec()).fetch_one(&f.owner).await.unwrap();
        assert_eq!(f.clear(0,seed).await.0,StatusCode::NOT_FOUND);
        assert_eq!(f.clear(0,historical).await.0,StatusCode::NOT_FOUND);
        let marked_after: String = sqlx::query_scalar("SELECT jsonb_agg(to_jsonb(r) ORDER BY id)::text FROM content.reports r WHERE board=ANY($1)")
            .bind(f.boards.to_vec()).fetch_one(&f.owner).await.unwrap();
        assert_eq!(marked_after,marked_before);
        assert_eq!(f.audit_count().await,1);
        assert_eq!(f.history().await,before);
        let queue = f.request("/reports",None).await;
        assert_eq!(queue.0,StatusCode::OK);
        for secret in [&automatic_a,&automatic_b,&f.token] {
            assert!(!queue.1.contains(secret));
            assert!(!html.contains(secret));
        }
        for id in [seed,same_actor,same_automatic] {
            assert!(!queue.1.contains(&format!("id=\"report-{id}\"")));
            assert!(!queue.1.contains(&format!("name=\"report_id\" value=\"{id}\"")));
            for action in ["resolve","dismiss"] {
                assert_eq!(f.request("/moderate",Some(format!("csrf={}&board={}&target={id}&action={action}",f.csrf,if id==seed {&f.boards[0]} else {&f.boards[1]}))).await.0,StatusCode::NOT_FOUND);
            }
        }
        assert!(queue.1.contains(&format!("name=\"report_id\" value=\"{transitive_actor}\"")));
        assert_eq!(f.history().await,before);
        assert_eq!(f.audit_count().await,1);
        // A NULL seed identity only matches its actor, not other NULL rows.
        assert_eq!(f.clear(0,transitive_actor).await.0,StatusCode::OK);
        assert_eq!(f.membership().await,vec![transitive_automatic,null_identity]);
        assert_eq!(f.clear(0,null_identity).await.0,StatusCode::OK);
        assert_eq!(f.group(0).await,None);
        assert_eq!(f.clear(1,transitive_automatic).await.0,StatusCode::OK);
        assert_eq!(f.group(1).await,None);
    }}).await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn authorization_and_exact_input_shape_reject_without_side_effects() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result = tokio::spawn({let f=f.clone();async move {
        let seed = f.report(0,"open",11,None,true).await;
        let before=f.history().await;
        for (role,allow,deny) in [
            ("janitor",vec!["all".to_owned()],vec![]),
            ("moderator",f.boards.to_vec(),vec![]),
            ("moderator",vec!["all".to_owned()],vec![f.boards[1].clone()]),
        ] {
            f.role(role,&allow,&deny).await;
            assert_eq!(f.clear(0,seed).await.0,StatusCode::FORBIDDEN);
        }
        f.role("moderator",&["all".to_owned()],&[]).await;
        for field in ["actor_hash=00","automatic_identity=00000000-0000-0000-0000-000000000000","ip=127.0.0.1","csrf=duplicate","report_id=1","board=other"] {
            assert_eq!(f.request("/reporter-clear",Some(format!("{}&{field}",f.form(0,seed)))).await.0,StatusCode::BAD_REQUEST,"{field}");
        }
        for body in [format!("board={}&report_id={seed}",f.boards[0]),format!("csrf={}&board={}",f.csrf,f.boards[0])] {
            assert_eq!(f.request("/reporter-clear",Some(body)).await.0,StatusCode::BAD_REQUEST);
        }
        assert_eq!(f.request("/reporter-clear",Some(f.form(0,seed).replace(&f.csrf,&auth::token()))).await.0,StatusCode::FORBIDDEN);
        for origin in [None,Some("https://untrusted.invalid")] {
            let mut request=Request::builder().method(Method::POST).uri("/reporter-clear")
                .header("cookie",format!("staff={}; staff-csrf={}",f.token,f.csrf))
                .header("content-type","application/x-www-form-urlencoded").header("sec-fetch-site","same-origin");
            if let Some(origin)=origin { request=request.header("origin",origin); }
            let response=f.app.clone().oneshot(request.body(Body::from(f.form(0,seed))).unwrap()).await.unwrap();
            assert_eq!(response.status(),StatusCode::FORBIDDEN);
        }
        assert_eq!(f.clear(1,seed).await.0,StatusCode::NOT_FOUND);
        assert_eq!(f.clear(0,i64::MAX).await.0,StatusCode::NOT_FOUND);
        sqlx::query("UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp()-interval '11 minutes' WHERE token_hash=$1")
            .bind(auth::hash(&f.token)).execute(&f.owner).await.unwrap();
        assert_eq!(f.clear(0,seed).await.0,StatusCode::FORBIDDEN);
        sqlx::query("UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp(),expires_at=clock_timestamp()-interval '1 second' WHERE token_hash=$1")
            .bind(auth::hash(&f.token)).execute(&f.owner).await.unwrap();
        assert_eq!(f.clear(0,seed).await.0,StatusCode::UNAUTHORIZED);
        sqlx::query("UPDATE staff_identity.sessions SET expires_at=clock_timestamp()+interval '1 hour' WHERE token_hash=$1")
            .bind(auth::hash(&f.token)).execute(&f.owner).await.unwrap();
        sqlx::query("UPDATE staff_identity.accounts SET revoked_at=clock_timestamp() WHERE id=$1")
            .bind(f.account).execute(&f.owner).await.unwrap();
        assert_eq!(f.clear(0,seed).await.0,StatusCode::UNAUTHORIZED);
        assert_eq!(f.history().await,before);
        assert_eq!(f.membership().await,vec![seed]);
        assert!(f.marked().await.is_empty());
        assert_eq!(f.audit_count().await,0);
    }}).await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn audit_failure_rolls_back_markers_memberships_and_groups() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let seed = f.report(0, "open", 31, None, true).await;
    f.report(1, "resolved", 31, None, true).await;
    let function = format!("reporter_clear_fault_{}", f.boards[0]);
    assert!(f.boards[0][2..].bytes().all(|b| b.is_ascii_hexdigit()));
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE FUNCTION content.{function}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'owned reporter clear audit failure'; END $$"))).execute(&f.owner).await.unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE TRIGGER {function} BEFORE INSERT ON content.moderation_audit FOR EACH ROW WHEN (NEW.board='{}' AND NEW.action='reporter-clear') EXECUTE FUNCTION content.{function}()",f.boards[0]))).execute(&f.owner).await.unwrap();
    let result = tokio::spawn({
        let f = f.clone();
        async move {
            let history = f.history().await;
            let members = f.membership().await;
            assert_eq!(f.clear(0, seed).await.0, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(f.history().await, history);
            assert_eq!(f.membership().await, members);
            assert_eq!(f.group(0).await, Some((0, true)));
            assert_eq!(f.group(1).await, Some((0, true)));
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
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn runtime_roles_cannot_read_identities_or_write_clear_markers() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result = tokio::spawn({
        let f = f.clone();
        async move {
            for variable in ["TEST_PUBLIC_DATABASE_URL", "AUTH_DATABASE_URL"] {
                let pool = PgPool::connect(&std::env::var(variable).unwrap())
                    .await
                    .unwrap();
                let error = sqlx::query("SELECT content.clear_reporter($1,$2)")
                    .bind(&f.boards[0])
                    .bind(1_i64)
                    .execute(&pool)
                    .await
                    .unwrap_err();
                assert_eq!(
                    error.as_database_error().unwrap().code().as_deref(),
                    Some("42501")
                );
                pool.close().await;
            }
            for sql in [
                "SELECT actor_hash,automatic_identity FROM post_secrets.report_membership LIMIT 0",
                "SELECT * FROM post_secrets.report_weight_evidence LIMIT 0",
                "UPDATE content.reports SET reporter_cleared_at=clock_timestamp() WHERE false",
            ] {
                let error = sqlx::query(sql).execute(&f.staff).await.unwrap_err();
                assert_eq!(
                    error.as_database_error().unwrap().code().as_deref(),
                    Some("42501"),
                    "{sql}"
                );
            }
        }
    })
    .await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn partial_clear_preserves_illegal_count_and_taint_then_empty_group_resets() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let catalog = serde_json::json!({"version":1,"categories":[
        {"id":31,"board":f.boards[0],"op_only":false,"reply_only":false,"image_only":false,"exclude_boards":null,"title":"Owned clear illegal","weight":0.5,"filtered":0}
    ]});
    let revision: i64 = sqlx::query_scalar("SELECT content.import_report_catalog($1::text::jsonb)")
        .bind(catalog.to_string())
        .fetch_one(&f.owner)
        .await
        .unwrap();
    let result=tokio::spawn({let f=f.clone();async move {
        let survivor=f.report(0,"open",41,None,true).await;
        let mut tx=f.owner.begin().await.unwrap();
        sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
            .bind(&f.boards[0]).execute(&mut *tx).await.unwrap();
        let mut ids=Vec::new();
        for _ in 0..3 {
            let id:i64=sqlx::query_scalar("INSERT INTO content.reports(board,post_id,reason,category_revision,category_id,category_kind,category_base_weight) VALUES($1,$2,'Owned categorical clear',$3,31,2,0.5) RETURNING id")
                .bind(&f.boards[0]).bind(f.posts[0]).bind(revision).fetch_one(&mut *tx).await.unwrap();
            ids.push(id);
        }
        sqlx::query("SET LOCAL ROLE board_report_admission_owner").execute(&mut *tx).await.unwrap();
        sqlx::query("INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at) SELECT id,$2,$3,$4,$4,clock_timestamp() FROM unnest($1::bigint[]) id")
            .bind(&ids).bind(auth::hash(&format!("{}:42",f.token))).bind(&f.boards[0]).bind(f.posts[0]).execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        let history=f.history().await;
        assert_eq!(f.group(0).await,Some((3,true)));
        assert_eq!(f.clear(0,ids[0]).await.0,StatusCode::OK);
        assert_eq!(f.group(0).await,Some((3,true)));
        assert_eq!(f.membership().await,vec![survivor]);
        assert_eq!(f.history().await,history);
        assert_eq!(f.clear(0,survivor).await.0,StatusCode::OK);
        assert_eq!(f.group(0).await,None);
        let fresh=f.report(0,"open",43,None,true).await;
        assert_eq!(f.group(0).await,Some((0,true)));
        assert_eq!(f.membership().await,vec![fresh]);
    }}).await;
    let owner = f.owner.clone();
    // Delete owned reports first so the inactive catalog has no references.
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SELECT slug FROM content.boards WHERE slug=ANY($1) ORDER BY slug FOR UPDATE")
        .bind(f.boards.to_vec())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.reports WHERE board=ANY($1)")
        .bind(f.boards.to_vec())
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("SET LOCAL ROLE board_report_admission_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM post_secrets.report_catalog_rows WHERE revision=$1")
        .bind(revision)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("DELETE FROM post_secrets.report_catalog_versions WHERE revision=$1")
        .bind(revision)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    f.cleanup().await;
    result.unwrap();
}

async fn wait_for_content_lock(owner: &PgPool, blocker: i32) {
    tokio::time::timeout(Duration::from_secs(1),async {
        loop {
            let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE usename='board_staff' AND $1=ANY(pg_blocking_pids(pid)))")
                .bind(blocker).fetch_one(owner).await.unwrap();
            if waiting { break; }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }).await.expect("clear must wait on the held board before admission gate");
}

#[tokio::test]
async fn archive_and_deletion_can_retire_the_seed_while_clear_waits_for_board() {
    let _serial = TEST.lock().await;
    for archive in [true, false] {
        let f = Fixture::new().await;
        let result=tokio::spawn({let f=f.clone();async move {
            // A known ordinary group is retired at archive; no unknown taint.
            let seed=f.report(0,"open",51,None,true).await;
            let mut retire=f.owner.begin().await.unwrap();
            let pid:i32=sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *retire).await.unwrap();
            sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
                .bind(&f.boards[0]).execute(&mut *retire).await.unwrap();
            let pending=tokio::spawn({let f=f.clone();async move {f.clear(0,seed).await}});
            wait_for_content_lock(&f.owner,pid).await;
            // Only this owned fixture is changed to an already-known group.
            sqlx::query("SET LOCAL ROLE board_report_admission_owner").execute(&mut *retire).await.unwrap();
            sqlx::query("UPDATE post_secrets.report_group SET incomplete=false WHERE board=$1")
                .bind(&f.boards[0]).execute(&mut *retire).await.unwrap();
            sqlx::query("RESET ROLE").execute(&mut *retire).await.unwrap();
            if archive {
                sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
                    .bind(f.posts[0]).execute(&mut *retire).await.unwrap();
            } else {
                sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1")
                    .bind(f.posts[0]).execute(&mut *retire).await.unwrap();
            }
            retire.commit().await.unwrap();
            assert_eq!(pending.await.unwrap().0,StatusCode::NOT_FOUND);
            assert!(f.membership().await.is_empty());
            assert!(f.marked().await.is_empty(),"ordinary retirement must not become a clear marker");
            assert_eq!(f.audit_count().await,0);
        }}).await;
        f.cleanup().await;
        result.unwrap();
    }
}

#[tokio::test]
async fn admission_committed_ahead_of_clear_is_included_in_fresh_membership_scan() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result=tokio::spawn({let f=f.clone();async move {
        let seed=f.report(0,"open",61,None,true).await;
        let mut admission=f.owner.begin().await.unwrap();
        let pid:i32=sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *admission).await.unwrap();
        sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
            .bind(&f.boards[1]).execute(&mut *admission).await.unwrap();
        sqlx::query("SET LOCAL ROLE board_report_admission_owner").execute(&mut *admission).await.unwrap();
        sqlx::query("SELECT singleton FROM post_secrets.report_admission_gate WHERE singleton FOR UPDATE")
            .execute(&mut *admission).await.unwrap();
        let pending=tokio::spawn({let f=f.clone();async move {f.clear(0,seed).await}});
        wait_for_content_lock(&f.owner,pid).await;
        let report:i64=sqlx::query_scalar("INSERT INTO content.reports(board,post_id,reason) VALUES($1,$2,'Owned concurrent admission') RETURNING id")
            .bind(&f.boards[1]).bind(f.posts[1]).fetch_one(&mut *admission).await.unwrap();
        sqlx::query("INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at) VALUES($1,$2,$3,$4,$4,clock_timestamp())")
            .bind(report).bind(auth::hash(&format!("{}:61",f.token))).bind(&f.boards[1]).bind(f.posts[1]).execute(&mut *admission).await.unwrap();
        admission.commit().await.unwrap();
        let (status,html)=pending.await.unwrap();
        assert_eq!(status,StatusCode::OK,"{html}");
        assert!(html.contains("<h1>Cleared 2 reports</h1>"));
        assert_eq!(f.marked().await,vec![seed,report]);
        assert!(f.membership().await.is_empty());
        assert_eq!(f.audit_count().await,1);
    }}).await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn authority_expiring_during_content_lock_wait_rolls_back_clear_and_audit() {
    let _serial = TEST.lock().await;
    for recent in [false, true] {
        // This pool alone permits the real clock boundary to pass behind a lock.
        let staff = sqlx::postgres::PgPoolOptions::new()
            .max_connections(2)
            .after_connect(|connection, _| {
                Box::pin(async move {
                    sqlx::query("SET lock_timeout='5s'")
                        .execute(&mut *connection)
                        .await?;
                    sqlx::query("SET statement_timeout='8s'")
                        .execute(connection)
                        .await?;
                    Ok(())
                })
            })
            .connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let f = Fixture::with_staff_pool(Some(staff)).await;
        let result=tokio::spawn({let f=f.clone();async move {
            let seed=f.report(0,"open",71,None,true).await;
            let history=f.history().await;
            let mut blocker=f.owner.begin().await.unwrap();
            let pid:i32=sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *blocker).await.unwrap();
            sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
                .bind(&f.boards[0]).execute(&mut *blocker).await.unwrap();
            let sql=if recent {
                "UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp()-interval '10 minutes'+interval '2 seconds' WHERE token_hash=$1"
            } else {
                "UPDATE staff_identity.sessions SET expires_at=clock_timestamp()+interval '2 seconds' WHERE token_hash=$1"
            };
            sqlx::query(sql).bind(auth::hash(&f.token)).execute(&f.owner).await.unwrap();
            let pending=tokio::spawn({let f=f.clone();async move {f.clear(0,seed).await}});
            wait_for_content_lock(&f.owner,pid).await;
            let mut attempt=f.owner.begin().await.unwrap();
            let error=sqlx::query("SELECT id FROM staff_identity.accounts WHERE id=$1 FOR UPDATE NOWAIT")
                .bind(f.account).execute(&mut *attempt).await.unwrap_err();
            assert_eq!(error.as_database_error().unwrap().code().as_deref(),Some("55P03"));
            attempt.rollback().await.unwrap();
            tokio::time::sleep(Duration::from_millis(2100)).await;
            blocker.commit().await.unwrap();
            assert_eq!(pending.await.unwrap().0,if recent {StatusCode::FORBIDDEN} else {StatusCode::UNAUTHORIZED});
            assert_eq!(f.membership().await,vec![seed]);
            assert!(f.marked().await.is_empty());
            assert_eq!(f.history().await,history);
            assert_eq!(f.audit_count().await,0);
        }}).await;
        f.cleanup().await;
        result.unwrap();
    }
}

#[tokio::test]
async fn newly_committed_matching_board_aborts_then_fresh_request_clears_safely() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result=tokio::spawn({let f=f.clone();async move {
        let seed=f.report(0,"open",81,None,true).await;
        // Keep the second owned board absent from the clear's first board scan.
        for sql in [
            "DELETE FROM content.posts WHERE board=$1",
            "DELETE FROM content.threads WHERE board=$1",
            "DELETE FROM content.boards WHERE slug=$1",
        ] {
            sqlx::query(sql).bind(&f.boards[1]).execute(&f.owner).await.unwrap();
        }
        let mut publisher=f.owner.begin().await.unwrap();
        let pid:i32=sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *publisher).await.unwrap();
        sqlx::query("SET LOCAL ROLE board_report_admission_owner").execute(&mut *publisher).await.unwrap();
        sqlx::query("SELECT singleton FROM post_secrets.report_admission_gate WHERE singleton FOR UPDATE")
            .execute(&mut *publisher).await.unwrap();
        let pending=tokio::spawn({let f=f.clone();async move {f.clear(0,seed).await}});
        wait_for_content_lock(&f.owner,pid).await;
        sqlx::query("RESET ROLE").execute(&mut *publisher).await.unwrap();
        sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Owned newly published board','',1000,100,100,100,10)")
            .bind(&f.boards[1]).execute(&mut *publisher).await.unwrap();
        sqlx::query("INSERT INTO content.threads(id,board) VALUES($1,$2)")
            .bind(f.posts[1]).bind(&f.boards[1]).execute(&mut *publisher).await.unwrap();
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','','Owned new board post')")
            .bind(f.posts[1]).bind(&f.boards[1]).execute(&mut *publisher).await.unwrap();
        let report:i64=sqlx::query_scalar("INSERT INTO content.reports(board,post_id,reason) VALUES($1,$2,'Owned new board admission') RETURNING id")
            .bind(&f.boards[1]).bind(f.posts[1]).fetch_one(&mut *publisher).await.unwrap();
        // This newly inserted row is already exclusively owned by this transaction.
        sqlx::query("SET LOCAL ROLE board_report_admission_owner").execute(&mut *publisher).await.unwrap();
        sqlx::query("INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at) VALUES($1,$2,$3,$4,$4,clock_timestamp())")
            .bind(report).bind(auth::hash(&format!("{}:81",f.token))).bind(&f.boards[1]).bind(f.posts[1]).execute(&mut *publisher).await.unwrap();
        publisher.commit().await.unwrap();
        assert_eq!(pending.await.unwrap().0,StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(f.membership().await,vec![seed,report]);
        assert!(f.marked().await.is_empty());
        assert_eq!(f.audit_count().await,0);
        let history=f.history().await;
        let (status,html)=f.clear(0,seed).await;
        assert_eq!(status,StatusCode::OK,"{html}");
        assert!(html.contains("<h1>Cleared 2 reports</h1>"));
        assert_eq!(f.marked().await,vec![seed,report]);
        assert!(f.membership().await.is_empty());
        assert_eq!(f.audit_count().await,1);
        assert_eq!(f.history().await,history);
    }}).await;
    f.cleanup().await;
    result.unwrap();
}
