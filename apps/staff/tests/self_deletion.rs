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

static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

// Source delete_post (imgboard.php:2490-2497,2545-2594).
// Every fixture owns its boards, session, and failure injection predicate.
// Default production pool timeouts are retained; no test sleeps behind a lock.

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
                asn_database: None,
                browser_hint_key: None,
                media: None,
                proxy: None,
                poster_id_key: Some(Arc::new(
                    board_domain::poster_id::PosterIdKey::parse(&"35".repeat(32)).unwrap(),
                )),
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
        self.request_from(path, form, Some("192.0.2.10:45000"))
            .await
    }

    async fn request_from(
        &self,
        path: &str,
        form: Option<String>,
        peer: Option<&str>,
    ) -> (StatusCode, String) {
        let mut request = Request::builder()
            .uri(path)
            .header("forwarded", "for=192.0.2.10")
            .header("x-forwarded-for", "192.0.2.10")
            .header("x-board-client-ip", "192.0.2.10")
            .header(
                "cookie",
                format!("staff={}; staff-csrf={}", self.token, self.csrf),
            );
        if let Some(peer) = peer {
            request = request.extension(axum::extract::ConnectInfo(
                peer.parse::<std::net::SocketAddr>().unwrap(),
            ));
        }
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
            "DELETE FROM content.post_media WHERE post_id IN(SELECT id FROM content.posts WHERE board=ANY($1))",
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

impl Fixture {
    async fn prepare_target(&self, own_peer: bool, password: Option<&str>) {
        let board = &self.boards[0];
        let post = self.posts[0];
        let report:i64=sqlx::query_scalar("INSERT INTO content.reports(board,post_id,reason) VALUES($1,$2,'Owned self deletion report') RETURNING id")
            .bind(board).bind(post).fetch_one(&self.owner).await.unwrap();
        let mut evidence = self.owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_report_admission_owner")
            .execute(&mut *evidence)
            .await
            .unwrap();
        sqlx::query("INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at) SELECT $1,decode(repeat('ab',32),'hex'),board,id,thread_id,clock_timestamp() FROM content.posts WHERE id=$2")
            .bind(report).bind(post).execute(&mut *evidence).await.unwrap();
        evidence.commit().await.unwrap();
        if own_peer {
            let key = board_domain::poster_id::PosterIdKey::parse(&"35".repeat(32)).unwrap();
            let actor = key.public_posting_rate_identity("192.0.2.10".parse().unwrap());
            sqlx::query("INSERT INTO post_secrets.posting_history(post_id,board,thread_id,actor_hash,request_at) VALUES($1,$2,$1,$3,1)")
                .bind(post).bind(board).bind(actor.as_bytes().as_slice()).execute(&self.owner).await.unwrap();
        }
        if let Some(password) = password {
            use argon2::{Argon2, PasswordHasher, password_hash::SaltString};
            let hash = Argon2::default()
                .hash_password(
                    password.as_bytes(),
                    &SaltString::encode_b64(b"owned-self-delete").unwrap(),
                )
                .unwrap()
                .to_string();
            sqlx::query("INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES($1,$2)")
                .bind(post)
                .bind(hash)
                .execute(&self.owner)
                .await
                .unwrap();
        }
    }

    async fn delete(&self, action: &str, password: &str) -> StatusCode {
        self.request(
            "/moderate",
            Some(format!(
                "csrf={}&board={}&target={}&action={action}&deletion_password={password}",
                self.csrf, self.boards[0], self.posts[0]
            )),
        )
        .await
        .0
    }

    async fn assert_file_result(&self, expected_reports: i64, expected_action: &str) {
        let row: (bool, bool, String) =
            sqlx::query_as("SELECT deleted,file_deleted,comment FROM content.posts WHERE id=$1")
                .bind(self.posts[0])
                .fetch_one(&self.owner)
                .await
                .unwrap();
        assert_eq!(row, (false, true, "Scope fixture".into()));
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM content.reports WHERE board=$1")
            .bind(&self.boards[0])
            .fetch_one(&self.owner)
            .await
            .unwrap();
        assert_eq!(
            count, expected_reports,
            "self deletion must preserve report evidence"
        );
        let mut evidence = self.owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_report_admission_owner")
            .execute(&mut *evidence)
            .await
            .unwrap();
        let membership: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM post_secrets.report_membership WHERE board=$1",
        )
        .bind(&self.boards[0])
        .fetch_one(&mut *evidence)
        .await
        .unwrap();
        let groups: i64 =
            sqlx::query_scalar("SELECT count(*) FROM post_secrets.report_group WHERE board=$1")
                .bind(&self.boards[0])
                .fetch_one(&mut *evidence)
                .await
                .unwrap();
        assert_eq!((membership, groups), (expected_reports, expected_reports));
        evidence.rollback().await.unwrap();
        let actions: Vec<String> = sqlx::query_scalar(
            "SELECT action FROM content.moderation_audit WHERE account_id=$1 ORDER BY id",
        )
        .bind(self.account)
        .fetch_all(&self.owner)
        .await
        .unwrap();
        assert_eq!(actions, vec![expected_action]);
    }
}

#[tokio::test]
async fn verified_peer_self_file_deletion_preserves_reports_and_records_self_event() {
    let _serial = TEST_LOCK.lock().await;
    let fixture = Fixture::new().await;
    let f = fixture.clone();
    let result = tokio::spawn(async move {
        f.prepare_target(true, None).await;
        assert_eq!(f.delete("remove-file", "").await, StatusCode::SEE_OTHER);
        f.assert_file_result(1, "self-remove-file").await;
    })
    .await;
    fixture.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn verified_password_self_file_deletion_preserves_reports_and_records_self_event() {
    let _serial = TEST_LOCK.lock().await;
    let fixture = Fixture::new().await;
    let f = fixture.clone();
    let result = tokio::spawn(async move {
        f.prepare_target(false, Some("owned-self-password")).await;
        assert_eq!(
            f.delete("remove-file", "owned-self-password").await,
            StatusCode::SEE_OTHER
        );
        f.assert_file_result(1, "self-remove-file").await;
    })
    .await;
    fixture.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn absent_self_proof_keeps_administrative_file_retirement() {
    let _serial = TEST_LOCK.lock().await;
    let fixture = Fixture::new().await;
    let f = fixture.clone();
    let result = tokio::spawn(async move {
        f.prepare_target(false, None).await;
        assert_eq!(f.delete("remove-file", "").await, StatusCode::SEE_OTHER);
        f.assert_file_result(0, "remove-file").await;
    })
    .await;
    fixture.cleanup().await;
    result.unwrap();
}

impl Fixture {
    async fn assert_untouched(&self) {
        let state: (bool, bool, String) =
            sqlx::query_as("SELECT deleted,file_deleted,comment FROM content.posts WHERE id=$1")
                .bind(self.posts[0])
                .fetch_one(&self.owner)
                .await
                .unwrap();
        assert_eq!(state, (false, false, "Scope fixture".into()));
        let reports: i64 =
            sqlx::query_scalar("SELECT count(*) FROM content.reports WHERE board=$1")
                .bind(&self.boards[0])
                .fetch_one(&self.owner)
                .await
                .unwrap();
        assert_eq!(reports, 1);
        assert_eq!(self.audit_count().await, 0);
    }
}

#[tokio::test]
async fn wrong_explicit_password_refuses_instead_of_clearing_reports() {
    let _serial = TEST_LOCK.lock().await;
    let fixture = Fixture::new().await;
    let f = fixture.clone();
    let result = tokio::spawn(async move {
        f.prepare_target(false, Some("correct-owned-password"))
            .await;
        assert_eq!(
            f.delete("remove-file", "wrong-password").await,
            StatusCode::FORBIDDEN
        );
        f.assert_untouched().await;
        assert_eq!(
            f.delete("remove-file", &"x".repeat(129)).await,
            StatusCode::BAD_REQUEST
        );
        f.assert_untouched().await;
    })
    .await;
    fixture.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn current_peer_proof_remains_sufficient_with_wrong_password() {
    let _serial = TEST_LOCK.lock().await;
    let fixture = Fixture::new().await;
    let f = fixture.clone();
    let result = tokio::spawn(async move {
        f.prepare_target(true, Some("correct-owned-password")).await;
        assert_eq!(
            f.delete("remove-file", "wrong-password").await,
            StatusCode::SEE_OTHER
        );
        f.assert_file_result(1, "self-remove-file").await;
    })
    .await;
    fixture.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn self_proof_never_overrides_board_or_sticky_authority() {
    let _serial = TEST_LOCK.lock().await;
    let fixture = Fixture::new().await;
    let f = fixture.clone();
    let result = tokio::spawn(async move {
        f.prepare_target(true, Some("owned-password")).await;
        f.role("janitor", &f.boards, &f.boards[..1]).await;
        assert_eq!(
            f.delete("remove-file", "owned-password").await,
            StatusCode::FORBIDDEN
        );
        f.assert_untouched().await;
        f.role("janitor", &f.boards, &[]).await;
        sqlx::query("UPDATE content.threads SET sticky=true WHERE id=$1")
            .bind(f.posts[0])
            .execute(&f.owner)
            .await
            .unwrap();
        assert_eq!(
            f.delete("remove-post", "owned-password").await,
            StatusCode::FORBIDDEN
        );
        f.assert_untouched().await;
    })
    .await;
    fixture.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn self_proof_never_overrides_csrf_or_revoked_session() {
    let _serial = TEST_LOCK.lock().await;
    let fixture = Fixture::new().await;
    let f = fixture.clone();
    let result = tokio::spawn(async move {
        f.prepare_target(true, None).await;
        assert_eq!(
            f.request(
                "/moderate",
                Some(format!(
                    "csrf=wrong&board={}&target={}&action=remove-file",
                    f.boards[0], f.posts[0]
                ))
            )
            .await
            .0,
            StatusCode::FORBIDDEN
        );
        f.assert_untouched().await;
        sqlx::query("DELETE FROM staff_identity.sessions WHERE account_id=$1")
            .bind(f.account)
            .execute(&f.owner)
            .await
            .unwrap();
        let status = f.delete("remove-file", "").await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        f.assert_untouched().await;
    })
    .await;
    fixture.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn self_whole_deletion_erases_payload_and_reports_with_distinct_audit() {
    let _serial = TEST_LOCK.lock().await;
    for action in ["remove-post", "remove-thread"] {
        let fixture = Fixture::new().await;
        let f = fixture.clone();
        let result = tokio::spawn(async move {
            f.prepare_target(true, Some("owned-password")).await;
            assert_eq!(
                f.delete(action, "owned-password").await,
                StatusCode::SEE_OTHER
            );
            let row: (bool, String) =
                sqlx::query_as("SELECT deleted,comment FROM content.posts WHERE id=$1")
                    .bind(f.posts[0])
                    .fetch_one(&f.owner)
                    .await
                    .unwrap();
            assert_eq!(row, (true, String::new()));
            let reports: i64 =
                sqlx::query_scalar("SELECT count(*) FROM content.reports WHERE board=$1")
                    .bind(&f.boards[0])
                    .fetch_one(&f.owner)
                    .await
                    .unwrap();
            assert_eq!(reports, 0);
            let proofs: i64 =
                sqlx::query_scalar("SELECT count(*) FROM post_secrets.deletion WHERE post_id=$1")
                    .bind(f.posts[0])
                    .fetch_one(&f.owner)
                    .await
                    .unwrap();
            assert_eq!(proofs, 0);
            let audit: String = sqlx::query_scalar(
                "SELECT action FROM content.moderation_audit WHERE account_id=$1",
            )
            .bind(f.account)
            .fetch_one(&f.owner)
            .await
            .unwrap();
            assert_eq!(audit, format!("self-{action}"));
        })
        .await;
        fixture.cleanup().await;
        result.unwrap();
    }
}

#[tokio::test]
async fn password_rotation_while_waiting_rejects_preverified_stale_proof() {
    let _serial = TEST_LOCK.lock().await;
    let fixture = Fixture::new().await;
    let f = fixture.clone();
    let result=tokio::spawn(async move {
        f.prepare_target(false,Some("owned-password")).await;
        let mut locked=f.owner.begin().await.unwrap();
        let blocker:i32=sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *locked).await.unwrap();
        sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
            .bind(&f.boards[0]).execute(&mut *locked).await.unwrap();
        let pending=f.clone();
        let request=tokio::spawn(async move {pending.delete("remove-file","owned-password").await});
        let started=std::time::Instant::now();
        let mut observed=false;
        while started.elapsed()<Duration::from_secs(3) {
            observed=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE usename='board_staff' AND $1=ANY(pg_blocking_pids(pid)))")
                .bind(blocker).fetch_one(&f.owner).await.unwrap();
            if observed {break;}
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        sqlx::query("UPDATE post_secrets.deletion SET password_hash='rotated-owned-hash' WHERE post_id=$1")
            .bind(f.posts[0]).execute(&mut *locked).await.unwrap();
        locked.commit().await.unwrap();
        let status=request.await.unwrap();
        assert!(observed,"request must reach the board lock after password verification");
        assert_eq!(status,StatusCode::FORBIDDEN);
        f.assert_untouched().await;
    }).await;
    fixture.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn ownership_helpers_are_narrow_and_runtime_private() {
    let _serial = TEST_LOCK.lock().await;
    let fixture = Fixture::new().await;
    let f = fixture.clone();
    let result = tokio::spawn(async move {
        f.prepare_target(true, Some("owned-password")).await;
        let key = board_domain::poster_id::PosterIdKey::parse(&"35".repeat(32)).unwrap();
        let actor = key.public_posting_rate_identity("192.0.2.10".parse().unwrap());
        let matches: bool = sqlx::query_scalar("SELECT content.staff_post_peer_matches($1,$2,$3)")
            .bind(&f.boards[0])
            .bind(f.posts[0])
            .bind(actor.as_bytes().as_slice())
            .fetch_one(&f.staff)
            .await
            .unwrap();
        assert!(matches);
        let hash: Option<String> =
            sqlx::query_scalar("SELECT content.staff_post_deletion_hash($1,$2)")
                .bind(&f.boards[0])
                .bind(f.posts[0])
                .fetch_one(&f.staff)
                .await
                .unwrap();
        assert!(hash.is_some_and(|h| h.starts_with("$argon2id$")));
        for board in [&f.boards[1], "missing-owned-board"] {
            let hash: Option<String> =
                sqlx::query_scalar("SELECT content.staff_post_deletion_hash($1,$2)")
                    .bind(board)
                    .bind(f.posts[0])
                    .fetch_one(&f.staff)
                    .await
                    .unwrap();
            assert!(hash.is_none());
            let matches: bool =
                sqlx::query_scalar("SELECT content.staff_post_peer_matches($1,$2,$3)")
                    .bind(board)
                    .bind(f.posts[0])
                    .bind(actor.as_bytes().as_slice())
                    .fetch_one(&f.staff)
                    .await
                    .unwrap();
            assert!(!matches);
        }
        for name in [
            "TEST_PUBLIC_DATABASE_URL",
            "AUTH_DATABASE_URL",
            "MEDIA_DATABASE_URL",
            "MEDIA_READ_DATABASE_URL",
            "INTAKE_DATABASE_URL",
            "MONITOR_DATABASE_URL",
        ] {
            let pool = PgPool::connect(&std::env::var(name).unwrap())
                .await
                .unwrap();
            for query in [
                "SELECT content.staff_post_deletion_hash($1,$2)",
                "SELECT content.staff_post_peer_matches($1,$2,NULL)",
            ] {
                let error = sqlx::query(query)
                    .bind(&f.boards[0])
                    .bind(f.posts[0])
                    .execute(&pool)
                    .await
                    .unwrap_err();
                assert_eq!(
                    error.as_database_error().unwrap().code().as_deref(),
                    Some("42501"),
                    "{name}: {query}"
                );
            }
            pool.close().await;
        }
        for query in [
            "SELECT * FROM post_secrets.deletion",
            "SELECT * FROM post_secrets.posting_history",
        ] {
            let error = sqlx::query(query).execute(&f.staff).await.unwrap_err();
            assert_eq!(
                error.as_database_error().unwrap().code().as_deref(),
                Some("42501")
            );
        }
    })
    .await;
    fixture.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn insecure_helper_catalog_refuses_readiness_and_mutation() {
    let _serial = TEST_LOCK.lock().await;
    let fixture = Fixture::new().await;
    fixture.prepare_target(true, None).await;
    assert_eq!(fixture.request("/readyz", None).await.0, StatusCode::OK);
    {
        let mut tx = fixture.owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_posting_cooldown_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query(
            "ALTER FUNCTION content.staff_post_peer_matches(text,bigint,bytea) SECURITY INVOKER",
        )
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }
    let f = fixture.clone();
    let result = tokio::spawn(async move {
        assert_eq!(
            f.request("/readyz", None).await.0,
            StatusCode::SERVICE_UNAVAILABLE
        );
        assert_eq!(
            f.delete("remove-file", "").await,
            StatusCode::SERVICE_UNAVAILABLE
        );
        f.assert_untouched().await;
    })
    .await;
    {
        let mut tx = fixture.owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_posting_cooldown_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query(
            "ALTER FUNCTION content.staff_post_peer_matches(text,bigint,bytea) SECURITY DEFINER",
        )
        .execute(&mut *tx)
        .await
        .unwrap();
        tx.commit().await.unwrap();
    }
    let restored = fixture.request("/readyz", None).await.0;
    fixture.cleanup().await;
    assert_eq!(restored, StatusCode::OK);
    result.unwrap();
}

#[tokio::test]
async fn audit_failure_rolls_back_self_file_deletion_and_retains_report() {
    let _serial = TEST_LOCK.lock().await;
    let fixture = Fixture::new().await;
    fixture.prepare_target(true, None).await;
    let constraint = format!("owned_self_audit_{}", fixture.account);
    sqlx::query(sqlx::AssertSqlSafe(format!("ALTER TABLE content.moderation_audit ADD CONSTRAINT {constraint} CHECK(account_id<>{}) NOT VALID",fixture.account)))
        .execute(&fixture.owner).await.unwrap();
    let f = fixture.clone();
    let result = tokio::spawn(async move {
        assert_eq!(
            f.delete("remove-file", "").await,
            StatusCode::SERVICE_UNAVAILABLE
        );
        f.assert_untouched().await;
    })
    .await;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "ALTER TABLE content.moderation_audit DROP CONSTRAINT {constraint}"
    )))
    .execute(&fixture.owner)
    .await
    .unwrap();
    fixture.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn forged_headers_cannot_replace_missing_or_different_transport_peer() {
    let _serial = TEST_LOCK.lock().await;
    for peer in [None, Some("192.0.2.99:45000")] {
        let fixture = Fixture::new().await;
        let f = fixture.clone();
        let result = tokio::spawn(async move {
            f.prepare_target(true, None).await;
            let form = format!(
                "csrf={}&board={}&target={}&action=remove-file&remote_addr=192.0.2.10",
                f.csrf, f.boards[0], f.posts[0]
            );
            assert_eq!(
                f.request_from("/moderate", Some(form), peer).await.0,
                StatusCode::SEE_OTHER
            );
            f.assert_file_result(0, "remove-file").await;
        })
        .await;
        fixture.cleanup().await;
        result.unwrap();
    }
}

#[tokio::test]
async fn reply_password_and_repeated_file_actions_preserve_report_evidence() {
    let _serial = TEST_LOCK.lock().await;
    let mut fixture = Fixture::new().await;
    let reply:i64=sqlx::query_scalar("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,$2,'Anonymous','','Scope fixture') RETURNING id")
        .bind(&fixture.boards[0]).bind(fixture.posts[0]).fetch_one(&fixture.owner).await.unwrap();
    fixture.posts[0] = reply;
    let f = fixture.clone();
    let result=tokio::spawn(async move {
        f.prepare_target(false,Some("reply-password")).await;
        assert_eq!(f.delete("remove-file","reply-password").await,StatusCode::SEE_OTHER);
        f.assert_file_result(1,"self-remove-file").await;
        assert_eq!(f.delete("remove-file","reply-password").await,StatusCode::SEE_OTHER);
        let reports:i64=sqlx::query_scalar("SELECT count(*) FROM content.reports WHERE board=$1")
            .bind(&f.boards[0]).fetch_one(&f.owner).await.unwrap();
        assert_eq!(reports,1);
        assert_eq!(f.audit_count().await,2);
        assert_eq!(f.delete("remove-post","reply-password").await,StatusCode::SEE_OTHER);
        let parent_deleted:bool=sqlx::query_scalar("SELECT t.deleted FROM content.posts p JOIN content.threads t ON t.id=p.thread_id WHERE p.id=$1")
            .bind(f.posts[0]).fetch_one(&f.owner).await.unwrap();
        assert!(!parent_deleted);
    }).await;
    fixture.cleanup().await;
    result.unwrap();
}
