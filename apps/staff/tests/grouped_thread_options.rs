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

// Every fixture owns its boards and identity. Failure injection below is restricted
// to its own board, so unrelated test binaries cannot encounter the injected error.

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
            [0, 1].map(|_| format!("sp{}", &uuid::Uuid::new_v4().simple().to_string()[..8]));
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
        for statement in [
            "DELETE FROM content.moderation_audit WHERE board=ANY($1)",
            "DELETE FROM content.reports WHERE board=ANY($1)",
            "DELETE FROM post_secrets.posting_thread_actions WHERE board=ANY($1)",
            "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=ANY($1))",
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

const FLAGS: [&str; 5] = ["sticky", "permasage", "closed", "permaage", "undead"];

impl Fixture {
    fn form(&self, options: &str) -> String {
        {
            let base = format!(
                "csrf={}&board={}&target={}",
                self.csrf, self.boards[0], self.posts[0]
            );
            if options.is_empty() {
                base
            } else {
                format!("{base}&{options}")
            }
        }
    }

    async fn options(&self, options: &str) -> StatusCode {
        self.request("/thread-options", Some(self.form(options)))
            .await
            .0
    }

    async fn seed(&self, mask: i16, rank: i16) {
        sqlx::query("UPDATE content.threads SET sticky=$2,closed=$3,permasage=$4,permaage=$5,undead=$6,sticky_rank=$7,bumped_at='1999-01-01Z',modified_at='1999-01-01Z' WHERE id=$1")
            .bind(self.posts[0]).bind(mask & 1 != 0).bind(mask & 4 != 0)
            .bind(mask & 2 != 0).bind(mask & 8 != 0).bind(mask & 16 != 0).bind(rank)
            .execute(&self.owner).await.unwrap();
    }

    async fn state(&self) -> serde_json::Value {
        let raw: String = sqlx::query_scalar("SELECT jsonb_build_object('thread',to_jsonb(t),'posts',(SELECT jsonb_agg(to_jsonb(p) ORDER BY p.id) FROM content.posts p WHERE p.thread_id=t.id))::text FROM content.threads t WHERE t.id=$1")
            .bind(self.posts[0]).fetch_one(&self.owner).await.unwrap();
        serde_json::from_str(&raw).unwrap()
    }

    async fn masks(&self) -> Vec<(i64, i64, String, Option<i16>, Option<i16>)> {
        sqlx::query_as("SELECT account_id,target_id,action,before_mask,after_mask FROM content.moderation_audit WHERE board=$1 ORDER BY id")
            .bind(&self.boards[0]).fetch_all(&self.owner).await.unwrap()
    }

    async fn developer(&self, enabled: bool) {
        sqlx::query("UPDATE staff_identity.accounts SET flags=$2 WHERE id=$1")
            .bind(self.account)
            .bind(if enabled { vec!["developer"] } else { vec![] })
            .execute(&self.owner)
            .await
            .unwrap();
    }
}

fn complete(mask: i16, rank: i16) -> String {
    let mut fields: Vec<_> = FLAGS
        .iter()
        .enumerate()
        .map(|(bit, name)| format!("{name}={}", i16::from(mask & (1 << bit) != 0)))
        .collect();
    fields.push(format!("sticky_rank={rank}"));
    fields.join("&")
}

fn mask(state: &serde_json::Value) -> i16 {
    FLAGS
        .iter()
        .enumerate()
        .map(|(bit, name)| {
            if state["thread"][name].as_bool().unwrap() {
                1 << bit
            } else {
                0
            }
        })
        .sum()
}

fn time(state: &serde_json::Value, field: &str) -> chrono::DateTime<chrono::FixedOffset> {
    chrono::DateTime::parse_from_rfc3339(state["thread"][field].as_str().unwrap()).unwrap()
}

fn invariants(mut state: serde_json::Value) -> serde_json::Value {
    let thread = state["thread"].as_object_mut().unwrap();
    for field in FLAGS.into_iter().chain([
        "sticky_rank",
        "bumped_at",
        "modified_at",
        "http_modified_at",
    ]) {
        thread.remove(field);
    }
    state
}

fn assert_transition(
    before: &serde_json::Value,
    after: &serde_json::Value,
    expected: i16,
    rank: i16,
) {
    assert_eq!(mask(after), expected);
    assert_eq!(after["thread"]["sticky_rank"], rank);
    assert!(time(after, "modified_at") > time(before, "modified_at"));
    assert!(time(after, "http_modified_at") > time(before, "http_modified_at"));
    if mask(before) & 1 != 0 && expected & 1 == 0 {
        assert!(time(after, "bumped_at") > time(before, "bumped_at"));
    } else {
        assert_eq!(after["thread"]["bumped_at"], before["thread"]["bumped_at"]);
    }
    assert_eq!(invariants(after.clone()), invariants(before.clone()));
}

async fn exercise_masks(f: &Fixture) {
    auth::check_identity(&f.auth, "board_auth").await.unwrap();
    auth::check_identity(&f.staff, "board_staff").await.unwrap();
    f.role("manager", &["all".into()], &[]).await;
    let mut audits = Vec::new();
    // Cover each of the five source mask bits in both directions, together.
    for desired in 0..32 {
        let old = 31 ^ desired;
        f.seed(old, if old & 1 != 0 { 59 } else { 0 }).await;
        let before = f.state().await;
        let rank = if desired & 1 != 0 { 60 } else { 0 };
        assert_eq!(
            f.options(&complete(desired, 60)).await,
            StatusCode::SEE_OTHER
        );
        let after = f.state().await;
        assert_transition(&before, &after, desired, rank);
        audits.push((
            f.account,
            f.posts[0],
            "thread-options".into(),
            Some(old),
            Some(desired),
        ));
        assert_eq!(f.masks().await, audits);
        assert_eq!(
            f.options(&complete(desired, 60)).await,
            StatusCode::SEE_OTHER
        );
        assert_transition(&after, &f.state().await, desired, rank);
        assert_eq!(f.masks().await, audits, "no-op refresh must not audit");
    }
    f.seed(31, 60).await;
    let before = f.state().await;
    assert_eq!(f.options("closed=1").await, StatusCode::SEE_OTHER);
    assert_transition(&before, &f.state().await, 4, 0);
    audits.push((
        f.account,
        f.posts[0],
        "thread-options".into(),
        Some(31),
        Some(4),
    ));
    assert_eq!(f.masks().await, audits, "sparse form resets omitted flags");
    f.seed(1, 0).await;
    for rank in [0, 59, 60, 0] {
        let before = f.state().await;
        assert_eq!(
            f.options(&format!("sticky=1&sticky_rank={rank}")).await,
            StatusCode::SEE_OTHER
        );
        assert_transition(&before, &f.state().await, 1, rank);
        assert_eq!(
            f.masks().await,
            audits,
            "rank changes are outside the audit mask"
        );
    }
    f.seed(31, 59).await;
    let before = f.state().await;
    let (a, b) = tokio::join!(f.options(""), f.options(""));
    assert_eq!((a, b), (StatusCode::SEE_OTHER, StatusCode::SEE_OTHER));
    assert_transition(&before, &f.state().await, 0, 0);
    audits.push((
        f.account,
        f.posts[0],
        "thread-options".into(),
        Some(31),
        Some(0),
    ));
    assert_eq!(
        f.masks().await,
        audits,
        "concurrent identical forms produce one effective transition"
    );
}

#[tokio::test]
async fn grouped_masks_sparse_forms_ranks_noops_and_concurrent_audit_are_atomic() {
    let f = Fixture::new().await;
    let result = tokio::spawn({
        let f = f.clone();
        async move { exercise_masks(&f).await }
    })
    .await;
    f.cleanup().await;
    result.unwrap();
}

#[derive(serde::Deserialize)]
struct Reference {
    cases: Vec<ReferenceCase>,
}
#[derive(serde::Deserialize)]
struct ReferenceCase {
    role: String,
    developer: bool,
    allow_all: bool,
    deny_noboard: bool,
    old: bool,
    desired: bool,
    old_undead: bool,
    undead: bool,
    thread_options_allowed: bool,
    prepared_permaage: bool,
}

async fn exercise_roles(f: &Fixture) {
    let reference: Reference =
        serde_json::from_str(include_str!("fixtures/staff-thread-options.json")).unwrap();
    assert_eq!(reference.cases.len(), 512);
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
        let deny = if case.deny_noboard {
            vec!["noboard".into()]
        } else {
            vec![]
        };
        f.role(role, &allow, &deny).await;
        f.developer(case.developer).await;
        let old = i16::from(case.old) * 8 + i16::from(case.old_undead) * 16;
        f.seed(old, 0).await;
        let before = f.state().await;
        let count = f.audit_count().await;
        assert_eq!(
            f.options(&format!(
                "permaage={}&undead={}",
                u8::from(case.desired),
                u8::from(case.undead)
            ))
            .await,
            if case.thread_options_allowed {
                StatusCode::SEE_OTHER
            } else {
                StatusCode::FORBIDDEN
            },
            "role={role} developer={} all={} deny_noboard={}",
            case.developer,
            case.allow_all,
            case.deny_noboard
        );
        let after = f.state().await;
        if case.thread_options_allowed {
            let expected = i16::from(case.prepared_permaage) * 8 + i16::from(case.undead) * 16;
            assert_transition(&before, &after, expected, 0);
            assert_eq!(f.audit_count().await, count + i64::from(old != expected));
            if old != expected {
                assert_eq!(
                    f.masks().await.last().unwrap(),
                    &(
                        f.account,
                        f.posts[0],
                        "thread-options".into(),
                        Some(old),
                        Some(expected)
                    )
                );
            }
        } else {
            assert_eq!(after, before, "developer never elevates janitor rank");
            assert_eq!(f.audit_count().await, count);
        }
    }
    // Missing permaage is also protected, rather than being reset for ordinary mods.
    f.role("moderator", &f.boards[..1], &[]).await;
    f.developer(false).await;
    f.seed(31, 60).await;
    let before = f.state().await;
    assert_eq!(f.options("closed=1").await, StatusCode::SEE_OTHER);
    assert_transition(&before, &f.state().await, 12, 0);
    for role in ["janitor", "moderator", "manager", "admin"] {
        for allow in [vec![f.boards[1].clone()], vec!["all".into()]] {
            let deny = if allow[0] == "all" {
                f.boards[..1].to_vec()
            } else {
                vec![]
            };
            f.role(role, &allow, &deny).await;
            let before = f.state().await;
            let count = f.audit_count().await;
            assert_eq!(f.options("sticky=1").await, StatusCode::FORBIDDEN);
            assert_eq!(
                f.request(
                    &format!(
                        "/thread-options?board={}&target={}",
                        f.boards[0], f.posts[0]
                    ),
                    None
                )
                .await
                .0,
                StatusCode::FORBIDDEN
            );
            assert_eq!(f.state().await, before);
            assert_eq!(f.audit_count().await, count);
        }
    }
}

#[tokio::test]
async fn grouped_options_match_source_actual_roles_global_developer_and_board_scope() {
    let f = Fixture::new().await;
    let result = tokio::spawn({
        let f = f.clone();
        async move { exercise_roles(&f).await }
    })
    .await;
    f.cleanup().await;
    result.unwrap();
}

async fn exercise_rejections(f: &Fixture) {
    f.role("manager", &["all".into()], &[]).await;
    f.seed(31, 59).await;
    let before = f.state().await;
    let count = f.audit_count().await;
    let mut malformed: Vec<String> = [
        "sticky_rank=-1",
        "sticky_rank=61",
        "sticky_rank=999999999999999999999999",
        "sticky_rank=1.0",
        "sticky_rank=1e1",
        "sticky_rank=%2B1",
        "sticky_rank=+1",
        "sticky_rank=1x",
        "sticky_rank=",
        "sticky_rank=%EF%BC%91",
        "sticky_rank=000",
        "sticky_rank[]=1",
        "sticky_rank=0&sticky_rank=1",
        "unknown=1",
        "target=1",
        "board=other",
        "csrf=invalid",
        "sticky[0]=1",
    ]
    .into_iter()
    .map(str::to_string)
    .collect();
    for flag in FLAGS {
        for invalid in [
            "true", "false", "on", "yes", "2", "-1", "01", "1.0", "", "%201",
        ] {
            malformed.push(format!("{flag}={invalid}"));
        }
        malformed.push(format!("{flag}=0&{flag}=1"));
    }
    for fields in malformed {
        let status = f.options(&fields).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "strict form: {fields}");
        assert_eq!(
            f.state().await,
            before,
            "invalid forms cannot refresh clocks"
        );
        assert_eq!(f.audit_count().await, count);
    }
    // Malformed protected fields are rejected even when a moderator cannot set them.
    f.role("moderator", &f.boards[..1], &[]).await;
    assert_eq!(f.options("permaage=2").await, StatusCode::BAD_REQUEST);
    f.role("manager", &["all".into()], &[]).await;
    for (origin, site, cookie, csrf, expected) in [
        (
            "http://evil.invalid",
            "same-origin",
            true,
            true,
            StatusCode::FORBIDDEN,
        ),
        (
            "http://localhost:3001",
            "cross-site",
            true,
            true,
            StatusCode::FORBIDDEN,
        ),
        (
            "http://localhost:3001",
            "same-origin",
            true,
            false,
            StatusCode::FORBIDDEN,
        ),
        (
            "http://localhost:3001",
            "same-origin",
            false,
            true,
            StatusCode::UNAUTHORIZED,
        ),
    ] {
        let mut request = Request::post("/thread-options")
            .header("content-type", "application/x-www-form-urlencoded")
            .header("origin", origin)
            .header("sec-fetch-site", site);
        if cookie {
            request = request.header(
                "cookie",
                format!("staff={}; staff-csrf={}", f.token, f.csrf),
            );
        }
        let form = if csrf {
            f.form("closed=0")
        } else {
            format!(
                "csrf=invalid&board={}&target={}&closed=0",
                f.boards[0], f.posts[0]
            )
        };
        let response = f
            .app
            .clone()
            .oneshot(request.body(Body::from(form)).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), expected);
        assert_eq!(f.state().await, before);
        assert_eq!(f.audit_count().await, count);
    }
    sqlx::query("UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp()-interval '11 minutes' WHERE token_hash=$1")
        .bind(auth::hash(&f.token)).execute(&f.owner).await.unwrap();
    let path = format!(
        "/thread-options?board={}&target={}",
        f.boards[0], f.posts[0]
    );
    assert_eq!(
        f.request(&path, None).await.0,
        StatusCode::OK,
        "nonrecent authority may read but not save"
    );
    assert_eq!(f.options("closed=0").await, StatusCode::FORBIDDEN);
    assert_eq!(f.state().await, before);
    sqlx::query("UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp(),expires_at=clock_timestamp()-interval '1 second' WHERE token_hash=$1")
        .bind(auth::hash(&f.token)).execute(&f.owner).await.unwrap();
    assert_eq!(f.options("closed=0").await, StatusCode::UNAUTHORIZED);
    assert_eq!(f.state().await, before);
    sqlx::query("UPDATE staff_identity.sessions SET expires_at=clock_timestamp()+interval '1 hour' WHERE token_hash=$1")
        .bind(auth::hash(&f.token)).execute(&f.owner).await.unwrap();
    let reply: i64 = sqlx::query_scalar("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,$2,'Anonymous','','Owned grouped reply') RETURNING id")
        .bind(&f.boards[0]).bind(f.posts[0]).fetch_one(&f.owner).await.unwrap();
    let before = f.state().await;
    for (board, target, expected) in [
        (f.boards[0].as_str(), reply, StatusCode::NOT_FOUND),
        (f.boards[0].as_str(), i64::MAX, StatusCode::NOT_FOUND),
        (f.boards[1].as_str(), f.posts[0], StatusCode::NOT_FOUND),
        ("missing", f.posts[0], StatusCode::NOT_FOUND),
        (f.boards[0].as_str(), 0, StatusCode::BAD_REQUEST),
        (f.boards[0].as_str(), -1, StatusCode::BAD_REQUEST),
    ] {
        assert_eq!(
            f.request(
                "/thread-options",
                Some(format!(
                    "csrf={}&board={board}&target={target}&closed=0",
                    f.csrf
                ))
            )
            .await
            .0,
            expected
        );
        assert_eq!(
            f.request(
                &format!("/thread-options?board={board}&target={target}"),
                None
            )
            .await
            .0,
            expected
        );
        assert_eq!(f.state().await, before);
        assert_eq!(f.audit_count().await, count);
    }
    sqlx::query("UPDATE content.threads SET sticky=false,sticky_rank=0,archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
        .bind(f.posts[0]).execute(&f.owner).await.unwrap();
    let archived = f.state().await;
    assert_eq!(f.options("closed=0").await, StatusCode::BAD_REQUEST);
    assert_eq!(f.request(&path, None).await.0, StatusCode::BAD_REQUEST);
    assert_eq!(f.state().await, archived);
    sqlx::query("UPDATE content.threads SET deleted=true WHERE id=$1")
        .bind(f.posts[0])
        .execute(&f.owner)
        .await
        .unwrap();
    let deleted = f.state().await;
    assert_eq!(f.options("closed=0").await, StatusCode::NOT_FOUND);
    assert_eq!(f.request(&path, None).await.0, StatusCode::NOT_FOUND);
    assert_eq!(f.state().await, deleted);
    assert_eq!(f.audit_count().await, count);
}

#[tokio::test]
async fn grouped_strict_forms_target_validation_and_http_authority_guards_never_mutate() {
    let f = Fixture::new().await;
    let result = tokio::spawn({
        let f = f.clone();
        async move { exercise_rejections(&f).await }
    })
    .await;
    f.cleanup().await;
    result.unwrap();
}

async fn wait_for_blocker(owner: &PgPool, blocker: i32, user: &str) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE usename=$2 AND $1=ANY(pg_blocking_pids(pid)))")
                .bind(blocker).bind(user).fetch_one(owner).await.unwrap();
            if waiting { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("actual database lock wait was not observed");
}

async fn exercise_waits(f: &Fixture) {
    f.role("manager", &["all".into()], &[]).await;
    f.seed(31, 60).await;
    let before = f.state().await;
    let mut lock = f.owner.begin().await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM content.threads WHERE id=$1 FOR UPDATE")
        .bind(f.posts[0])
        .execute(&mut *lock)
        .await
        .unwrap();
    let pending = tokio::spawn({
        let f = f.clone();
        async move { f.options("").await }
    });
    wait_for_blocker(&f.owner, blocker, "board_staff").await;
    let release: chrono::DateTime<chrono::Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    // Authority locks remain held while the staff content transaction waits.
    let mut operator = f.owner.begin().await.unwrap();
    let error = sqlx::query("SELECT id FROM staff_identity.accounts WHERE id=$1 FOR UPDATE NOWAIT")
        .bind(f.account)
        .execute(&mut *operator)
        .await
        .unwrap_err();
    assert_eq!(
        error.as_database_error().and_then(|e| e.code()).as_deref(),
        Some("55P03")
    );
    operator.rollback().await.unwrap();
    lock.commit().await.unwrap();
    assert_eq!(pending.await.unwrap(), StatusCode::SEE_OTHER);
    let after = f.state().await;
    assert_transition(&before, &after, 0, 0);
    assert!(
        time(&after, "bumped_at") >= release,
        "unsticky bump must use post-lock time"
    );
    // Preserve the value read under the content lock, not a stale pre-wait
    // value or the moderator's attempted protected-field assignment.
    f.role("moderator", &f.boards[..1], &[]).await;
    f.developer(false).await;
    for (protected, fields) in [(true, "closed=1&permaage=0"), (false, "closed=1")] {
        f.seed(if protected { 0 } else { 8 }, 0).await;
        let count = f.audit_count().await;
        let mut lock = f.owner.begin().await.unwrap();
        let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *lock)
            .await
            .unwrap();
        sqlx::query("SELECT id FROM content.threads WHERE id=$1 FOR UPDATE")
            .bind(f.posts[0])
            .execute(&mut *lock)
            .await
            .unwrap();
        let pending = tokio::spawn({
            let f = f.clone();
            async move { f.options(fields).await }
        });
        wait_for_blocker(&f.owner, blocker, "board_staff").await;
        sqlx::query("UPDATE content.threads SET permaage=$2 WHERE id=$1")
            .bind(f.posts[0])
            .bind(protected)
            .execute(&mut *lock)
            .await
            .unwrap();
        lock.commit().await.unwrap();
        assert_eq!(pending.await.unwrap(), StatusCode::SEE_OTHER);
        let old = if protected { 8 } else { 0 };
        assert_eq!(mask(&f.state().await), old | 4);
        assert_eq!(f.audit_count().await, count + 1);
        assert_eq!(
            f.masks().await.last().unwrap(),
            &(
                f.account,
                f.posts[0],
                "thread-options".into(),
                Some(old),
                Some(old | 4)
            )
        );
    }
    f.role("manager", &["all".into()], &[]).await;
    let count = f.audit_count().await;
    sqlx::query("UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp(),expires_at=clock_timestamp()+interval '1 hour',last_activity_at=clock_timestamp() WHERE token_hash=$1")
        .bind(auth::hash(&f.token)).execute(&f.owner).await.unwrap();
    let before = f.state().await;
    let mut lock = f.owner.begin().await.unwrap();
    let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *lock)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM staff_identity.accounts WHERE id=$1 FOR UPDATE")
        .bind(f.account)
        .execute(&mut *lock)
        .await
        .unwrap();
    let pending = tokio::spawn({
        let f = f.clone();
        async move { f.options("sticky=1").await }
    });
    wait_for_blocker(&f.owner, blocker, "board_auth").await;
    sqlx::query("UPDATE staff_identity.accounts SET revoked_at=clock_timestamp() WHERE id=$1")
        .bind(f.account)
        .execute(&mut *lock)
        .await
        .unwrap();
    lock.commit().await.unwrap();
    assert_eq!(pending.await.unwrap(), StatusCode::UNAUTHORIZED);
    assert_eq!(f.state().await, before);
    assert_eq!(f.audit_count().await, count);
}

#[tokio::test]
async fn grouped_waits_hold_authority_and_use_post_lock_values_and_unsticky_clock() {
    let f = Fixture::new().await;
    let result = tokio::spawn({
        let f = f.clone();
        async move { exercise_waits(&f).await }
    })
    .await;
    f.cleanup().await;
    result.unwrap();
}

async fn exercise_post_lock_expiry(f: &Fixture) {
    auth::check_identity(&f.staff, "board_staff").await.unwrap();
    f.role("manager", &["all".into()], &[]).await;
    let count = f.audit_count().await;
    // Let real wall-clock authority expire while a content row lock is held.
    for recent in [true, false] {
        f.seed(31, 59).await;
        sqlx::query("UPDATE staff_identity.sessions SET authenticated_at=CASE WHEN $2 THEN clock_timestamp()-interval '10 minutes'+interval '3 seconds' ELSE clock_timestamp() END,expires_at=CASE WHEN $2 THEN clock_timestamp()+interval '1 hour' ELSE clock_timestamp()+interval '3 seconds' END,last_activity_at=clock_timestamp() WHERE token_hash=$1")
            .bind(auth::hash(&f.token)).bind(recent).execute(&f.owner).await.unwrap();
        let before = f.state().await;
        let mut lock = f.owner.begin().await.unwrap();
        let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *lock)
            .await
            .unwrap();
        sqlx::query("SELECT id FROM content.threads WHERE id=$1 FOR UPDATE")
            .bind(f.posts[0])
            .execute(&mut *lock)
            .await
            .unwrap();
        let pending = tokio::spawn({
            let f = f.clone();
            async move { f.options("sticky=1&closed=1&sticky_rank=60").await }
        });
        wait_for_blocker(&f.owner, blocker, "board_staff").await;
        tokio::time::timeout(Duration::from_secs(6), async {
            loop {
                let expired: bool = sqlx::query_scalar("SELECT CASE WHEN $2 THEN authenticated_at<=clock_timestamp()-interval '10 minutes' ELSE expires_at<=clock_timestamp() END FROM staff_identity.sessions WHERE token_hash=$1")
                    .bind(auth::hash(&f.token)).bind(recent).fetch_one(&f.owner).await.unwrap();
                if expired { break; }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        }).await.unwrap();
        lock.commit().await.unwrap();
        assert_eq!(
            pending.await.unwrap(),
            if recent {
                StatusCode::FORBIDDEN
            } else {
                StatusCode::UNAUTHORIZED
            }
        );
        assert_eq!(
            f.state().await,
            before,
            "expired authority cannot leave partial flags, rank, or clocks"
        );
        assert_eq!(f.audit_count().await, count);
    }
}

#[tokio::test]
async fn grouped_post_lock_expiry_rechecks_live_recent_and_session_authority() {
    // This fixture deliberately holds a content lock across a three-second
    // authority expiry. The production/CI two-second lock timeout would win
    // first, testing a database failure instead of the final authority check.
    // Override only this fixture's staff connections, with both waits bounded;
    // leave role defaults and the initial/final authority checks unchanged.
    let staff = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET lock_timeout='5s'")
                    .execute(&mut *connection)
                    .await?;
                sqlx::query("SET statement_timeout='10s'")
                    .execute(connection)
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
        async move { exercise_post_lock_expiry(&f).await }
    })
    .await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn grouped_default_lock_timeout_rolls_back_flags_rank_clocks_and_audit() {
    // Match scripts/dev-staff-db.sh explicitly so this proof also runs against
    // local test databases whose role defaults have not been configured.
    let staff = sqlx::postgres::PgPoolOptions::new()
        .max_connections(2)
        .after_connect(|connection, _| {
            Box::pin(async move {
                sqlx::query("SET lock_timeout='2s'")
                    .execute(&mut *connection)
                    .await?;
                sqlx::query("SET statement_timeout='5s'")
                    .execute(connection)
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
            auth::check_identity(&f.staff, "board_staff").await.unwrap();
            f.role("manager", &["all".into()], &[]).await;
            f.seed(31, 60).await;
            let before = f.state().await;
            let count = f.audit_count().await;
            let mut lock = f.owner.begin().await.unwrap();
            let blocker: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                .fetch_one(&mut *lock)
                .await
                .unwrap();
            sqlx::query("SELECT id FROM content.threads WHERE id=$1 FOR UPDATE")
                .bind(f.posts[0])
                .execute(&mut *lock)
                .await
                .unwrap();
            let pending = tokio::spawn({
                let f = f.clone();
                async move { f.options("").await }
            });
            wait_for_blocker(&f.owner, blocker, "board_staff").await;
            // Keep the blocker held until the request actually times out.
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(10), pending)
                    .await
                    .expect("staff lock timeout did not finish the request")
                    .unwrap(),
                StatusCode::SERVICE_UNAVAILABLE
            );
            assert_eq!(f.state().await, before);
            assert_eq!(f.audit_count().await, count);
            lock.rollback().await.unwrap();
            assert_eq!(f.options("").await, StatusCode::SEE_OTHER);
            assert_transition(&before, &f.state().await, 0, 0);
            assert_eq!(f.audit_count().await, count + 1);
        }
    })
    .await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn grouped_owned_audit_failure_rolls_back_all_flags_rank_and_clocks() {
    let f = Fixture::new().await;
    // PostgreSQL DDL identifiers and trigger WHEN literals cannot use bind
    // parameters. Audit the only dynamic input before AssertSqlSafe: an owned
    // UUID-derived slug, never request data, with no quotes or SQL punctuation.
    assert!(f.boards[0].starts_with("sp"));
    assert_eq!(f.boards[0].len(), 10);
    assert!(
        f.boards[0][2..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    );
    let function = format!("grouped_fail_{}", f.boards[0]);
    // Unique board predicate prevents poisoning any unrelated concurrent fixture.
    // No global audit table lock or globally failing trigger is used.
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE FUNCTION content.{function}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'owned grouped audit failure'; END $$")))
        .execute(&f.owner).await.unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE TRIGGER {function} BEFORE INSERT ON content.moderation_audit FOR EACH ROW WHEN (NEW.board='{}' AND NEW.action='thread-options') EXECUTE FUNCTION content.{function}()", f.boards[0])))
        .execute(&f.owner).await.unwrap();
    let result = tokio::spawn({
        let f = f.clone();
        async move {
            f.role("manager", &["all".into()], &[]).await;
            f.seed(31, 60).await;
            let before = f.state().await;
            assert_eq!(f.options("").await, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(
                f.state().await,
                before,
                "failed audit must undo unsticky bump and all grouped assignments"
            );
            assert_eq!(f.audit_count().await, 0);
            // The predicate also proves another owned board can still audit normally.
            assert_eq!(
                f.request(
                    "/thread-options",
                    Some(format!(
                        "csrf={}&board={}&target={}&closed=1",
                        f.csrf, f.boards[1], f.posts[1]
                    ))
                )
                .await
                .0,
                StatusCode::SEE_OTHER
            );
            assert_eq!(f.audit_count().await, 1);
        }
    })
    .await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP TRIGGER {function} ON content.moderation_audit"
    )))
    .execute(&f.owner)
    .await
    .unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP FUNCTION content.{function}()"
    )))
    .execute(&f.owner)
    .await
    .unwrap();
    if result.is_ok() {
        assert_eq!(
            f.options("").await,
            StatusCode::SEE_OTHER,
            "same request succeeds after removing owned fault"
        );
        assert_eq!(f.audit_count().await, 2);
    }
    f.cleanup().await;
    result.unwrap();
}
