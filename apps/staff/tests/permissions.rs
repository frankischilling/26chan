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
    reports: [i64; 2],
}

impl Fixture {
    async fn new() -> Self {
        async fn pool(key: &str) -> PgPool {
            PgPool::connect(&std::env::var(key).unwrap()).await.unwrap()
        }
        let owner = pool("MIGRATION_DATABASE_URL").await;
        let auth_pool = pool("AUTH_DATABASE_URL").await;
        let staff = pool("STAFF_DATABASE_URL").await;
        let state = Arc::new(AppState {
            config: Config {
                origin: "http://localhost:3001".into(),
                public_origin: "http://localhost:3000".into(),
                media_origin: "http://127.0.0.1:3002".into(),
                bind: "127.0.0.1:3001".parse().unwrap(),
                production: false,
                auth_database: String::new(),
                staff_database: String::new(),
                idle_timeout: Duration::from_secs(60),
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
            [0, 1].map(|_| format!("sp{}", &uuid::Uuid::new_v4().simple().to_string()[..8]));
        let mut posts = [0; 2];
        let mut reports = [0; 2];
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
            reports[index] = sqlx::query_scalar("INSERT INTO content.reports(board,post_id,reason) VALUES($1,$2,'Owned scope report') RETURNING id")
                .bind(board).bind(posts[index]).fetch_one(&owner).await.unwrap();
        }
        let account = sqlx::query_scalar("INSERT INTO staff_identity.accounts(role,allow_boards,deny_boards) VALUES('janitor',$1,$2) RETURNING id")
            .bind(boards.to_vec()).bind(vec![boards[1].clone()]).fetch_one(&owner).await.unwrap();
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
            reports,
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

    async fn action(&self, board: usize, target: i64, action: &str) -> StatusCode {
        self.request(
            "/moderate",
            Some(format!(
                "csrf={}&board={}&target={target}&action={action}",
                self.csrf, self.boards[board]
            )),
        )
        .await
        .0
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
        for statement in [
            "DELETE FROM content.moderation_audit WHERE board=ANY($1)",
            "DELETE FROM content.reports WHERE board=ANY($1)",
            "DELETE FROM content.posts WHERE board=ANY($1)",
            "DELETE FROM content.threads WHERE board=ANY($1)",
            "DELETE FROM content.boards WHERE slug=ANY($1)",
        ] {
            sqlx::query(statement)
                .bind(self.boards.to_vec())
                .execute(&self.owner)
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
                .execute(&self.owner)
                .await
                .unwrap();
        }
        self.auth.close().await;
        self.staff.close().await;
        self.owner.close().await;
    }
}

async fn exercise(f: &Fixture) {
    auth::check_identity(&f.auth, "board_auth").await.unwrap();
    auth::check_identity(&f.staff, "board_staff").await.unwrap();
    let (status, html) = f.request("/reports", None).await;
    assert_eq!(status, StatusCode::OK, "{html}");
    assert!(html.contains(&format!("/{}/ post", f.boards[0])));
    assert!(!html.contains(&format!("/{}/ post", f.boards[1])));
    assert!(html.contains("Remove post"));
    assert!(html.contains("Resolve report"));
    assert!(!html.contains("Close thread"));
    assert!(!html.contains("Post with staff badge"));
    assert_eq!(
        f.action(0, f.posts[0], "close").await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        f.action(1, f.reports[1], "resolve").await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        f.action(1, f.posts[1], "remove-thread").await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(f.audit_count().await, 0);
    assert_eq!(
        f.action(0, f.reports[0], "resolve").await,
        StatusCode::SEE_OTHER
    );
    assert_eq!(f.audit_count().await, 1);

    f.role("manager", &f.boards[..1], &[]).await;
    let (status, html) = f.request("/reports", None).await;
    assert_eq!(status, StatusCode::OK);
    assert!(html.contains("Close thread"));
    assert!(html.contains("Post with staff badge"));
    assert!(!html.contains(&format!("/{}/ post", f.boards[1])));
    assert!(!html.contains("Enable permaage"));
    assert_eq!(
        f.action(0, f.posts[0], "close").await,
        StatusCode::SEE_OTHER
    );
    assert_eq!(
        f.action(0, f.posts[0], "permaage").await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        f.action(1, f.posts[1], "close").await,
        StatusCode::FORBIDDEN
    );

    f.role("admin", &["all".into()], &f.boards[..1]).await;
    assert_eq!(
        f.action(0, f.posts[0], "permaage").await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        f.action(1, f.posts[1], "close").await,
        StatusCode::SEE_OTHER
    );
    assert_eq!(
        f.action(1, f.posts[1], "invented").await,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(f.audit_count().await, 3);
    assert!(
        sqlx::query("UPDATE staff_identity.accounts SET allow_boards=ARRAY['all'] WHERE id=$1")
            .bind(f.account)
            .execute(&f.auth)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("SELECT * FROM staff_identity.accounts WHERE id=$1")
            .bind(f.account)
            .execute(&f.staff)
            .await
            .is_err()
    );

    f.role("moderator", &["all".into()], &[]).await;
    let mut blocker = f.owner.begin().await.unwrap();
    let blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM staff_identity.accounts WHERE id=$1 FOR UPDATE")
        .bind(f.account)
        .execute(&mut *blocker)
        .await
        .unwrap();
    let pending = tokio::spawn({
        let f = f.clone();
        async move { f.action(0, f.posts[0], "reopen").await }
    });
    let mut observed = false;
    for _ in 0..100 {
        observed = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE usename='board_auth' AND $1=ANY(pg_blocking_pids(pid)))")
            .bind(blocker_pid).fetch_one(&mut *blocker).await.unwrap();
        if observed {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        observed,
        "Protected request must wait for the current account state"
    );
    sqlx::query("UPDATE staff_identity.accounts SET revoked_at=clock_timestamp() WHERE id=$1")
        .bind(f.account)
        .execute(&mut *blocker)
        .await
        .unwrap();
    blocker.commit().await.unwrap();
    assert_eq!(pending.await.unwrap(), StatusCode::UNAUTHORIZED);
    assert_eq!(f.audit_count().await, 3);
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT closed FROM content.threads WHERE id=$1")
            .bind(f.posts[0])
            .fetch_one(&f.owner)
            .await
            .unwrap()
    );

    f.role("moderator", &["all".into()], &[]).await;
    let mut board_lock = f.owner.begin().await.unwrap();
    let blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *board_lock)
        .await
        .unwrap();
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(&f.boards[0])
        .execute(&mut *board_lock)
        .await
        .unwrap();
    let pending = tokio::spawn({
        let f = f.clone();
        async move { f.action(0, f.posts[0], "reopen").await }
    });
    let mut observed = false;
    for _ in 0..100 {
        observed = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE usename='board_staff' AND $1=ANY(pg_blocking_pids(pid)))")
            .bind(blocker_pid).fetch_one(&mut *board_lock).await.unwrap();
        if observed {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(observed);
    let mut operator = f.owner.begin().await.unwrap();
    let error = sqlx::query("SELECT id FROM staff_identity.accounts WHERE id=$1 FOR UPDATE NOWAIT")
        .bind(f.account)
        .execute(&mut *operator)
        .await
        .unwrap_err();
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("55P03")
    );
    operator.rollback().await.unwrap();
    board_lock.commit().await.unwrap();
    assert_eq!(pending.await.unwrap(), StatusCode::SEE_OTHER);
    assert_eq!(f.audit_count().await, 4);
    f.role("janitor", &[], &[]).await;
    assert_eq!(
        f.action(0, f.posts[0], "close").await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(f.audit_count().await, 4);
}

#[tokio::test]
async fn scoped_staff_actions_and_queues_hold_authorization_through_commit() {
    let fixture = Fixture::new().await;
    let result = tokio::spawn({
        let fixture = fixture.clone();
        async move { exercise(&fixture).await }
    })
    .await;
    fixture.cleanup().await;
    result.unwrap();
}
