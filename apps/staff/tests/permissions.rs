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
    assert!(!html.contains("href=\"/post\""));
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
    assert!(html.contains("href=\"/post\""));
    assert!(!html.contains(&format!("/{}/ post", f.boards[1])));
    assert!(html.contains("Enable permaage"));
    assert!(html.contains("Enable Undead"));
    assert_eq!(
        f.action(0, f.posts[0], "close").await,
        StatusCode::SEE_OTHER
    );
    assert_eq!(
        f.action(0, f.posts[0], "permaage").await,
        StatusCode::SEE_OTHER
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
    assert_eq!(f.audit_count().await, 4);
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
    assert_eq!(f.audit_count().await, 4);
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
    assert_eq!(f.audit_count().await, 5);
    f.role("janitor", &[], &[]).await;
    assert_eq!(
        f.action(0, f.posts[0], "close").await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(f.audit_count().await, 5);
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

#[derive(serde::Deserialize)]
struct ThreadOptionsReference {
    cases: Vec<ThreadOptionsCase>,
}

#[derive(serde::Deserialize)]
struct ThreadOptionsCase {
    role: String,
    developer: bool,
    allow_all: bool,
    deny_noboard: bool,
    old: bool,
    desired: bool,
    old_undead: bool,
    undead: bool,
    thread_options_allowed: bool,
    permaage_allowed: bool,
    prepared_permaage: bool,
    logged: bool,
    audit: Option<ThreadOptionsAudit>,
}

#[derive(serde::Deserialize)]
struct ThreadOptionsAudit {
    old_mask: i32,
    new_mask: i32,
}

async fn thread_options_source_cases(f: &Fixture) {
    let reference: ThreadOptionsReference =
        serde_json::from_str(include_str!("fixtures/staff-thread-options.json")).unwrap();
    assert_eq!(reference.cases.len(), 512);
    let mut compared = 0;
    for case in reference.cases {
        let role = if case.role == "mod" {
            "moderator"
        } else {
            &case.role
        };
        let allow = if case.allow_all {
            vec!["all".into()]
        } else {
            vec![f.boards[0].clone()]
        };
        let deny: Vec<String> = if case.deny_noboard {
            vec!["noboard".into()]
        } else {
            vec![]
        };
        f.role(role, &allow, &deny).await;
        sqlx::query("UPDATE staff_identity.accounts SET flags=$2 WHERE id=$1")
            .bind(f.account)
            .bind(if case.developer {
                vec!["developer"]
            } else {
                vec![]
            })
            .execute(&f.owner)
            .await
            .unwrap();
        for permaage_action in [true, false] {
            // Each UI form changes one option. Select source executions whose
            // other option is unchanged, so source audit masks are comparable.
            if (permaage_action && case.old_undead != case.undead)
                || (!permaage_action && case.old != case.desired)
            {
                continue;
            }
            compared += 1;
            sqlx::query("UPDATE content.threads SET permaage=$2,undead=$3 WHERE id=$1")
                .bind(f.posts[0])
                .bind(case.old)
                .bind(case.old_undead)
                .execute(&f.owner)
                .await
                .unwrap();
            let before: (chrono::DateTime<chrono::Utc>, chrono::DateTime<chrono::Utc>) =
                sqlx::query_as("SELECT bumped_at,modified_at FROM content.threads WHERE id=$1")
                    .bind(f.posts[0])
                    .fetch_one(&f.owner)
                    .await
                    .unwrap();
            let audit_before = f.audit_count().await;
            let action = match (permaage_action, case.desired, case.undead) {
                (true, true, _) => "permaage",
                (true, false, _) => "unpermaage",
                (false, _, true) => "undead",
                (false, _, false) => "unundead",
            };
            let allowed =
                case.thread_options_allowed && (!permaage_action || case.permaage_allowed);
            assert_eq!(
                f.action(0, f.posts[0], action).await,
                if allowed {
                    StatusCode::SEE_OTHER
                } else {
                    StatusCode::FORBIDDEN
                },
                "{} {action} {} {} {}",
                role,
                case.developer,
                case.allow_all,
                case.deny_noboard
            );
            let after: (
                bool,
                bool,
                chrono::DateTime<chrono::Utc>,
                chrono::DateTime<chrono::Utc>,
            ) = sqlx::query_as(
                "SELECT permaage,undead,bumped_at,modified_at FROM content.threads WHERE id=$1",
            )
            .bind(f.posts[0])
            .fetch_one(&f.owner)
            .await
            .unwrap();
            assert_eq!(
                (after.0, after.1),
                (
                    if allowed && permaage_action {
                        case.prepared_permaage
                    } else {
                        case.old
                    },
                    if allowed && !permaage_action {
                        case.undead
                    } else {
                        case.old_undead
                    }
                )
            );
            assert_eq!(after.2, before.0, "option submission never bumps");
            if allowed {
                assert!(
                    after.3 > before.1,
                    "source updates time even when unchanged"
                );
            } else {
                assert_eq!(after.3, before.1);
            }
            assert_eq!(
                f.audit_count().await - audit_before,
                i64::from(allowed && case.logged)
            );
            if allowed && let Some(source) = &case.audit {
                assert_eq!(
                    source.old_mask,
                    i32::from(case.old) * 8 + i32::from(case.old_undead) * 16
                );
                assert_eq!(
                    source.new_mask,
                    i32::from(after.0) * 8 + i32::from(after.1) * 16
                );
                let audit: (i64, i64, String) = sqlx::query_as("SELECT account_id,target_id,action FROM content.moderation_audit WHERE board=$1 ORDER BY id DESC LIMIT 1")
                    .bind(&f.boards[0]).fetch_one(&f.owner).await.unwrap();
                assert_eq!(audit, (f.account, f.posts[0], action.into()));
            }
            let (status, html) = f.request("/reports", None).await;
            assert_eq!(status, StatusCode::OK);
            assert_eq!(
                html.contains("Enable permaage") || html.contains("Disable permaage"),
                case.thread_options_allowed && case.permaage_allowed
            );
            assert_eq!(
                html.contains("Enable Undead") || html.contains("Disable Undead"),
                case.thread_options_allowed
            );
        }
    }
    assert_eq!(compared, 512);
    for role in ["janitor", "moderator", "manager", "admin"] {
        f.role(role, &["all".into()], &f.boards[..1]).await;
        for action in ["permaage", "unpermaage", "undead", "unundead"] {
            assert_eq!(f.action(0, f.posts[0], action).await, StatusCode::FORBIDDEN);
        }
    }
    f.role("manager", &["all".into()], &[]).await;
    let audit_before = f.audit_count().await;
    sqlx::query("UPDATE content.threads SET undead=false WHERE id=$1")
        .bind(f.posts[0])
        .execute(&f.owner)
        .await
        .unwrap();
    let (left, right) = tokio::join!(
        f.action(0, f.posts[0], "undead"),
        f.action(0, f.posts[0], "undead")
    );
    assert_eq!(
        (left, right),
        (StatusCode::SEE_OTHER, StatusCode::SEE_OTHER)
    );
    assert_eq!(f.audit_count().await, audit_before + 1);
    let before_cancel: (
        bool,
        bool,
        chrono::DateTime<chrono::Utc>,
        chrono::DateTime<chrono::Utc>,
    ) = sqlx::query_as(
        "SELECT permaage,undead,bumped_at,modified_at FROM content.threads WHERE id=$1",
    )
    .bind(f.posts[0])
    .fetch_one(&f.owner)
    .await
    .unwrap();
    let mut audit_lock = f.owner.begin().await.unwrap();
    let blocker_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *audit_lock)
        .await
        .unwrap();
    sqlx::query("LOCK TABLE content.moderation_audit IN ACCESS EXCLUSIVE MODE")
        .execute(&mut *audit_lock)
        .await
        .unwrap();
    let pending = tokio::spawn({
        let f = f.clone();
        async move { f.action(0, f.posts[0], "unundead").await }
    });
    let mut observed = false;
    for _ in 0..100 {
        observed=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE usename='board_staff' AND $1=ANY(pg_blocking_pids(pid)))")
            .bind(blocker_pid).fetch_one(&mut *audit_lock).await.unwrap();
        if observed {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        observed,
        "New option must reach the blocked audit before cancellation"
    );
    pending.abort();
    assert!(pending.await.unwrap_err().is_cancelled());
    audit_lock.rollback().await.unwrap();
    let after_cancel: (
        bool,
        bool,
        chrono::DateTime<chrono::Utc>,
        chrono::DateTime<chrono::Utc>,
    ) = sqlx::query_as(
        "SELECT permaage,undead,bumped_at,modified_at FROM content.threads WHERE id=$1",
    )
    .bind(f.posts[0])
    .fetch_one(&f.owner)
    .await
    .unwrap();
    assert_eq!(after_cancel, before_cancel);
    assert_eq!(f.audit_count().await, audit_before + 1);
    for action in ["permaage", "undead"] {
        assert_eq!(
            f.request(
                "/moderate",
                Some(format!(
                    "csrf=invalid&board={}&target={}&action={action}",
                    f.boards[0], f.posts[0]
                ))
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
    }
    sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
        .bind(f.posts[0]).execute(&f.owner).await.unwrap();
    for action in ["permaage", "unpermaage", "undead", "unundead"] {
        assert_eq!(
            f.action(0, f.posts[0], action).await,
            StatusCode::BAD_REQUEST
        );
    }
    let (_, html) = f.request("/reports", None).await;
    let marker = format!("<article id=\"report-{}\">", f.reports[0]);
    let archived = html
        .split(&marker)
        .nth(1)
        .unwrap()
        .split("</article>")
        .next()
        .unwrap();
    assert!(!archived.contains("Enable permaage") && !archived.contains("Disable permaage"));
    assert!(!archived.contains("Enable Undead") && !archived.contains("Disable Undead"));
    assert_eq!(f.audit_count().await, audit_before + 1);
}

#[tokio::test]
async fn permaage_and_undead_match_original_permission_state_and_audit_cases() {
    let fixture = Fixture::new().await;
    let result = tokio::spawn({
        let fixture = fixture.clone();
        async move { thread_options_source_cases(&fixture).await }
    })
    .await;
    fixture.cleanup().await;
    result.unwrap();
}
