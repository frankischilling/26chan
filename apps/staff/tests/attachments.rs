#![cfg(feature = "database-tests")]

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use board_staff::{AppState, Config, Limits, auth, router, store};
use http_body_util::BodyExt;
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use tower::ServiceExt;
use webauthn_rs::prelude::*;

async fn pool(key: &str) -> PgPool {
    PgPool::connect(&std::env::var(key).expect("Explicit database test credentials required"))
        .await
        .unwrap()
}

// Metadata-only synthetic fixture. Decoder/publication and real file bytes are
// tested by the media suites, not by these administrator-inserted rows.
struct Fixture {
    owner: PgPool,
    state: Arc<AppState>,
    board: String,
    post: i64,
    report: i64,
    asset: String,
    account: i64,
    token: String,
    csrf: String,
}
impl Fixture {
    async fn new() -> Self {
        let owner = pool("MIGRATION_DATABASE_URL").await;
        let board = format!("s{}", &uuid::Uuid::new_v4().simple().to_string()[..9]);
        sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES ($1,'Staff attachment test','Synthetic',16000,100,100,100,10)")
            .bind(&board).execute(&owner).await.unwrap();
        let post: i64 =
            sqlx::query_scalar("INSERT INTO content.threads(board) VALUES ($1) RETURNING id")
                .bind(&board)
                .fetch_one(&owner)
                .await
                .unwrap();
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES ($1,$2,$1,'Synthetic','','Keep this text')")
            .bind(post).bind(&board).execute(&owner).await.unwrap();
        let report = sqlx::query_scalar("INSERT INTO content.reports(board,post_id,reason) VALUES ($1,$2,'Synthetic') RETURNING id")
            .bind(&board).bind(post).fetch_one(&owner).await.unwrap();
        let asset = uuid::Uuid::new_v4().simple().to_string();
        sqlx::query("INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at,md5,thumbnail_sha256,thumbnail_bytes,thumbnail_width,thumbnail_height) VALUES ($1,$1,$1,repeat('a',64),100,500,300,'approved',clock_timestamp(),repeat('b',32),repeat('c',64),50,250,150)")
            .bind(&asset).execute(&owner).await.unwrap();
        sqlx::query("INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler) VALUES ($1,$2,$2,$3,100,500,300,false)")
            .bind(post).bind(&asset).bind("<img src=x onerror=alert(1)> & \"name\".png").execute(&owner).await.unwrap();
        let account: i64 = sqlx::query_scalar(
            "INSERT INTO staff_identity.accounts(role) VALUES ('moderator') RETURNING id",
        )
        .fetch_one(&owner)
        .await
        .unwrap();
        let credential = uuid::Uuid::new_v4().as_bytes().to_vec();
        sqlx::query("INSERT INTO staff_identity.credentials(id,account_id,credential) VALUES ($1,$2,'{}'::jsonb)")
            .bind(&credential).bind(account).execute(&owner).await.unwrap();
        let token = auth::token();
        let csrf = auth::token();
        sqlx::query("INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id) VALUES ($1,$2,$3,$4)")
            .bind(auth::hash(&token)).bind(auth::hash(&csrf)).bind(account).bind(credential).execute(&owner).await.unwrap();
        let origin = Url::parse("http://localhost:3001").unwrap();
        let state = Arc::new(AppState {
            config: Config {
                origin: origin.origin().ascii_serialization(),
                media_origin: "http://127.0.0.1:3002".into(),
                bind: "127.0.0.1:3001".parse().unwrap(),
                production: false,
                auth_database: String::new(),
                staff_database: String::new(),
                idle_timeout: Duration::from_secs(900),
            },
            auth: pool("AUTH_DATABASE_URL").await,
            staff: pool("STAFF_DATABASE_URL").await,
            webauthn: WebauthnBuilder::new("localhost", &origin)
                .unwrap()
                .build()
                .unwrap(),
            limits: Limits::default(),
        });
        Self {
            owner,
            state,
            board,
            post,
            report,
            asset,
            account,
            token,
            csrf,
        }
    }
    async fn request(&self, board: &str, csrf: &str, token: &str) -> StatusCode {
        let response = router(self.state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/moderate")
                    .header("origin", "http://localhost:3001")
                    .header("sec-fetch-site", "same-origin")
                    .header("cookie", format!("staff={token}"))
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(format!(
                        "csrf={csrf}&board={board}&target={}&action=remove-file",
                        self.post
                    )))
                    .unwrap(),
            )
            .await
            .unwrap();
        response.status()
    }
    async fn html(&self) -> String {
        let response = router(self.state.clone())
            .oneshot(
                Request::builder()
                    .uri("/reports")
                    .header(
                        "cookie",
                        format!("staff={}; staff-csrf={}", self.token, self.csrf),
                    )
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        assert!(
            response.headers()["content-security-policy"]
                .to_str()
                .unwrap()
                .ends_with("img-src http://127.0.0.1:3002")
        );
        let html = String::from_utf8(
            response
                .into_body()
                .collect()
                .await
                .unwrap()
                .to_bytes()
                .to_vec(),
        )
        .unwrap();
        let article = html
            .split(&format!("<article id=\"report-{}\">", self.report))
            .nth(1)
            .unwrap();
        article.split("</article>").next().unwrap().to_string()
    }
    async fn unchanged(&self) {
        let state: (bool, bool, bool, String, i64) = sqlx::query_as("SELECT m.file_deleted,p.deleted,t.deleted,p.comment,(SELECT count(*) FROM content.moderation_audit WHERE board=$1) FROM content.post_media m JOIN content.posts p ON p.id=m.post_id JOIN content.threads t ON t.id=p.thread_id WHERE p.id=$2")
            .bind(&self.board).bind(self.post).fetch_one(&self.owner).await.unwrap();
        assert_eq!(state, (false, false, false, "Keep this text".into(), 0));
    }
    async fn cleanup(&self) {
        for sql in [
            "DELETE FROM content.moderation_audit WHERE board=$1",
            "DELETE FROM content.reports WHERE board=$1",
            "DELETE FROM content.post_media WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
            "DELETE FROM content.posts WHERE board=$1",
            "DELETE FROM content.threads WHERE board=$1",
            "DELETE FROM content.boards WHERE slug=$1",
        ] {
            sqlx::query(sql)
                .bind(&self.board)
                .execute(&self.owner)
                .await
                .unwrap();
        }
        sqlx::query("DELETE FROM media.assets WHERE id=$1")
            .bind(&self.asset)
            .execute(&self.owner)
            .await
            .unwrap();
        for sql in [
            "DELETE FROM staff_identity.sessions WHERE account_id=$1",
            "DELETE FROM staff_identity.credentials WHERE account_id=$1",
            "DELETE FROM staff_identity.accounts WHERE id=$1",
        ] {
            sqlx::query(sql)
                .bind(self.account)
                .execute(&self.owner)
                .await
                .unwrap();
        }
    }
}

#[tokio::test]
async fn staff_attachment_preview_and_file_only_action_enforce_real_authority() {
    let f = Arc::new(Fixture::new().await);
    let exercise = f.clone();
    let result = tokio::spawn(async move { exercise_attachment(&exercise).await }).await;
    f.cleanup().await;
    result.unwrap();
}

async fn exercise_attachment(f: &Fixture) {
    let html = f.html().await;
    assert!(!html.contains("<img src=x"));
    assert!(html.contains("&#60;img") || html.contains("&lt;img"));
    assert!(html.contains("width=\"250\" height=\"150\""));
    assert!(html.contains("referrerpolicy=\"no-referrer\""));
    assert!(
        !html.contains(&f.asset),
        "No opaque job or asset identity in staff view"
    );
    let public = pool("TEST_PUBLIC_DATABASE_URL").await;
    let fields: Vec<String> = sqlx::query_scalar("SELECT attname::text FROM pg_attribute WHERE attrelid='content.staff_post_media'::regclass AND attnum>0 AND NOT attisdropped ORDER BY attnum")
        .fetch_all(&f.owner).await.unwrap();
    assert_eq!(
        fields,
        [
            "post_id",
            "filename",
            "bytes",
            "width",
            "height",
            "spoiler",
            "tim",
            "thumbnail_width",
            "thumbnail_height",
            "available"
        ]
    );
    let privileges: (bool,bool,bool,bool) = sqlx::query_as("SELECT has_table_privilege(current_user,'content.staff_post_media','SELECT'),has_table_privilege(current_user,'content.staff_post_media','INSERT'),has_table_privilege(current_user,'content.staff_post_media','UPDATE'),has_table_privilege(current_user,'content.staff_post_media','DELETE')")
        .fetch_one(&f.state.staff).await.unwrap();
    assert_eq!(privileges, (true, false, false, false));
    for (pool, sql) in [
        (&public, "SELECT * FROM content.staff_post_media LIMIT 0"),
        (
            &f.state.auth,
            "SELECT * FROM content.staff_post_media LIMIT 0",
        ),
        (&f.state.staff, "SELECT * FROM content.post_media LIMIT 0"),
        (&f.state.staff, "SELECT * FROM media.assets LIMIT 0"),
        (
            &f.state.staff,
            "UPDATE content.post_media SET filename='changed' WHERE false",
        ),
    ] {
        let error = sqlx::query(sql).execute(pool).await.unwrap_err();
        assert_eq!(
            error.as_database_error().unwrap().code().as_deref(),
            Some("42501")
        );
    }
    assert_eq!(
        f.request(&f.board, "invalid", &f.token).await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        f.request("other", &f.csrf, &f.token).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        f.request(&f.board, &f.csrf, &auth::token()).await,
        StatusCode::UNAUTHORIZED
    );
    for statement in [
        "UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp()-interval '11 minutes' WHERE account_id=$1",
        "UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp(),expires_at=clock_timestamp()-interval '1 second' WHERE account_id=$1",
        "UPDATE staff_identity.sessions SET expires_at=clock_timestamp()+interval '1 hour',last_activity_at=clock_timestamp()-interval '16 minutes' WHERE account_id=$1",
    ] {
        sqlx::query(statement)
            .bind(f.account)
            .execute(&f.owner)
            .await
            .unwrap();
        let expected = if statement.contains("11 minutes") {
            StatusCode::FORBIDDEN
        } else {
            StatusCode::UNAUTHORIZED
        };
        assert_eq!(f.request(&f.board, &f.csrf, &f.token).await, expected);
        f.unchanged().await;
    }
    sqlx::query(
        "UPDATE staff_identity.sessions SET last_activity_at=clock_timestamp() WHERE account_id=$1",
    )
    .bind(f.account)
    .execute(&f.owner)
    .await
    .unwrap();
    sqlx::query("UPDATE staff_identity.accounts SET revoked_at=clock_timestamp() WHERE id=$1")
        .bind(f.account)
        .execute(&f.owner)
        .await
        .unwrap();
    assert_eq!(
        f.request(&f.board, &f.csrf, &f.token).await,
        StatusCode::UNAUTHORIZED
    );
    sqlx::query("UPDATE staff_identity.accounts SET revoked_at=NULL WHERE id=$1")
        .bind(f.account)
        .execute(&f.owner)
        .await
        .unwrap();
    let mut session = auth::Session {
        account_id: f.account,
        role: "unknown".into(),
        csrf_hash: vec![],
        recent: true,
    };
    assert!(matches!(
        store::moderate(&f.state.staff, &session, &f.board, f.post, "remove-file").await,
        Err(board_staff::AppError::Forbidden)
    ));
    session.role = "moderator".into();
    f.unchanged().await;
    interrupted_audit_rolls_back_file_removal(f).await;
    sqlx::query("UPDATE content.post_media SET spoiler=true WHERE post_id=$1")
        .bind(f.post)
        .execute(&f.owner)
        .await
        .unwrap();
    let html = f.html().await;
    assert!(html.contains("Open spoiler image"));
    assert!(!html.contains("<img "));
    let before: chrono::DateTime<chrono::Utc> =
        sqlx::query_scalar("SELECT modified_at FROM content.threads WHERE id=$1")
            .bind(f.post)
            .fetch_one(&f.owner)
            .await
            .unwrap();
    let (one, two) = tokio::join!(
        f.request(&f.board, &f.csrf, &f.token),
        f.request(&f.board, &f.csrf, &f.token)
    );
    assert!(
        (one == StatusCode::SEE_OTHER && two == StatusCode::NOT_FOUND)
            || (two == StatusCode::SEE_OTHER && one == StatusCode::NOT_FOUND)
    );
    let state: (bool, bool, bool, String, chrono::DateTime<chrono::Utc>) = sqlx::query_as("SELECT m.file_deleted,p.deleted,t.deleted,p.comment,t.modified_at FROM content.post_media m JOIN content.posts p ON p.id=m.post_id JOIN content.threads t ON t.id=p.thread_id WHERE p.id=$1")
        .bind(f.post).fetch_one(&f.owner).await.unwrap();
    assert_eq!(
        (state.0, state.1, state.2, state.3),
        (true, false, false, "Keep this text".into())
    );
    assert!(state.4 > before);
    let audit: Vec<(i64, i64, String)> = sqlx::query_as(
        "SELECT account_id,target_id,action FROM content.moderation_audit WHERE board=$1",
    )
    .bind(&f.board)
    .fetch_all(&f.owner)
    .await
    .unwrap();
    assert_eq!(audit, vec![(f.account, f.post, "remove-file".into())]);
    let reader = pool("MEDIA_READ_DATABASE_URL").await;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM media.approved_assets WHERE id=$1")
        .bind(&f.asset)
        .fetch_one(&reader)
        .await
        .unwrap();
    assert_eq!(count, 0);
    let html = f.html().await;
    assert!(html.contains("File unavailable"));
    assert!(!html.contains("<img "));
    assert!(!html.contains("Open spoiler"));
    assert!(!html.contains("Remove file only"));
    assert!(html.contains("Keep this text"));
    store::moderate(&f.state.staff, &session, &f.board, f.post, "remove-thread")
        .await
        .unwrap();
    assert!(
        f.html().await.contains("File unavailable"),
        "Removed post metadata remains reviewable"
    );
}

async fn interrupted_audit_rolls_back_file_removal(f: &Fixture) {
    let mut blocker = f.owner.begin().await.unwrap();
    sqlx::query("LOCK TABLE content.moderation_audit IN SHARE MODE")
        .execute(&mut *blocker)
        .await
        .unwrap();
    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *blocker)
        .await
        .unwrap();
    let staff = f.state.staff.clone();
    let board = f.board.clone();
    let post = f.post;
    let account = f.account;
    let change = tokio::spawn(async move {
        let session = auth::Session {
            account_id: account,
            role: "moderator".into(),
            csrf_hash: vec![],
            recent: true,
        };
        store::moderate(&staff, &session, &board, post, "remove-file").await
    });
    let mut waiting = false;
    for _ in 0..200 {
        waiting = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM pg_stat_activity WHERE usename='board_staff' AND $1=ANY(pg_blocking_pids(pid)))")
            .bind(pid).fetch_one(&mut *blocker).await.unwrap();
        if waiting {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // The delete function has run, but neither its tombstone nor its timestamp
    // can become visible before the append-only audit insertion commits.
    f.unchanged().await;
    change.abort();
    let stopped = change.await;
    blocker.rollback().await.unwrap();
    assert!(waiting, "Moderation never reached the audit lock");
    assert!(stopped.unwrap_err().is_cancelled());
    f.unchanged().await;
}
