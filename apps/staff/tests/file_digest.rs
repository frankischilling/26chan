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
    async fn new(digest: Option<&str>) -> Self {
        Self::new_with_format(digest, "png").await
    }
    async fn new_with_format(digest: Option<&str>, output_format: &str) -> Self {
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
        sqlx::query("INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at,md5,thumbnail_sha256,thumbnail_bytes,thumbnail_width,thumbnail_height,output_format) VALUES ($1,$1,$1,repeat('a',64),100,500,300,'approved',clock_timestamp(),$2,CASE WHEN $2::text IS NOT NULL THEN repeat('c',64) END,CASE WHEN $2::text IS NOT NULL THEN 50 END,CASE WHEN $2::text IS NOT NULL THEN 250 END,CASE WHEN $2::text IS NOT NULL THEN 150 END,$3)")
            .bind(&asset).bind(digest).bind(output_format).execute(&owner).await.unwrap();
        sqlx::query("INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler) VALUES ($1,$2,$2,$3,100,500,300,false)")
            .bind(post).bind(&asset).bind("<img src=x onerror=alert(1)> & \"name\".png").execute(&owner).await.unwrap();
        // Parallel cases deliberately reuse digest bytes. Keep each report
        // queue scoped to its fixture instead of observing another case's rows.
        let account: i64 = sqlx::query_scalar(
            "INSERT INTO staff_identity.accounts(role,allow_boards) VALUES ('moderator',ARRAY[$1]) RETURNING id",
        )
        .bind(&board)
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
                media: None,
                proxy: None,
                poster_id_key: None,
                country_database: None,
                origin: origin.origin().ascii_serialization(),
                public_origin: "http://localhost:3000".into(),
                media_origin: "http://127.0.0.1:3002".into(),
                bind: "127.0.0.1:3001".parse().unwrap(),
                production: false,
                auth_database: String::new(),
                staff_database: String::new(),
                idle_timeout: Duration::from_secs(900),
                tripcode_key: None,
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
    async fn html(&self, authenticated: bool) -> (StatusCode, String) {
        let mut request = Request::builder().uri("/reports");
        if authenticated {
            request = request.header(
                "cookie",
                format!("staff={}; staff-csrf={}", self.token, self.csrf),
            );
        }
        let response = router(self.state.clone())
            .oneshot(request.body(Body::empty()).unwrap())
            .await
            .unwrap();
        let status = response.status();
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
        (status, html)
    }
    async fn digest(&self) -> (bool, Option<String>) {
        sqlx::query_as("SELECT available,md5 FROM content.staff_post_media WHERE post_id=$1")
            .bind(self.post)
            .fetch_one(&self.state.staff)
            .await
            .unwrap()
    }
    async fn archived(&self) {
        sqlx::query("UPDATE content.boards SET archive_retention_seconds=86400 WHERE slug=$1")
            .bind(&self.board)
            .execute(&self.owner)
            .await
            .unwrap();
        sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
            .bind(self.post).execute(&self.owner).await.unwrap();
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

const DIGEST: &str = "00112233445566778899aabbccddeeff";

#[tokio::test]
async fn approved_gif_format_controls_staff_original_link_and_keeps_thumbnail_route() {
    let f = Arc::new(Fixture::new_with_format(Some(DIGEST), "gif").await);
    let exercise = f.clone();
    let result = tokio::spawn(async move {
        let f = exercise;
        let format: String = sqlx::query_scalar(
            "SELECT output_format FROM content.staff_post_media WHERE post_id=$1",
        )
        .bind(f.post)
        .fetch_one(&f.state.staff)
        .await
        .unwrap();
        assert_eq!(format, "gif");

        let session = auth::Session {
            account_id: f.account,
            role: "moderator".into(),
            csrf_hash: vec![],
            recent: true,
            permissions: board_staff::access::Permissions {
                allow_boards: vec![f.board.clone()],
                ..Default::default()
            },
        };
        let reports = store::reports(&f.state.staff, &session).await.unwrap();
        let attachment = reports
            .iter()
            .find(|report| report.id == f.report)
            .and_then(|report| report.attachment.as_ref())
            .unwrap();
        assert_eq!(attachment.output_format.extension(), "gif");
        let tim = attachment.tim;

        let (status, html) = f.html(true).await;
        assert_eq!(status, StatusCode::OK);
        let article = html
            .split(&format!("<article id=\"report-{}\">", f.report))
            .nth(1)
            .unwrap()
            .split("</article>")
            .next()
            .unwrap();
        let original = format!("http://127.0.0.1:3002/{}/{tim}.gif", f.board);
        let wrong_original = format!("http://127.0.0.1:3002/{}/{tim}.png", f.board);
        let thumbnail = format!("http://127.0.0.1:3002/{}/{tim}s.jpg", f.board);
        assert!(article.contains(&format!("href=\"{original}\"")));
        assert!(!article.contains(&wrong_original));
        assert!(article.contains(&format!("src=\"{thumbnail}\"")));
    })
    .await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn file_md5_is_read_only_scoped_and_consistent_for_duplicate_reports() {
    let f = Arc::new(Fixture::new(Some(DIGEST)).await);
    const OTHER_DIGEST: &str = "ffeeddccbbaa99887766554433221100";
    let other = Arc::new(Fixture::new(Some(OTHER_DIGEST)).await);
    other.archived().await;
    let exercise = f.clone();
    let other_exercise = other.clone();
    let result = tokio::spawn(async move {
        let f = exercise;
        let other = other_exercise;
        assert_eq!(f.digest().await, (true, Some(DIGEST.into())));
        let (status, html) = f.html(true).await;
        assert_eq!(status, StatusCode::OK);
        assert!(!html.contains("<summary>File MD5</summary>"));
        assert!(!html.contains(&format!("<article id=\"report-{}\">", other.report)));
        assert!(!html.contains(OTHER_DIGEST));
        f.archived().await;
        let duplicate: i64 = sqlx::query_scalar("INSERT INTO content.reports(board,post_id,reason) VALUES ($1,$2,'Duplicate synthetic') RETURNING id")
            .bind(&f.board).bind(f.post).fetch_one(&f.owner).await.unwrap();
        let (status, html) = f.html(true).await;
        assert_eq!(status, StatusCode::OK);
        for report in [f.report, duplicate] {
            let article = html.split(&format!("<article id=\"report-{report}\">"))
                .nth(1).unwrap().split("</article>").next().unwrap();
            assert!(article.contains("<summary>File MD5</summary>"));
            assert!(article.contains(&format!("Approved normalized file MD5: <code>{DIGEST}</code>")));
            assert!(!article.contains("<script"));
        }
        let public_md5: String = sqlx::query_scalar("SELECT md5 FROM content.visible_post_media WHERE post_id=$1")
            .bind(f.post).fetch_one(&f.owner).await.unwrap();
        assert_eq!(public_md5, "ABEiM0RVZneImaq7zN3u/w==");
        let session = auth::Session {
            account_id: f.account, role: "moderator".into(), csrf_hash: vec![], recent: true,
            permissions: board_staff::access::Permissions {
                allow_boards: vec![f.board.clone()],
                ..Default::default()
            },
        };
        let reports = store::reports(&f.state.staff, &session).await.unwrap();
        assert_eq!(reports.len(), 2);
        for report in &reports {
            assert_eq!(report.post_id, f.post);
            assert_eq!(report.attachment.as_ref().unwrap().file_md5(), Some(DIGEST));
        }
        let (status, html) = f.html(false).await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
        assert!(!html.contains(DIGEST));
        // Deny must win over an explicit allow without hiding reports from
        // the other allowed board. This also makes the scope check non-vacuous.
        sqlx::query("UPDATE staff_identity.accounts SET allow_boards=ARRAY[$2,$3],deny_boards=ARRAY[$2] WHERE id=$1")
            .bind(f.account).bind(&f.board).bind(&other.board).execute(&f.owner).await.unwrap();
        let (status, html) = f.html(true).await;
        assert_eq!(status, StatusCode::OK);
        for report in [f.report, duplicate] {
            assert!(!html.contains(&format!("<article id=\"report-{report}\">")));
        }
        assert!(!html.contains(DIGEST));
        let other_article = html.split(&format!("<article id=\"report-{}\">", other.report))
            .nth(1).unwrap().split("</article>").next().unwrap();
        assert!(other_article.contains("<summary>File MD5</summary>"));
        assert!(other_article.contains(OTHER_DIGEST));
        let state: (bool, bool, i64) = sqlx::query_as("SELECT m.file_deleted,p.deleted,(SELECT count(*) FROM content.moderation_audit WHERE board=$1) FROM content.post_media m JOIN content.posts p ON p.id=m.post_id WHERE p.id=$2")
            .bind(&f.board).bind(f.post).fetch_one(&f.owner).await.unwrap();
        assert_eq!(state, (false, false, 0));
    }).await;
    f.cleanup().await;
    other.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn file_md5_never_exposes_missing_or_unavailable_metadata() {
    for case in [
        "legacy",
        "removed",
        "deleted-post",
        "deleted-thread",
        "expired",
        "archive-disabled",
        "pending",
        "deleting",
        "missing-asset",
    ] {
        let f = Arc::new(Fixture::new((case != "legacy").then_some(DIGEST)).await);
        let exercise = f.clone();
        let result = tokio::spawn(async move {
            let f = exercise;
            f.archived().await;
            let sql = match case {
                "legacy" => None,
                "removed" => Some("UPDATE content.post_media SET file_deleted=true WHERE post_id=$1"),
                "deleted-post" => Some("UPDATE content.posts SET deleted=true WHERE id=$1"),
                "deleted-thread" => Some("UPDATE content.threads SET deleted=true WHERE id=$1"),
                "expired" => Some("UPDATE content.threads SET archived_at=clock_timestamp()-interval '2 hours',archive_expires_at=clock_timestamp()-interval '1 hour' WHERE id=$1"),
                _ => None,
            };
            if let Some(sql) = sql {
                sqlx::query(sql).bind(f.post).execute(&f.owner).await.unwrap();
            }
            match case {
                "archive-disabled" => { sqlx::query("UPDATE content.boards SET archive_retention_seconds=0 WHERE slug=$1").bind(&f.board).execute(&f.owner).await.unwrap(); }
                "pending" | "deleting" => { sqlx::query("UPDATE media.assets SET state=$2,approved_at=NULL WHERE id=$1").bind(&f.asset).bind(case).execute(&f.owner).await.unwrap(); }
                "missing-asset" => { sqlx::query("DELETE FROM media.assets WHERE id=$1").bind(&f.asset).execute(&f.owner).await.unwrap(); }
                _ => {}
            }
            assert_eq!(f.digest().await, (case == "legacy", None), "{case}");
            let (status, html) = f.html(true).await;
            assert_eq!(status, StatusCode::OK);
            let marker = format!("<article id=\"report-{}\">", f.report);
            if matches!(case, "deleted-post" | "deleted-thread") {
                assert!(!html.contains(&marker), "{case}: whole deletion removes the report article");
                assert!(!html.contains(DIGEST), "{case}");
                let exists: bool = sqlx::query_scalar(
                    "SELECT EXISTS(SELECT 1 FROM content.reports WHERE board=$1 AND id=$2 AND post_id=$3)",
                )
                .bind(&f.board)
                .bind(f.report)
                .bind(f.post)
                .fetch_one(&f.owner)
                .await
                .unwrap();
                assert!(!exists, "{case}: whole deletion physically removes the report row");
            } else {
                let article = html.split(&marker)
                    .nth(1).expect("The surviving report must have a queue article")
                    .split("</article>").next().unwrap();
                assert!(!article.contains("<summary>File MD5</summary>"), "{case}");
                assert!(!article.contains(DIGEST), "{case}");
            }
        }).await;
        f.cleanup().await;
        result.unwrap();
    }
}

#[tokio::test]
async fn file_digest_expires_by_wall_clock_inside_an_older_transaction() {
    let f = Arc::new(Fixture::new(Some(DIGEST)).await);
    let exercise = f.clone();
    let result = tokio::spawn(async move {
        let f = exercise;
        f.archived().await;
        let mut tx = f.owner.begin().await.unwrap();
        sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
            .execute(&mut *tx)
            .await
            .unwrap();
        let projection = "SELECT available,md5 FROM content.staff_post_media WHERE post_id=$1";
        let before: (bool, Option<String>) = sqlx::query_as(projection)
            .bind(f.post)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(before, (true, Some(DIGEST.into())));
        // Put expiry between the transaction's start and the next statement's
        // wall clock. Own writes stay visible in REPEATABLE READ; no sleep or
        // competing transaction is needed to establish the two clock bounds.
        let after_start: bool = sqlx::query_scalar("UPDATE content.threads SET archived_at=transaction_timestamp()-interval '1 hour',archive_expires_at=clock_timestamp() WHERE id=$1 RETURNING archive_expires_at>transaction_timestamp()")
            .bind(f.post).fetch_one(&mut *tx).await.unwrap();
        assert!(after_start);
        let expired: bool = sqlx::query_scalar("SELECT archive_expires_at<clock_timestamp() FROM content.visible_threads WHERE id=$1")
            .bind(f.post).fetch_one(&mut *tx).await.unwrap();
        assert!(expired);
        let after: (bool, Option<String>) = sqlx::query_as(projection)
            .bind(f.post)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert_eq!(after, (true, None));
        tx.rollback().await.unwrap();
    })
    .await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn file_digest_view_keeps_restricted_grants_barrier_and_approved_format() {
    let owner = pool("MIGRATION_DATABASE_URL").await;
    let mut tx = owner.begin().await.unwrap();
    // The historical 0107 upgrade is exercised on its actual predecessor by
    // test-staff-file-digest-migration.sh. Replaying it here would remove the
    // approved-format column appended by 0123, which PostgreSQL rejects.
    let (view_owner, options): (String, Vec<String>) = sqlx::query_as("SELECT relowner::regrole::text,reloptions FROM pg_class WHERE oid='content.staff_post_media'::regclass")
        .fetch_one(&mut *tx).await.unwrap();
    assert_eq!(view_owner, "board_migrator");
    assert!(options.contains(&"security_barrier=true".into()));
    let columns: Vec<String> = sqlx::query_scalar("SELECT attname::text FROM pg_attribute WHERE attrelid='content.staff_post_media'::regclass AND attnum>0 AND NOT attisdropped ORDER BY attnum")
        .fetch_all(&mut *tx).await.unwrap();
    assert_eq!(
        columns,
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
            "available",
            "md5",
            "output_format"
        ]
    );
    let privileges: (bool, bool, bool, bool) = sqlx::query_as("SELECT has_table_privilege('board_staff','content.staff_post_media','SELECT'),has_any_column_privilege('board_staff','media.assets','SELECT'),EXISTS(SELECT 1 FROM pg_class c,aclexplode(c.relacl) acl WHERE c.oid='content.staff_post_media'::regclass AND acl.grantee=0),EXISTS(SELECT 1 FROM pg_attribute a CROSS JOIN LATERAL aclexplode(a.attacl) acl WHERE a.attrelid='content.staff_post_media'::regclass AND a.attnum>0 AND NOT a.attisdropped AND acl.grantee=0)")
        .fetch_one(&mut *tx).await.unwrap();
    assert_eq!(privileges, (true, false, false, false));
    tx.rollback().await.unwrap();
}
