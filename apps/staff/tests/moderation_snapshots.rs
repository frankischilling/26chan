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

// Administrator-seeded historical logical fields, exercised through real HTTP
// authority and board_staff transactions. These are not posting-admission or
// media-publication claims, nor byte comparisons with legacy storedHTML.

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
        self.role("manager", &["all".into()], &[]).await;
        sqlx::query("UPDATE content.boards SET comment_spoiler_cleanup=true WHERE slug=ANY($1)")
            .bind(self.boards.to_vec())
            .execute(&self.owner)
            .await
            .unwrap();
    }

    async fn options(&self, fields: &str) -> StatusCode {
        self.request(
            "/thread-options",
            Some(format!(
                "csrf={}&board={}&target={}&{fields}",
                self.csrf, self.boards[0], self.posts[0]
            )),
        )
        .await
        .0
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

    async fn assert_prompt_denial(&self, action: &str, expected: StatusCode) {
        for lock_post in [false, true] {
            let mut lock = self.owner.begin().await.unwrap();
            if lock_post {
                sqlx::query("SELECT id FROM content.posts WHERE id=$1 FOR UPDATE")
                    .bind(self.posts[0])
                    .execute(&mut *lock)
                    .await
                    .unwrap();
            } else {
                sqlx::query("SELECT id FROM content.threads WHERE id=$1 FOR UPDATE")
                    .bind(self.posts[0])
                    .execute(&mut *lock)
                    .await
                    .unwrap();
            }
            let denied =
                tokio::time::timeout(Duration::from_secs(2), self.action(self.posts[0], action))
                    .await;
            lock.rollback().await.unwrap();
            assert_eq!(
                denied.expect("ineligible target denial must not acquire a target lock"),
                expected
            );
        }
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

#[tokio::test]
async fn grouped_exact_saved_preimages_survive_policy_edits_and_deletion() {
    let f = Fixture::new().await;
    let result = tokio::spawn({ let f = f.clone(); async move {
        f.ready().await;
        f.rich_post().await;
        let before = f.saved(f.posts[0]).await;
        assert_eq!(before["snapshot_name"], "");
        assert_eq!(before["snapshot_filename"], "<removed & name>.png");
        assert_eq!(before["snapshot_dice_result"], "3d6: 2 + 4 + 6 = 12");
        assert_eq!(f.options("sticky=1&closed=1&sticky_rank=3").await, StatusCode::SEE_OTHER);
        let recorded = f.audits().await;
        assert_eq!(recorded.len(), 1);
        assert_snapshot(&recorded[0], &before);
        assert_eq!(recorded[0]["before_mask"], 0);
        assert_eq!(recorded[0]["after_mask"], 5);
        assert_eq!(recorded[0]["account_id"], f.account);
        assert_eq!(recorded[0]["target_id"], f.posts[0]);
        assert_eq!(recorded[0]["action"], "thread-options");
        for fields in ["sticky=1&closed=1&sticky_rank=3", "sticky=1&closed=1&sticky_rank=60"] {
            assert_eq!(f.options(fields).await, StatusCode::SEE_OTHER);
            assert_eq!(f.audits().await, recorded, "no-op and rank-only forms cannot duplicate evidence");
        }
        sqlx::query("UPDATE content.boards SET word_filter_enabled=true,word_filter_profile=4,comment_code_spacing=false,comment_spoiler_cleanup=false,op_markup=false,dice_roll=false,fortune_trip=false WHERE slug=$1")
            .bind(&f.boards[0]).execute(&f.owner).await.unwrap();
        assert_eq!(f.options("closed=1").await, StatusCode::SEE_OTHER);
        let rows = f.audits().await;
        assert_eq!(rows.len(), 2);
        assert_snapshot(&rows[1], &before);
        sqlx::query("UPDATE content.posts SET name='Edited',subject='Edited',comment='Edited',comment_format=0,staff_authorized_limits=false,trip=NULL,capcode=NULL,dice_result=NULL,fortune_text='New fortune',fortune_color='#abcdef',deleted=true WHERE id=$1")
            .bind(f.posts[0]).execute(&f.owner).await.unwrap();
        sqlx::query("DELETE FROM content.post_media WHERE post_id=$1").bind(f.posts[0]).execute(&f.owner).await.unwrap();
        sqlx::query("DELETE FROM content.posts WHERE id=$1").bind(f.posts[0]).execute(&f.owner).await.unwrap();
        assert_eq!(f.audits().await, rows, "saved evidence has no live-target dependency");
    }}).await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn spoiler_preimages_cover_both_old_flags_all_roles_and_reply_without_file() {
    let f = Fixture::new().await;
    let result = tokio::spawn({ let f = f.clone(); async move {
        f.ready().await;
        f.rich_post().await;
        let reply: i64 = sqlx::query_scalar("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,$2,'Reply','','Saved reply') RETURNING id")
            .bind(&f.boards[0]).bind(f.posts[0]).fetch_one(&f.owner).await.unwrap();
        sqlx::query("UPDATE content.posts SET fortune_text='Excellent Luck',fortune_color='#a1b2c3' WHERE id=$1")
            .bind(reply).execute(&f.owner).await.unwrap();
        for role in ["janitor", "moderator", "manager", "admin"] {
            f.role(role, &f.boards[..1], &[]).await;
            for target in [f.posts[0], reply] {
                for action in ["spoiler", "unspoiler"] {
                    let before = f.saved(target).await;
                    let count = f.audits().await.len();
                    assert_eq!(f.action(target, action).await, StatusCode::SEE_OTHER);
                    let rows = f.audits().await;
                    assert_eq!(rows.len(), count + 1);
                    assert_snapshot(rows.last().unwrap(), &before);
                    assert_eq!(before["snapshot_image_spoiler"], action == "unspoiler");
                    assert_eq!(rows.last().unwrap()["action"], action);
                    assert_eq!(rows.last().unwrap()["target_id"], target);
                    assert!(rows.last().unwrap()["before_mask"].is_null());
                    assert!(rows.last().unwrap()["after_mask"].is_null());
                    let unchanged = f.state().await;
                    assert_eq!(f.action(target, action).await, StatusCode::SEE_OTHER);
                    assert_eq!(f.state().await, unchanged);
                    assert_eq!(f.audits().await, rows);
                }
            }
        }
        let reply_snapshot = f.saved(reply).await;
        assert!(reply_snapshot["snapshot_filename"].is_null());
        assert_eq!(reply_snapshot["snapshot_fortune_text"], "Excellent Luck");
        assert_eq!(reply_snapshot["snapshot_fortune_color"], "#a1b2c3");
        // Historical output remains unchanged when current randomizer policy is off.
        assert_eq!(f.saved(f.posts[0]).await["snapshot_dice_result"], "3d6: 2 + 4 + 6 = 12");
    }}).await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn filtered_url_context_discarded_from_saved_comment_never_enters_audit() {
    let f = Fixture::new().await;
    let result = tokio::spawn({ let f = f.clone(); async move {
        f.ready().await;
        let discarded = "discarded-secret-context";
        let input = format!("https://boards.4chan.org/{}/thread/123/{discarded} fam", f.boards[0]);
        let mut prepared = board_domain::wordfiltered_comment::prepare(&input, board_domain::comment_markup::MarkupPolicy::default(), board_domain::wordfilter::Profile::Global, None).unwrap();
        prepared.freeze_format(&f.boards[0]);
        let payload = prepared.encode().unwrap();
        assert!(payload.windows(discarded.len()).any(|window| window == discarded.as_bytes()));
        let lines = board_domain::filtered_formatting::lines(&prepared, &f.boards[0]);
        let saved = board_domain::filtered_formatting::source_projection(&lines);
        assert!(!saved.contains(discarded));
        let search = board_domain::formatting::plain_text(&lines);
        sqlx::query("UPDATE content.posts SET comment=$2,wordfilter_payload=$3,wordfilter_search=$4 WHERE id=$1")
            .bind(f.posts[0]).bind(&saved).bind(payload).bind(search).execute(&f.owner).await.unwrap();
        let before = f.saved(f.posts[0]).await;
        assert_eq!(before["snapshot_wordfiltered"], true);
        assert_eq!(f.options("closed=1").await, StatusCode::SEE_OTHER);
        assert_eq!(f.action(f.posts[0], "spoiler").await, StatusCode::SEE_OTHER);
        let rows = f.audits().await;
        assert_eq!(rows.len(), 2);
        for row in rows {
            assert_snapshot(&row, &before);
            assert_eq!(row["snapshot_comment"], saved);
            assert!(!row.to_string().contains(discarded));
        }
    }}).await;
    f.cleanup().await;
    result.unwrap();
}

async fn wait_for_blocker(owner: &PgPool, blocker: i32) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE usename='board_staff' AND $1=ANY(pg_blocking_pids(pid)))")
                .bind(blocker).fetch_one(owner).await.unwrap();
            if waiting { break; }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }).await.expect("actual owned row lock wait was not observed");
}

#[tokio::test]
async fn snapshots_capture_committed_preimages_after_board_thread_and_post_lock_waits() {
    let f = Fixture::new().await;
    let result = tokio::spawn({
        let f = f.clone();
        async move {
            f.ready().await;
            f.rich_post().await;
            for grouped in [true, false] {
                for lock_kind in 0..3 {
                    sqlx::query("UPDATE content.threads SET closed=false WHERE id=$1")
                        .bind(f.posts[0])
                        .execute(&f.owner)
                        .await
                        .unwrap();
                    sqlx::query("UPDATE content.posts SET image_spoiler=false WHERE id=$1")
                        .bind(f.posts[0])
                        .execute(&f.owner)
                        .await
                        .unwrap();
                    let mut expected = f.saved(f.posts[0]).await;
                    let count = f.audits().await.len();
                    let mut lock = f.owner.begin().await.unwrap();
                    let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
                        .fetch_one(&mut *lock)
                        .await
                        .unwrap();
                    match lock_kind {
                        0 => {
                            sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
                                .bind(&f.boards[0])
                                .execute(&mut *lock)
                                .await
                                .unwrap();
                        }
                        1 => {
                            sqlx::query("SELECT id FROM content.threads WHERE id=$1 FOR UPDATE")
                                .bind(f.posts[0])
                                .execute(&mut *lock)
                                .await
                                .unwrap();
                        }
                        _ => {
                            sqlx::query("SELECT id FROM content.posts WHERE id=$1 FOR UPDATE")
                                .bind(f.posts[0])
                                .execute(&mut *lock)
                                .await
                                .unwrap();
                        }
                    }
                    let pending = tokio::spawn({
                        let f = f.clone();
                        async move {
                            if grouped {
                                f.options("closed=1").await
                            } else {
                                f.action(f.posts[0], "spoiler").await
                            }
                        }
                    });
                    wait_for_blocker(&f.owner, pid).await;
                    let comment = format!("Committed during {lock_kind} wait, grouped={grouped}");
                    sqlx::query("UPDATE content.posts SET comment=$2 WHERE id=$1")
                        .bind(f.posts[0])
                        .bind(&comment)
                        .execute(&mut *lock)
                        .await
                        .unwrap();
                    expected["snapshot_comment"] = serde_json::Value::String(comment);
                    lock.commit().await.unwrap();
                    assert_eq!(pending.await.unwrap(), StatusCode::SEE_OTHER);
                    let rows = f.audits().await;
                    assert_eq!(rows.len(), count + 1);
                    assert_snapshot(rows.last().unwrap(), &expected);
                }
            }
            // Concurrent identical effective requests must not append two snapshots.
            sqlx::query("UPDATE content.posts SET image_spoiler=false WHERE id=$1")
                .bind(f.posts[0])
                .execute(&f.owner)
                .await
                .unwrap();
            let before = f.saved(f.posts[0]).await;
            let count = f.audits().await.len();
            let (first, second) = tokio::join!(
                f.action(f.posts[0], "spoiler"),
                f.action(f.posts[0], "spoiler")
            );
            assert_eq!(
                (first, second),
                (StatusCode::SEE_OTHER, StatusCode::SEE_OTHER)
            );
            let rows = f.audits().await;
            assert_eq!(rows.len(), count + 1);
            assert_snapshot(rows.last().unwrap(), &before);
            let before = f.saved(f.posts[0]).await;
            let count = rows.len();
            let (first, second) = tokio::join!(f.options("sticky=1"), f.options("sticky=1"));
            assert_eq!(
                (first, second),
                (StatusCode::SEE_OTHER, StatusCode::SEE_OTHER)
            );
            let rows = f.audits().await;
            assert_eq!(rows.len(), count + 1);
            assert_snapshot(rows.last().unwrap(), &before);
        }
    })
    .await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn snapshot_capture_preserves_disabled_missing_deleted_and_archive_outcomes() {
    let f = Fixture::new().await;
    let result = tokio::spawn({ let f = f.clone(); async move {
        f.ready().await;
        sqlx::query("UPDATE content.boards SET comment_spoiler_cleanup=false WHERE slug=$1")
            .bind(&f.boards[0]).execute(&f.owner).await.unwrap();
        let unchanged = f.state().await;
        for target in [f.posts[0], i64::MAX] {
            assert_eq!(f.action(target, "spoiler").await, StatusCode::BAD_REQUEST);
        }
        for lock_post in [false, true] {
            let mut lock = f.owner.begin().await.unwrap();
            if lock_post {
                sqlx::query("SELECT id FROM content.posts WHERE id=$1 FOR UPDATE").bind(f.posts[0]).execute(&mut *lock).await.unwrap();
            } else {
                sqlx::query("SELECT id FROM content.threads WHERE id=$1 FOR UPDATE").bind(f.posts[0]).execute(&mut *lock).await.unwrap();
            }
            let denied = tokio::time::timeout(Duration::from_secs(2), f.action(f.posts[0], "spoiler")).await;
            lock.rollback().await.unwrap();
            assert_eq!(denied.expect("disabled board denial must not acquire a target lock"), StatusCode::BAD_REQUEST);
        }
        assert_eq!(f.state().await, unchanged);
        assert_eq!(f.audit_count().await, 0);
        sqlx::query("UPDATE content.boards SET comment_spoiler_cleanup=true,archive_retention_seconds=86400 WHERE slug=$1")
            .bind(&f.boards[0]).execute(&f.owner).await.unwrap();
        assert_eq!(f.action(i64::MAX, "spoiler").await, StatusCode::NOT_FOUND);
        sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
            .bind(f.posts[0]).execute(&f.owner).await.unwrap();
        let before = f.saved(f.posts[0]).await;
        assert_eq!(f.options("closed=1").await, StatusCode::BAD_REQUEST);
        assert_eq!(f.audit_count().await, 0);
        assert_eq!(f.action(f.posts[0], "spoiler").await, StatusCode::SEE_OTHER);
        let rows = f.audits().await;
        assert_eq!(rows.len(), 1);
        assert_snapshot(&rows[0], &before);
        sqlx::query("UPDATE content.threads SET created_at=clock_timestamp()-interval '3 hours',archived_at=clock_timestamp()-interval '2 hours',archive_expires_at=clock_timestamp()-interval '1 second' WHERE id=$1")
            .bind(f.posts[0]).execute(&f.owner).await.unwrap();
        let unchanged = f.state().await;
        assert_eq!(f.action(f.posts[0], "unspoiler").await, StatusCode::NOT_FOUND);
        f.assert_prompt_denial("unspoiler", StatusCode::NOT_FOUND).await;
        assert_eq!(f.state().await, unchanged);
        assert_eq!(f.audits().await, rows);
        sqlx::query("UPDATE content.threads SET archived_at=NULL,archive_expires_at=NULL WHERE id=$1")
            .bind(f.posts[0]).execute(&f.owner).await.unwrap();
        sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1")
            .bind(f.posts[0]).execute(&f.owner).await.unwrap();
        let unchanged = f.state().await;
        assert_eq!(f.action(f.posts[0], "unspoiler").await, StatusCode::NOT_FOUND);
        assert_eq!(f.options("closed=1").await, StatusCode::NOT_FOUND);
        f.assert_prompt_denial("unspoiler", StatusCode::NOT_FOUND).await;
        assert_eq!(f.state().await, unchanged);
        assert_eq!(f.audits().await, rows);
    }}).await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn owned_snapshot_audit_failure_and_final_authority_failure_roll_back_together() {
    let f = Fixture::new().await;
    f.ready().await;
    // Only a validated, fixture-owned UUID slug enters DDL identifiers/literals.
    assert_eq!(f.boards[0].len(), 10);
    assert!(f.boards[0].starts_with("sp"));
    assert!(
        f.boards[0][2..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    );
    let function = format!("snapshot_fault_{}", f.boards[0]);
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE FUNCTION content.{function}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN RAISE EXCEPTION 'owned snapshot audit failure'; END $$")))
        .execute(&f.owner).await.unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE TRIGGER {function} BEFORE INSERT ON content.moderation_audit FOR EACH ROW WHEN (NEW.board='{}' AND NEW.action IN ('thread-options','spoiler','unspoiler')) EXECUTE FUNCTION content.{function}()",f.boards[0])))
        .execute(&f.owner).await.unwrap();
    let result = tokio::spawn({ let f = f.clone(); let function = function.clone(); async move {
        f.rich_post().await;
        for grouped in [true, false] {
            let before = f.state().await;
            let status = if grouped { f.options("sticky=1&closed=1&sticky_rank=60").await } else { f.action(f.posts[0], "spoiler").await };
            assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
            assert_eq!(f.state().await, before);
            assert_eq!(f.audit_count().await, 0);
        }
        // This AFTER INSERT delay guarantees content changes and the snapshot
        // exist in the uncommitted transaction before final authority expires.
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("DROP TRIGGER {function} ON content.moderation_audit")))
            .execute(&f.owner).await.unwrap();
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE OR REPLACE FUNCTION content.{function}() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN PERFORM pg_sleep(3); RETURN NEW; END $$")))
            .execute(&f.owner).await.unwrap();
        sqlx::raw_sql(sqlx::AssertSqlSafe(format!("CREATE TRIGGER {function} AFTER INSERT ON content.moderation_audit FOR EACH ROW WHEN (NEW.board='{}' AND NEW.action IN ('thread-options','spoiler','unspoiler')) EXECUTE FUNCTION content.{function}()",f.boards[0])))
            .execute(&f.owner).await.unwrap();
        for grouped in [true, false] {
            sqlx::query("UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp(),expires_at=clock_timestamp()+interval '2 seconds',last_activity_at=clock_timestamp() WHERE token_hash=$1")
                .bind(auth::hash(&f.token)).execute(&f.owner).await.unwrap();
            let before = f.state().await;
            let status = if grouped { f.options("sticky=1&closed=1&sticky_rank=60").await } else { f.action(f.posts[0], "spoiler").await };
            assert_eq!(status, StatusCode::UNAUTHORIZED);
            assert_eq!(f.state().await, before, "final authority failure must undo both mutation and captured evidence");
            assert_eq!(f.audit_count().await, 0);
        }
    }}).await;
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "DROP TRIGGER IF EXISTS {function} ON content.moderation_audit"
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
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn private_j_setter_denial_cannot_become_a_snapshot_write() {
    let f = Fixture::new().await;
    f.ready().await;
    // Own only this thread/post on the existing private board; never change
    // global /j/ policy or clean up another fixture's discussion rows.
    let mut seed = f.owner.begin().await.unwrap();
    let (spoilers, code, sjis, op, profile): (bool, bool, bool, bool, i16) = sqlx::query_as("SELECT comment_spoiler_cleanup,comment_code_spacing,comment_sjis_spacing,op_markup,word_filter_profile FROM content.boards WHERE slug='j' FOR SHARE")
        .fetch_one(&mut *seed).await.unwrap();
    let profile = match profile {
        0 => board_domain::wordfilter::Profile::Global,
        1 => board_domain::wordfilter::Profile::Basic,
        2 => board_domain::wordfilter::Profile::Asp,
        3 => board_domain::wordfilter::Profile::Video,
        4 => board_domain::wordfilter::Profile::Test,
        _ => panic!("unknown owned fixture wordfilter profile"),
    };
    let rolls = (profile == board_domain::wordfilter::Profile::Test)
        .then(|| board_domain::wordfilter::LeetRolls::from_choices(0, 1).unwrap());
    let mut prepared = board_domain::wordfiltered_comment::prepare(
        "Private saved text",
        board_domain::comment_markup::MarkupPolicy {
            spoilers,
            code,
            sjis,
            op,
        },
        profile,
        rolls,
    )
    .unwrap();
    prepared.freeze_format("j");
    let payload = prepared.encode().unwrap();
    let lines = board_domain::filtered_formatting::lines(&prepared, "j");
    let comment = board_domain::filtered_formatting::source_projection(&lines);
    let search = board_domain::formatting::plain_text(&lines);
    sqlx::query("SELECT set_config('board.wordfilter_payload',encode($1::bytea,'hex'),true),set_config('board.wordfilter_search',$2,true)")
        .bind(payload).bind(search).execute(&mut *seed).await.unwrap();
    let post: i64 =
        sqlx::query_scalar("INSERT INTO content.threads(board) VALUES('j') RETURNING id")
            .fetch_one(&mut *seed)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,'j',$1,'Anonymous','Private discussion',$2)")
        .bind(post).bind(comment).execute(&mut *seed).await.unwrap();
    seed.commit().await.unwrap();
    let result = tokio::spawn({ let f = f.clone(); async move {
        let before: String = sqlx::query_scalar("SELECT to_jsonb(p)::text FROM content.posts p WHERE id=$1")
            .bind(post).fetch_one(&f.owner).await.unwrap();
        for action in ["spoiler", "unspoiler"] {
            for lock_post in [false, true] {
                let mut lock = f.owner.begin().await.unwrap();
                if lock_post {
                    sqlx::query("SELECT id FROM content.posts WHERE id=$1 FOR UPDATE").bind(post).execute(&mut *lock).await.unwrap();
                } else {
                    sqlx::query("SELECT id FROM content.threads WHERE id=$1 FOR UPDATE").bind(post).execute(&mut *lock).await.unwrap();
                }
                let denied = tokio::time::timeout(Duration::from_secs(2), f.request("/moderate", Some(format!("csrf={}&board=j&target={post}&action={action}",f.csrf)))).await;
                lock.rollback().await.unwrap();
                assert_eq!(denied.expect("private board denial must not acquire a target lock").0, StatusCode::NOT_FOUND);
            }
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM content.moderation_audit WHERE board='j' AND target_id=$1")
                .bind(post).fetch_one(&f.owner).await.unwrap();
            assert_eq!(count, 0);
        }
        let error = sqlx::query("SELECT content.set_post_image_spoiler('j',$1,true)")
            .bind(post).execute(&f.staff).await.unwrap_err();
        assert_eq!(error.as_database_error().and_then(|error| error.code()).as_deref(), Some("P0002"));
        let after: String = sqlx::query_scalar("SELECT to_jsonb(p)::text FROM content.posts p WHERE id=$1")
            .bind(post).fetch_one(&f.owner).await.unwrap();
        assert_eq!(after, before);
    }}).await;
    sqlx::query("DELETE FROM content.moderation_audit WHERE board='j' AND target_id=$1")
        .bind(post)
        .execute(&f.owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.posts WHERE board='j' AND id=$1")
        .bind(post)
        .execute(&f.owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.threads WHERE board='j' AND id=$1")
        .bind(post)
        .execute(&f.owner)
        .await
        .unwrap();
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn grouped_snapshots_follow_effective_role_mask_not_attempted_protected_assignment() {
    let f = Fixture::new().await;
    let result = tokio::spawn({
        let f = f.clone();
        async move {
            f.ready().await;
            f.rich_post().await;
            for role in ["moderator", "manager", "admin"] {
                f.role(role, &f.boards[..1], &[]).await;
                sqlx::query("UPDATE content.threads SET closed=false,permaage=true WHERE id=$1")
                    .bind(f.posts[0])
                    .execute(&f.owner)
                    .await
                    .unwrap();
                let before = f.saved(f.posts[0]).await;
                let count = f.audits().await.len();
                assert_eq!(
                    f.options("closed=1&permaage=0").await,
                    StatusCode::SEE_OTHER
                );
                let rows = f.audits().await;
                assert_eq!(rows.len(), count + 1);
                let row = rows.last().unwrap();
                assert_eq!(row["before_mask"], 8);
                assert_eq!(row["after_mask"], if role == "moderator" { 12 } else { 4 });
                assert_snapshot(row, &before);
            }
            f.role("moderator", &f.boards[..1], &[]).await;
            sqlx::query("UPDATE content.threads SET closed=false,permaage=true WHERE id=$1")
                .bind(f.posts[0])
                .execute(&f.owner)
                .await
                .unwrap();
            let rows = f.audits().await;
            assert_eq!(f.options("permaage=0").await, StatusCode::SEE_OTHER);
            assert_eq!(
                f.audits().await,
                rows,
                "rejected protected assignment alone is no effective mask change"
            );
            f.role("janitor", &f.boards[..1], &[]).await;
            let before = f.state().await;
            assert_eq!(f.options("closed=1").await, StatusCode::FORBIDDEN);
            assert_eq!(f.state().await, before);
            assert_eq!(f.audits().await, rows);
        }
    })
    .await;
    f.cleanup().await;
    result.unwrap();
}
