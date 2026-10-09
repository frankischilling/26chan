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

// Source forcearchive (imgboard.php:9144–9206) and archive_thread (1746–1865).
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
    async fn ready(&self) {
        auth::check_identity(&self.auth, "board_auth")
            .await
            .unwrap();
        auth::check_identity(&self.staff, "board_staff")
            .await
            .unwrap();
        self.role("moderator", &self.boards[..1], &[]).await;
        sqlx::query("UPDATE content.boards SET archive_retention_seconds=3600 WHERE slug=ANY($1)")
            .bind(self.boards.to_vec())
            .execute(&self.owner)
            .await
            .unwrap();
    }

    async fn action(&self, target: i64, action: &str) -> StatusCode {
        self.request(
            "/moderate",
            Some(format!(
                "csrf={}&board={}&target={target}&action={action}",
                self.csrf, self.boards[0]
            )),
        )
        .await
        .0
    }

    async fn saved(&self, target: i64) -> serde_json::Value {
        let raw: String = sqlx::query_scalar("SELECT jsonb_build_object('snapshot_version',1,'snapshot_name',p.name,'snapshot_trip',p.trip,'snapshot_capcode',p.capcode,'snapshot_subject',p.subject,'snapshot_comment',p.comment,'snapshot_comment_format',p.comment_format,'snapshot_staff_authorized_limits',p.staff_authorized_limits,'snapshot_wordfiltered',p.wordfilter_payload IS NOT NULL,'snapshot_image_spoiler',p.image_spoiler,'snapshot_filename',m.filename,'snapshot_dice_result',p.dice_result,'snapshot_fortune_text',p.fortune_text,'snapshot_fortune_color',p.fortune_color)::text FROM content.posts p LEFT JOIN content.post_media m ON m.post_id=p.id WHERE p.id=$1")
            .bind(target).fetch_one(&self.owner).await.unwrap();
        serde_json::from_str(&raw).unwrap()
    }

    async fn audits(&self) -> Vec<serde_json::Value> {
        let rows: Vec<String> = sqlx::query_scalar(
            "SELECT to_jsonb(a)::text FROM content.moderation_audit a WHERE board=$1 ORDER BY id",
        )
        .bind(&self.boards[0])
        .fetch_all(&self.owner)
        .await
        .unwrap();
        rows.into_iter()
            .map(|row| serde_json::from_str(&row).unwrap())
            .collect()
    }

    async fn state(&self) -> serde_json::Value {
        let raw: String = sqlx::query_scalar("SELECT jsonb_build_object('thread',to_jsonb(t),'posts',(SELECT jsonb_agg(to_jsonb(p) ORDER BY p.id) FROM content.posts p WHERE p.thread_id=t.id),'media',(SELECT jsonb_agg(to_jsonb(m) ORDER BY m.post_id) FROM content.post_media m JOIN content.posts p ON p.id=m.post_id WHERE p.thread_id=t.id))::text FROM content.threads t WHERE t.id=$1")
            .bind(self.posts[0]).fetch_one(&self.owner).await.unwrap();
        serde_json::from_str(&raw).unwrap()
    }

    async fn rich_post(&self) {
        sqlx::query("UPDATE content.posts SET name='',trip='!!AbCdEf012+/',capcode='admin_highlight',subject='<b>saved & subject</b>',comment='[code]stored & <script> text[/code]',comment_format=127,staff_authorized_limits=true,dice_result='3d6: 2 + 4 + 6 = 12',fortune_text=NULL,fortune_color=NULL WHERE id=$1")
            .bind(self.posts[0]).execute(&self.owner).await.unwrap();
        // Filename survives deletion; no real bytes or media jobs are required.
        let asset = uuid::Uuid::new_v4().simple().to_string();
        sqlx::query("INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler,file_deleted) VALUES($1,$2,$2,'<removed & name>.png',100,10,10,false,true)")
            .bind(self.posts[0]).bind(asset).execute(&self.owner).await.unwrap();
    }
}

fn assert_snapshot(row: &serde_json::Value, expected: &serde_json::Value) {
    let actual: serde_json::Map<String, serde_json::Value> = row
        .as_object()
        .unwrap()
        .iter()
        .filter(|(key, _)| key.starts_with("snapshot_"))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    assert_eq!(&serde_json::Value::Object(actual), expected);
    for forbidden in [
        "wordfilter_payload",
        "wordfilter_search",
        "session",
        "token",
        "sha256",
        "md5",
        "ip",
        "asset_id",
        "job_id",
    ] {
        assert!(
            !row.as_object()
                .unwrap()
                .keys()
                .any(|key| key == forbidden || key == &format!("snapshot_{forbidden}"))
        );
    }
}

fn timestamp(state: &serde_json::Value, field: &str) -> chrono::DateTime<chrono::FixedOffset> {
    chrono::DateTime::parse_from_rfc3339(state["thread"][field].as_str().unwrap()).unwrap()
}

#[tokio::test]
async fn scoped_moderator_archives_old_undead_thread_with_exact_snapshot_once() {
    let f = Fixture::new().await;
    let result = tokio::spawn({ let f = f.clone(); async move {
        f.ready().await;
        f.rich_post().await;
        sqlx::query("UPDATE content.threads SET created_at=clock_timestamp()-interval '72 hours',bumped_at=clock_timestamp()-interval '71 hours',modified_at=clock_timestamp()-interval '71 hours',undead=true,permaage=true,permasage=true WHERE id=$1")
            .bind(f.posts[0]).execute(&f.owner).await.unwrap();
        // Retained report evidence renders the saved malicious comment through
        // the ordinary queue view after the archive transition.
        sqlx::query("INSERT INTO content.reports(board,post_id,reason) VALUES($1,$2,'Owned force archive preview')")
            .bind(&f.boards[0]).bind(f.posts[0]).execute(&f.owner).await.unwrap();
        let (status, controls) = f.request("/reports", None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(controls.contains("Archive thread"));
        let before = f.state().await;
        let saved = f.saved(f.posts[0]).await;
        let start: chrono::DateTime<chrono::Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
            .fetch_one(&f.owner).await.unwrap();
        let (a,b) = tokio::join!(f.action(f.posts[0],"force-archive"),f.action(f.posts[0],"force-archive"));
        assert!((a==StatusCode::SEE_OTHER && b==StatusCode::BAD_REQUEST) || (b==StatusCode::SEE_OTHER && a==StatusCode::BAD_REQUEST), "concurrent archive outcomes: {a}, {b}");
        let end: chrono::DateTime<chrono::Utc> = sqlx::query_scalar("SELECT clock_timestamp()")
            .fetch_one(&f.owner).await.unwrap();
        let after = f.state().await;
        assert_eq!(after["thread"]["closed"],true);
        assert_eq!(after["thread"]["sticky"],false);
        let archived = timestamp(&after,"archived_at");
        assert!(archived>=start && archived<=end);
        assert_eq!(timestamp(&after,"archive_expires_at")-archived,chrono::Duration::seconds(3600));
        assert_eq!(timestamp(&after,"bumped_at"),archived);
        assert_eq!(timestamp(&after,"modified_at"),archived);
        assert!(timestamp(&after,"http_modified_at")>timestamp(&before,"http_modified_at"));
        let mut unchanged = after.clone();
        for field in ["closed","archived_at","archive_expires_at","bumped_at","modified_at","http_modified_at"] {
            unchanged["thread"][field]=before["thread"][field].clone();
        }
        assert_eq!(unchanged,before,"all unrelated flags, posts, and media remain unchanged");
        let rows = f.audits().await;
        assert_eq!(rows.len(),1);
        assert_snapshot(&rows[0],&saved);
        assert_eq!(rows[0]["action"],"force-archive");
        assert_eq!(rows[0]["target_id"],f.posts[0]);
        assert_eq!(rows[0]["account_id"],f.account);
        assert!(rows[0]["before_mask"].is_null() && rows[0]["after_mask"].is_null());
        assert_eq!(f.action(f.posts[0],"force-archive").await,StatusCode::BAD_REQUEST);
        assert_eq!(f.state().await,after);
        assert_eq!(f.audits().await,rows);
        let (status,html)=f.request("/reports",None).await;
        assert_eq!(status,StatusCode::OK);
        // Askama uses numeric HTML entities. Assert the complete typed code
        // rendering, not just absence of an executable script fragment.
        assert!(html.contains("<pre class=\"prettyprint\">stored &#38; &#60;script&#62; text</pre>"),"saved comment must use the normal escaped code rendering");
        assert!(html.contains("<h3>&#60;b&#62;saved &#38; subject&#60;/b&#62;</h3>"));
        assert!(!html.contains("Archive thread"));
        assert!(!html.contains("<script> text"));
        assert!(!html.contains("<b>saved & subject</b>"));
    }}).await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn force_archive_rejects_invalid_targets_and_disabled_policy_without_changes() {
    // Fresh whole deletion is irreversible. Each target-state rejection owns
    // independent rows instead of resurrecting a previously deleted fixture.
    for (statement, expected) in [
        (
            "UPDATE content.threads SET sticky=true WHERE id=$1",
            StatusCode::BAD_REQUEST,
        ),
        (
            "UPDATE content.threads SET deleted=true WHERE id=$1",
            StatusCode::NOT_FOUND,
        ),
        (
            "UPDATE content.posts SET deleted=true WHERE id=$1",
            StatusCode::NOT_FOUND,
        ),
    ] {
        let f = Fixture::new().await;
        let result = tokio::spawn({
            let f = f.clone();
            async move {
                f.ready().await;
                sqlx::query(statement)
                    .bind(f.posts[0])
                    .execute(&f.owner)
                    .await
                    .unwrap();
                let before = f.state().await;
                assert_eq!(f.action(f.posts[0], "force-archive").await, expected);
                assert_eq!(f.state().await, before);
                assert_eq!(f.audit_count().await, 0);
            }
        })
        .await;
        f.cleanup().await;
        result.unwrap();
    }
    let f = Fixture::new().await;
    let result=tokio::spawn({let f=f.clone();async move {
        f.ready().await;
        let reply:i64=sqlx::query_scalar("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,$2,'Anonymous','','Owned archive reply') RETURNING id")
            .bind(&f.boards[0]).bind(f.posts[0]).fetch_one(&f.owner).await.unwrap();
        for target in [reply,i64::MAX] {
            let before=f.state().await;
            assert_eq!(f.action(target,"force-archive").await,StatusCode::NOT_FOUND);
            assert_eq!(f.state().await,before);
        }
        sqlx::query("UPDATE content.boards SET archive_retention_seconds=0 WHERE slug=$1").bind(&f.boards[0]).execute(&f.owner).await.unwrap();
        let before=f.state().await;
        for target in [f.posts[0],i64::MAX] {
            assert_eq!(f.action(target,"force-archive").await,StatusCode::BAD_REQUEST);
        }
        // Disabled policy wins without waiting for either eligible target row.
        for post_lock in [false,true] {
            let mut lock=f.owner.begin().await.unwrap();
            let sql=if post_lock {"SELECT id FROM content.posts WHERE id=$1 FOR UPDATE"} else {"SELECT id FROM content.threads WHERE id=$1 FOR UPDATE"};
            sqlx::query(sql).bind(f.posts[0]).execute(&mut *lock).await.unwrap();
            let denied=tokio::time::timeout(Duration::from_secs(1),f.action(f.posts[0],"force-archive")).await;
            lock.rollback().await.unwrap();
            assert_eq!(denied.expect("disabled archive must not wait for target"),StatusCode::BAD_REQUEST);
        }
        assert_eq!(f.state().await,before);
        assert_eq!(f.audit_count().await,0);
    }}).await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn force_archive_requires_current_scoped_moderator_origin_and_csrf() {
    let f = Fixture::new().await;
    let result=tokio::spawn({let f=f.clone();async move {
        f.ready().await;
        let before=f.state().await;
        for (role,allow,deny) in [
            ("janitor",f.boards[..1].to_vec(),vec![]),
            ("moderator",f.boards[1..].to_vec(),vec![]),
            ("moderator",vec!["all".into()],f.boards[..1].to_vec()),
        ] {
            f.role(role,&allow,&deny).await;
            assert_eq!(f.action(f.posts[0],"force-archive").await,StatusCode::FORBIDDEN);
            assert_eq!(f.state().await,before);
        }
        f.ready().await;
        for (origin,site,csrf,cookie,status) in [
            ("http://evil.invalid","same-origin",true,true,StatusCode::FORBIDDEN),
            ("http://localhost:3001","cross-site",true,true,StatusCode::FORBIDDEN),
            ("http://localhost:3001","same-origin",false,true,StatusCode::FORBIDDEN),
            ("http://localhost:3001","same-origin",true,false,StatusCode::UNAUTHORIZED),
        ] {
            let mut req=Request::post("/moderate").header("content-type","application/x-www-form-urlencoded").header("origin",origin).header("sec-fetch-site",site);
            if cookie {req=req.header("cookie",format!("staff={}; staff-csrf={}",f.token,f.csrf));}
            let form=format!("csrf={}&board={}&target={}&action=force-archive",if csrf {&f.csrf} else {"invalid"},f.boards[0],f.posts[0]);
            assert_eq!(f.app.clone().oneshot(req.body(Body::from(form)).unwrap()).await.unwrap().status(),status);
            assert_eq!(f.state().await,before);
        }
        sqlx::query("UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp()-interval '11 minutes' WHERE token_hash=$1").bind(auth::hash(&f.token)).execute(&f.owner).await.unwrap();
        assert_eq!(f.action(f.posts[0],"force-archive").await,StatusCode::FORBIDDEN);
        sqlx::query("UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp(),expires_at=clock_timestamp()-interval '1 second' WHERE token_hash=$1").bind(auth::hash(&f.token)).execute(&f.owner).await.unwrap();
        assert_eq!(f.action(f.posts[0],"force-archive").await,StatusCode::UNAUTHORIZED);
        sqlx::query("UPDATE staff_identity.sessions SET expires_at=clock_timestamp()+interval '1 hour' WHERE token_hash=$1").bind(auth::hash(&f.token)).execute(&f.owner).await.unwrap();
        sqlx::query("UPDATE staff_identity.accounts SET revoked_at=clock_timestamp() WHERE id=$1").bind(f.account).execute(&f.owner).await.unwrap();
        assert_eq!(f.action(f.posts[0],"force-archive").await,StatusCode::UNAUTHORIZED);
        assert_eq!(f.state().await,before);
        assert_eq!(f.audit_count().await,0);
    }}).await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn force_archive_audit_failure_rolls_back_transition_and_deletion_retirement() {
    let f = Fixture::new().await;
    f.ready().await;
    let function = format!("force_archive_fault_{}", f.boards[0]);
    assert!(
        f.boards[0].starts_with("sp") && f.boards[0][2..].bytes().all(|b| b.is_ascii_hexdigit())
    );
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE FUNCTION content.{function}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'owned archive audit failure'; END $$"))).execute(&f.owner).await.unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE TRIGGER {function} BEFORE INSERT ON content.moderation_audit FOR EACH ROW WHEN (NEW.board='{}' AND NEW.action='force-archive') EXECUTE FUNCTION content.{function}()",f.boards[0]))).execute(&f.owner).await.unwrap();
    let result=tokio::spawn({let f=f.clone();async move {
        sqlx::query("INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES($1,'owned archive rollback fixture')").bind(f.posts[0]).execute(&f.owner).await.unwrap();
        let before=f.state().await;
        assert_eq!(f.action(f.posts[0],"force-archive").await,StatusCode::SERVICE_UNAVAILABLE);
        assert_eq!(f.state().await,before);
        assert_eq!(f.audit_count().await,0);
        let secret:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM post_secrets.deletion WHERE post_id=$1)").bind(f.posts[0]).fetch_one(&f.owner).await.unwrap();
        assert!(secret,"archive trigger retirement must roll back with the failed audit");
    }}).await;
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
async fn force_archive_uses_existing_secret_and_report_group_retirement_triggers() {
    let f = Fixture::new().await;
    f.ready().await;
    // Import a fixture-owned immutable catalog without activating global mode.
    let categories = serde_json::json!({"version":1,"categories":[
        {"id":1,"board":f.boards[0],"op_only":false,"reply_only":false,"image_only":false,"exclude_boards":null,"title":"Owned archive ordinary","weight":1.0,"filtered":0},
        {"id":31,"board":f.boards[0],"op_only":false,"reply_only":false,"image_only":false,"exclude_boards":null,"title":"Owned archive illegal","weight":1.0,"filtered":0}
    ]});
    let revision: i64 = sqlx::query_scalar("SELECT content.import_report_catalog($1::text::jsonb)")
        .bind(categories.to_string())
        .fetch_one(&f.owner)
        .await
        .unwrap();
    let result=tokio::spawn({let f=f.clone();async move {
        let mut posts=vec![f.posts[0]];
        for _ in 0..5 {
            posts.push(sqlx::query_scalar("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,$2,'Anonymous','','Owned retirement reply') RETURNING id")
                .bind(&f.boards[0]).bind(f.posts[0]).fetch_one(&f.owner).await.unwrap());
        }
        let cases:Vec<Vec<Option<i16>>>=vec![vec![Some(1)],vec![Some(1),Some(2)],vec![Some(1),Some(2),Some(2)],vec![Some(1),Some(2),Some(2),Some(2)],vec![None,Some(1)],vec![Some(1)]];
        let mut seed=f.owner.begin().await.unwrap();
        // Match the documented board-first bulk membership maintenance boundary.
        sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE").bind(&f.boards[0]).execute(&mut *seed).await.unwrap();
        for (&post,kinds) in posts.iter().zip(&cases) {
            sqlx::query("INSERT INTO post_secrets.deletion(post_id,password_hash) VALUES($1,'owned retirement fixture')").bind(post).execute(&mut *seed).await.unwrap();
            let mut reports=vec![];
            for &kind in kinds {
                reports.push(sqlx::query_scalar::<_,i64>("INSERT INTO content.reports(board,post_id,reason,category_revision,category_id,category_kind,category_base_weight) VALUES($1,$2,'Owned archive report',$3,$4,$5,$6) RETURNING id")
                    .bind(&f.boards[0]).bind(post).bind(kind.map(|_|revision)).bind(kind.map(|k|if k==2 {31_i64}else{1_i64})).bind(kind).bind(kind.map(|_|1.0_f64)).fetch_one(&mut *seed).await.unwrap());
            }
            sqlx::query("SET LOCAL ROLE board_report_admission_owner").execute(&mut *seed).await.unwrap();
            sqlx::query("INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at) SELECT id,decode(repeat('ab',32),'hex'),$2,$3,$4,clock_timestamp() FROM unnest($1::bigint[]) id")
                .bind(&reports).bind(&f.boards[0]).bind(post).bind(f.posts[0]).execute(&mut *seed).await.unwrap();
            sqlx::query("RESET ROLE").execute(&mut *seed).await.unwrap();
        }
        // A genuinely missing historical counter must be kept conservatively.
        // Only this owned row is removed; no global trigger is ever disabled.
        sqlx::query("SET LOCAL ROLE board_report_admission_owner").execute(&mut *seed).await.unwrap();
        sqlx::query("DELETE FROM post_secrets.report_group WHERE board=$1 AND post_id=$2").bind(&f.boards[0]).bind(posts[5]).execute(&mut *seed).await.unwrap();
        sqlx::query("RESET ROLE").execute(&mut *seed).await.unwrap();
        seed.commit().await.unwrap();
        let retained_before:String=sqlx::query_scalar("SELECT jsonb_agg(to_jsonb(r) ORDER BY id)::text FROM content.reports r WHERE board=$1").bind(&f.boards[0]).fetch_one(&f.owner).await.unwrap();
        assert_eq!(f.action(f.posts[0],"force-archive").await,StatusCode::SEE_OTHER);
        let secrets:i64=sqlx::query_scalar("SELECT count(*) FROM post_secrets.deletion WHERE post_id=ANY($1)").bind(&posts).fetch_one(&f.owner).await.unwrap();
        assert_eq!(secrets,0,"archive retires both OP and reply deletion credentials");
        let mut inspect=f.owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_report_admission_owner").execute(&mut *inspect).await.unwrap();
        for (index,&post) in posts.iter().enumerate() {
            let members:i64=sqlx::query_scalar("SELECT count(*) FROM post_secrets.report_membership WHERE board=$1 AND post_id=$2").bind(&f.boards[0]).bind(post).fetch_one(&mut *inspect).await.unwrap();
            assert_eq!(members,if index<3 {0}else{cases[index].len() as i64},"illegal threshold/incomplete/missing group case {index}");
            let group:Option<(i64,bool)>=sqlx::query_as("SELECT illegal_count,incomplete FROM post_secrets.report_group WHERE board=$1 AND post_id=$2").bind(&f.boards[0]).bind(post).fetch_optional(&mut *inspect).await.unwrap();
            match index {
                0..=2|5=>assert_eq!(group,None),
                3=>assert_eq!(group,Some((3,false))),
                4=>assert_eq!(group,Some((0,true))),
                _=>unreachable!(),
            }
        }
        inspect.rollback().await.unwrap();
        let retained_after:String=sqlx::query_scalar("SELECT jsonb_agg(to_jsonb(r) ORDER BY id)::text FROM content.reports r WHERE board=$1").bind(&f.boards[0]).fetch_one(&f.owner).await.unwrap();
        assert_eq!(retained_after,retained_before,"retirement never deletes, resolves, or reclassifies report evidence");
        assert_eq!(f.audit_count().await,1);
    }}).await;
    // Remove only this fixture's reports and inactive immutable catalog.
    let mut cleanup = f.owner.begin().await.unwrap();
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(&f.boards[0])
        .execute(&mut *cleanup)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.reports WHERE board=$1")
        .bind(&f.boards[0])
        .execute(&mut *cleanup)
        .await
        .unwrap();
    sqlx::query("SET LOCAL ROLE board_report_admission_owner")
        .execute(&mut *cleanup)
        .await
        .unwrap();
    sqlx::query("DELETE FROM post_secrets.report_catalog_rows WHERE revision=$1")
        .bind(revision)
        .execute(&mut *cleanup)
        .await
        .unwrap();
    sqlx::query("DELETE FROM post_secrets.report_catalog_versions WHERE revision=$1")
        .bind(revision)
        .execute(&mut *cleanup)
        .await
        .unwrap();
    cleanup.commit().await.unwrap();
    f.cleanup().await;
    result.unwrap();
}

#[test]
fn isolated_source_oracle_pins_rejection_order_and_unchanged_saved_op_fields() {
    // The fixture executes only extracted forcearchive with synthetic boundary
    // recorders. It makes no database mutation or rendered-byte parity claim.
    let oracle: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/force-archive.json")).unwrap();
    let cases = oracle["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 13);
    let rejections = [
        ("rank_before_disabled_and_missing", "Can't let you do that."),
        (
            "disabled_before_missing",
            "Archives are disabled on this board.",
        ),
        ("missing_id", "Bad Request."),
        ("query_failure", "Database error."),
        ("missing_thread", "Thread not found."),
        ("reply_before_archived_and_sticky", "Thread not found."),
        ("archived_before_sticky", "This thread is already archived."),
        ("sticky", "fixture-sticky-denial"),
    ];
    for (id, error) in rejections {
        let case = cases.iter().find(|case| case["id"] == id).unwrap();
        assert_eq!(case["source"]["error"], error);
        assert!(case["source"]["audit"].is_null());
        assert!(
            case["source"]["calls"]
                .as_array()
                .unwrap()
                .iter()
                .all(|call| call["name"] == "mysql_board_call")
        );
    }
    for case in cases
        .iter()
        .filter(|case| case["source"]["error"].is_null())
    {
        let calls = case["source"]["calls"].as_array().unwrap();
        let mut expected = vec!["mysql_board_call", "archive_thread", "log_mod_action"];
        if case["json_enabled"] == true {
            expected.push("generate_board_archived_json");
        }
        expected.push("updating_index");
        assert_eq!(
            calls
                .iter()
                .map(|call| call["name"].as_str().unwrap())
                .collect::<Vec<_>>(),
            expected
        );
        let audit = &case["source"]["audit"];
        assert_eq!(audit["action"], 3);
        assert_eq!(audit["post"].as_object().unwrap().len(), 6);
        for field in ["no", "name", "sub", "com", "filename", "ext"] {
            assert_eq!(
                audit["post"][field], case["thread"][field],
                "source forwards saved {field} in {}",
                case["id"]
            );
        }
        assert_eq!(calls[2]["arguments"][0], 3);
        assert_eq!(calls[2]["arguments"][1], audit["post"]);
    }
    let undead = cases
        .iter()
        .find(|case| case["id"] == "undead_allowed")
        .unwrap();
    assert_eq!(undead["thread"]["undead"], 1);
    assert!(undead["source"]["error"].is_null());
}
