#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use board_staff::{AppState, Config, Limits, auth, router};
use serde_json::Value;
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use tower::ServiceExt;
use webauthn_rs::prelude::*;

// The production gate deliberately refuses concurrent preparations. Serialize
// these independent HTTP proofs; contention belongs to the authority suite.
static TRANSFERS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone)]
struct Fixture {
    owner: PgPool,
    auth_pool: PgPool,
    staff: PgPool,
    app: Router,
    boards: [String; 2],
    op: i64,
    reply: i64,
    account: i64,
    credential: Vec<u8>,
    token: String,
    csrf: String,
}
async fn pool(key: &str) -> PgPool {
    PgPool::connect(&std::env::var(key).expect("Explicit test database required"))
        .await
        .unwrap()
}
impl Fixture {
    async fn new() -> Self {
        let owner = pool("MIGRATION_DATABASE_URL").await;
        let auth_pool = pool("AUTH_DATABASE_URL").await;
        let staff = pool("STAFF_DATABASE_URL").await;
        let boards =
            [0, 1].map(|_| format!("tt{}", &uuid::Uuid::new_v4().simple().to_string()[..8]));
        for board in &boards {
            sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Owned transfer fixture','',1000,100,100,100,10)")
                .bind(board).execute(&owner).await.unwrap();
        }
        let op = sqlx::query_scalar("INSERT INTO content.threads(board) VALUES($1) RETURNING id")
            .bind(&boards[0])
            .fetch_one(&owner)
            .await
            .unwrap();
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Historical name','Historical subject','TransferNeedle original')")
            .bind(op).bind(&boards[0]).execute(&owner).await.unwrap();
        let reply = sqlx::query_scalar("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,$2,'Reply name','','TransferNeedle reply') RETURNING id")
            .bind(&boards[0]).bind(op).fetch_one(&owner).await.unwrap();
        // Owner-authored synthetic historical rows satisfy normal constraints;
        // no trigger is disabled and no posting identity is impersonated.
        sqlx::query("UPDATE content.posts SET comment_format=104,country='XX',country_name='Unknown',created_at='2001-02-03T04:05:06Z' WHERE board=$1")
            .bind(&boards[0]).execute(&owner).await.unwrap();
        sqlx::query("UPDATE content.threads SET reply_count=1 WHERE id=$1")
            .bind(op)
            .execute(&owner)
            .await
            .unwrap();
        let account = sqlx::query_scalar("INSERT INTO staff_identity.accounts(role,allow_boards) VALUES('moderator',$1) RETURNING id")
            .bind(boards.to_vec()).fetch_one(&owner).await.unwrap();
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
            .bind(auth::hash(&token)).bind(auth::hash(&csrf)).bind(account).bind(&credential).execute(&owner).await.unwrap();
        let state = Arc::new(AppState {
            config: Config {
                asn_database: None,
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
                idle_timeout: Duration::from_secs(900),
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
        Self {
            owner,
            auth_pool,
            staff,
            app: router(state),
            boards,
            op,
            reply,
            account,
            credential,
            token,
            csrf,
        }
    }
    fn form(&self) -> String {
        format!(
            "board={}&target={}&destination={}&csrf={}&move_del=1",
            self.boards[0], self.op, self.boards[1], self.csrf
        )
    }
    fn request(&self) -> axum::http::request::Builder {
        Request::post("/thread-transfer")
            .header(
                "cookie",
                format!("staff={}; staff-csrf={}", self.token, self.csrf),
            )
            .header("content-type", "application/x-www-form-urlencoded")
            .header("origin", "http://localhost:3001")
            .header("sec-fetch-site", "same-origin")
    }
    async fn send(&self, form: String) -> (StatusCode, String) {
        response(
            self.app.clone(),
            self.request().body(Body::from(form)).unwrap(),
        )
        .await
    }
    async fn snapshot(&self) -> String {
        sqlx::query_scalar("SELECT jsonb_build_object('posts',(SELECT jsonb_agg(to_jsonb(p) ORDER BY id) FROM content.posts p WHERE board=ANY($1)),'threads',(SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM content.threads t WHERE board=ANY($1)),'reports',(SELECT jsonb_agg(to_jsonb(r) ORDER BY id) FROM content.reports r WHERE board=ANY($1)),'audit',(SELECT jsonb_agg(to_jsonb(a) ORDER BY id) FROM content.moderation_audit a WHERE board=ANY($1)))::text")
            .bind(self.boards.to_vec()).fetch_one(&self.owner).await.unwrap()
    }
    async fn cleanup(self) {
        // Pending children cascade only from their exact owned intent. Hold the
        // same global gate while removing this fixture's pending reservation.
        let mut tx = self.owner.begin().await.unwrap();
        // The migration login is NOINHERIT. Use its existing owner membership
        // only for these private, account-scoped fixture cleanup statements.
        sqlx::query("SET LOCAL ROLE board_thread_transfer_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SELECT singleton FROM post_secrets.thread_transfer_gate FOR UPDATE")
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("DELETE FROM post_secrets.thread_transfer_intents WHERE account_id=$1")
            .bind(self.account)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("DELETE FROM post_secrets.thread_transfer_result_posts WHERE request_id IN(SELECT request_id FROM post_secrets.thread_transfer_results WHERE account_id=$1)").bind(self.account).execute(&mut *tx).await.unwrap();
        sqlx::query("DELETE FROM post_secrets.thread_transfer_results WHERE account_id=$1")
            .bind(self.account)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("RESET ROLE").execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
        let assets: Vec<String> = sqlx::query_scalar("SELECT asset_id FROM content.post_media WHERE post_id IN(SELECT id FROM content.posts WHERE board=ANY($1))")
            .bind(self.boards.to_vec()).fetch_all(&self.owner).await.unwrap();
        for statement in [
            "DELETE FROM content.moderation_audit WHERE board=ANY($1)",
            "DELETE FROM content.reports WHERE board=ANY($1)",
            "DELETE FROM content.post_media WHERE post_id IN(SELECT id FROM content.posts WHERE board=ANY($1))",
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
        sqlx::query("DELETE FROM media.assets WHERE id=ANY($1)")
            .bind(assets)
            .execute(&self.owner)
            .await
            .unwrap();
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
        self.auth_pool.close().await;
        self.staff.close().await;
        self.owner.close().await;
    }
}
async fn response(app: Router, request: Request<Body>) -> (StatusCode, String) {
    let response = app.oneshot(request).await.unwrap();
    let status = response.status();
    let body = String::from_utf8(
        to_bytes(response.into_body(), 4_194_304)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    (status, body)
}

#[tokio::test]
async fn transfer_http_strict_form_and_current_authority_never_mutate() {
    let _serial = TRANSFERS.lock().await;
    let f = Fixture::new().await;
    let result = tokio::spawn({ let f = f.clone(); async move {
        let before = f.snapshot().await;
        for extra in ["&target=1", "&%74arget=2", "&destination=x", "&unknown=x", "&saved_presentation=FP01", "&comment=forged", "&ticket=chosen", "&capcode=admin", "&staff_authorized_limits=true"] {
            assert_eq!(f.send(format!("{}{extra}", f.form())).await.0, StatusCode::BAD_REQUEST, "{extra}");
            assert_eq!(f.snapshot().await, before);
        }
        for form in [
            f.form().replace("move_del=1", "move_del=0"),
            f.form().replace("&move_del=1", ""),
            f.form().replace(&format!("target={}", f.op), "target=-1"),
            f.form().replace(&format!("target={}", f.op), "target=9223372036854775808"),
            f.form().replace(&format!("destination={}", f.boards[1]), "destination=%FF"),
            f.form().replace(&format!("destination={}", f.boards[1]), "destination=%"),
            f.form().replace(&format!("destination={}", f.boards[1]), &format!("destination={}", f.boards[0])),
        ] { assert_eq!(f.send(form).await.0, StatusCode::BAD_REQUEST); assert_eq!(f.snapshot().await, before); }
        // The router rejects oversized bodies before form parsing.
        assert_eq!(
            f.send(format!("{}{}", f.form(), "x".repeat(4097))).await.0,
            StatusCode::PAYLOAD_TOO_LARGE,
        );
        assert_eq!(f.snapshot().await, before);
        let bad_csrf = f.form().replace(&f.csrf, &"a".repeat(43));
        assert_eq!(f.send(bad_csrf).await.0, StatusCode::FORBIDDEN);
        for (origin, site, content_type, cookie, expected) in [
            ("https://evil.invalid", "same-origin", "application/x-www-form-urlencoded", true, StatusCode::FORBIDDEN),
            ("http://localhost:3001", "cross-site", "application/x-www-form-urlencoded", true, StatusCode::FORBIDDEN),
            ("http://localhost:3001", "same-origin", "application/json", true, StatusCode::BAD_REQUEST),
            ("http://localhost:3001", "same-origin", "application/x-www-form-urlencoded", false, StatusCode::UNAUTHORIZED),
        ] {
            let mut req = Request::post("/thread-transfer").header("origin", origin).header("sec-fetch-site", site).header("content-type", content_type);
            if cookie { req = req.header("cookie", format!("staff={}; staff-csrf={}", f.token, f.csrf)); }
            assert_eq!(response(f.app.clone(), req.body(Body::from(f.form())).unwrap()).await.0, expected);
            assert_eq!(f.snapshot().await, before);
        }
        for header in ["origin", "sec-fetch-site", "content-type"] {
            let value = match header { "origin" => "http://localhost:3001", "sec-fetch-site" => "same-origin", _ => "application/x-www-form-urlencoded" };
            let status = response(f.app.clone(), f.request().header(header, value).body(Body::from(f.form())).unwrap()).await.0;
            assert!(status.is_client_error(), "duplicate {header}: {status}");
            assert_eq!(f.snapshot().await, before);
        }
        for (role, allow, deny) in [
            ("janitor", f.boards.to_vec(), vec![]),
            ("moderator", vec![f.boards[0].clone()], vec![]),
            ("moderator", vec![f.boards[1].clone()], vec![]),
            ("moderator", f.boards.to_vec(), vec![f.boards[1].clone()]),
        ] {
            sqlx::query("UPDATE staff_identity.accounts SET role=$2,allow_boards=$3,deny_boards=$4 WHERE id=$1").bind(f.account).bind(role).bind(allow).bind(deny).execute(&f.owner).await.unwrap();
            assert_eq!(f.send(f.form()).await.0, StatusCode::FORBIDDEN);
            assert_eq!(f.snapshot().await, before);
        }
        sqlx::query("UPDATE staff_identity.accounts SET role='moderator',allow_boards=$2,deny_boards='{}' WHERE id=$1").bind(f.account).bind(f.boards.to_vec()).execute(&f.owner).await.unwrap();
        for (statement, expected) in [
            ("UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp()-interval '11 minutes' WHERE account_id=$1", StatusCode::FORBIDDEN),
            ("UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp(),expires_at=clock_timestamp()-interval '1 second' WHERE account_id=$1", StatusCode::UNAUTHORIZED),
            ("UPDATE staff_identity.sessions SET expires_at=clock_timestamp()+interval '1 hour',last_activity_at=clock_timestamp()-interval '16 minutes' WHERE account_id=$1", StatusCode::UNAUTHORIZED),
        ] {
            sqlx::query(statement).bind(f.account).execute(&f.owner).await.unwrap();
            assert_eq!(f.send(f.form()).await.0, expected); assert_eq!(f.snapshot().await, before);
        }
        sqlx::query("UPDATE staff_identity.sessions SET last_activity_at=clock_timestamp() WHERE account_id=$1").bind(f.account).execute(&f.owner).await.unwrap();
        sqlx::query("UPDATE staff_identity.accounts SET revoked_at=clock_timestamp() WHERE id=$1").bind(f.account).execute(&f.owner).await.unwrap();
        assert_eq!(f.send(f.form()).await.0, StatusCode::UNAUTHORIZED);
        sqlx::query("UPDATE staff_identity.accounts SET revoked_at=NULL WHERE id=$1").bind(f.account).execute(&f.owner).await.unwrap();
        sqlx::query("DELETE FROM staff_identity.credentials WHERE id=$1").bind(&f.credential).execute(&f.owner).await.unwrap();
        assert_eq!(f.send(f.form()).await.0, StatusCode::UNAUTHORIZED);
        assert_eq!(f.snapshot().await, before);
    }}).await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn transfer_http_freezes_history_erases_source_and_reconciles_repeated_post() {
    let _serial = TRANSFERS.lock().await;
    let f = Fixture::new().await;
    let result = tokio::spawn({ let f = f.clone(); async move {
        sqlx::query("UPDATE content.boards SET country_flags=true WHERE slug=$1").bind(&f.boards[0]).execute(&f.owner).await.unwrap();
        // Destination admission/display switches must not restamp history.
        sqlx::query("UPDATE content.boards SET forced_anon=true,op_markup=true,comment_spoiler_cleanup=true,comment_code_spacing=true,comment_sjis_spacing=true WHERE slug=$1")
            .bind(&f.boards[1]).execute(&f.owner).await.unwrap();
        sqlx::query("UPDATE content.posts SET trip='!abcdefghij',capcode='admin',country='US',country_name='United States',file_deleted=true,image_spoiler=true,comment=$2 WHERE id=$1")
            .bind(f.op).bind(format!("TransferNeedle original >>{} >>{}", f.op, f.reply)).execute(&f.owner).await.unwrap();
        sqlx::query("UPDATE content.posts SET comment=$2,created_at='2001-02-03T04:06:07Z' WHERE id=$1")
            .bind(f.reply).bind(format!("TransferNeedle reply >>{} >>{}", f.op, f.reply)).execute(&f.owner).await.unwrap();
        sqlx::query("UPDATE content.threads SET sticky=true,sticky_rank=7,permasage=true,permaage=true,undead=true,created_at='2000-01-01Z',modified_at='2002-01-01Z',bumped_at='2003-01-01Z' WHERE id=$1")
            .bind(f.op).execute(&f.owner).await.unwrap();
        for id in [f.op, f.reply] {
            sqlx::query("INSERT INTO content.reports(board,post_id,reason) VALUES($1,$2,'Owned transfer report')").bind(&f.boards[0]).bind(id).execute(&f.owner).await.unwrap();
        }
        let source: Vec<Value> = sqlx::query_scalar::<_, String>("SELECT to_jsonb(p)::text FROM content.posts p WHERE board=$1 ORDER BY id")
            .bind(&f.boards[0]).fetch_all(&f.owner).await.unwrap().into_iter().map(|s| serde_json::from_str(&s).unwrap()).collect();
        let start: chrono::DateTime<chrono::Utc> = sqlx::query_scalar("SELECT clock_timestamp()").fetch_one(&f.owner).await.unwrap();
        let (status, html) = f.send(f.form()).await;
        assert_eq!(status, StatusCode::OK, "{html}");
        assert!(html.contains("Moved 2 posts"), "{html}");
        let destination: Vec<Value> = sqlx::query_scalar::<_, String>("SELECT to_jsonb(p)::text FROM content.posts p WHERE board=$1 ORDER BY id")
            .bind(&f.boards[1]).fetch_all(&f.owner).await.unwrap().into_iter().map(|s| serde_json::from_str(&s).unwrap()).collect();
        assert_eq!(destination.len(), 2);
        let new_op = destination[0]["id"].as_i64().unwrap();
        let new_reply = destination[1]["id"].as_i64().unwrap();
        assert!(new_op > f.reply && new_reply > new_op);
        for (old, new) in source.iter().zip(&destination) {
            for field in ["name", "trip", "subject", "capcode", "created_at", "file_deleted", "comment_format", "staff_authorized_limits", "country", "country_name"] {
                assert_eq!(new[field], old[field], "historical {field}");
            }
            assert_eq!(new["thread_id"], new_op);
            assert_eq!(new["board"], f.boards[1]);
            for field in ["poster_id", "json_op_poster_id", "board_flag", "flag_name", "wordfilter_payload", "wordfilter_search", "drawing_time_seconds", "drawing_source_post_id", "dice_result", "fortune_text", "fortune_color"] {
                assert!(new[field].is_null(), "unexpected {field}: {}", new[field]);
            }
            assert_eq!(new["board_flag_type"], "pol");
            for field in ["deleted", "content_erased", "image_spoiler"] { assert_eq!(new[field], false, "{field}"); }
            assert!(new["saved_presentation"].as_str().unwrap().starts_with("\\x46503031"));
            assert!(new["saved_presentation_search"].as_str().unwrap().contains("TransferNeedle"));
        }
        // OP self/forward references retain old IDs. Only the reply's prior OP
        // reference changes; its own ID is not yet in the replacement map.
        assert!(destination[0]["comment"].as_str().unwrap().contains(&format!("&gt;&gt;{}", f.op)));
        assert!(destination[0]["comment"].as_str().unwrap().contains(&format!("&gt;&gt;{}", f.reply)));
        assert!(destination[1]["comment"].as_str().unwrap().contains(&format!("&gt;&gt;{new_op}")));
        assert!(destination[1]["comment"].as_str().unwrap().contains(&format!("&gt;&gt;{}", f.reply)));
        let thread: Value = serde_json::from_str(&sqlx::query_scalar::<_, String>("SELECT to_jsonb(t)::text FROM content.threads t WHERE id=$1").bind(new_op).fetch_one(&f.owner).await.unwrap()).unwrap();
        assert_eq!(thread["created_at"], source[0]["created_at"]);
        assert_eq!(thread["modified_at"], source[0]["created_at"]);
        assert_eq!(thread["reply_count"], 1); assert_eq!(thread["sticky_rank"], 0);
        for flag in ["sticky", "closed", "permasage", "permaage", "undead", "deleted", "content_erased"] { assert_eq!(thread[flag], false, "{flag}"); }
        for field in ["archived_at", "archive_expires_at"] { assert!(thread[field].is_null()); }
        for field in ["bumped_at", "http_modified_at"] {
            let time = chrono::DateTime::parse_from_rfc3339(thread[field].as_str().unwrap()).unwrap().with_timezone(&chrono::Utc);
            assert!(time >= start, "fresh {field}");
        }
        let erased: i64 = sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1 AND deleted AND content_erased AND name='' AND subject='' AND comment='' AND trip IS NULL AND capcode IS NULL AND saved_presentation IS NULL AND saved_presentation_search IS NULL AND wordfilter_payload IS NULL AND wordfilter_search IS NULL AND created_at='epoch'::timestamptz")
            .bind(&f.boards[0]).fetch_one(&f.owner).await.unwrap();
        assert_eq!(erased, 2);
        let source_thread_erased: bool = sqlx::query_scalar("SELECT deleted AND content_erased AND reply_count=0 AND NOT closed AND NOT sticky AND modified_at='epoch'::timestamptz FROM content.threads WHERE id=$1")
            .bind(f.op).fetch_one(&f.owner).await.unwrap();
        assert!(source_thread_erased);
        let reports: i64 = sqlx::query_scalar("SELECT count(*) FROM content.reports WHERE board=$1").bind(&f.boards[0]).fetch_one(&f.owner).await.unwrap();
        assert_eq!(reports, 0);
        let committed = f.snapshot().await;
        assert_eq!(f.send(f.form()).await, (status, html.clone()), "repeated POST returns the committed result");
        assert_eq!(f.snapshot().await, committed, "recovery cannot allocate content or another audit");
        let get = Request::get(format!("/thread-transfer?board={}&target={}", f.boards[0], f.op))
            .header("cookie", format!("staff={}; staff-csrf={}", f.token, f.csrf)).body(Body::empty()).unwrap();
        assert_eq!(response(f.app.clone(), get).await, (status, html));
        let public_pool = pool("TEST_PUBLIC_DATABASE_URL").await;
        let (public, api) = board_public::routers(public_pool.clone(), "http://localhost:3000".into(), false);
        let path = format!("/{}/thread/{new_op}", f.boards[1]);
        let (status, body) = response(public.clone(), Request::get(&path).body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(body.contains("TransferNeedle")); assert!(body.contains("Historical name"));
        assert!(body.contains(&format!("&gt;&gt;{new_op}")), "frozen prior quote renders: {body}");
        let (status, body) = response(api, Request::get(format!("{path}.json")).body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        let json: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(json["posts"].as_array().unwrap().len(), 2);
        assert!(body.contains("TransferNeedle"));
        let (status, body) = response(public.clone(), Request::get(format!("/search/api?q=TransferNeedle&b={}", f.boards[1])).body(Body::empty()).unwrap()).await;
        assert_eq!(status, StatusCode::OK, "{body}"); assert!(body.contains("TransferNeedle"), "{body}");
        assert_eq!(response(public, Request::get(format!("/{}/thread/{}", f.boards[0], f.op)).body(Body::empty()).unwrap()).await.0, StatusCode::NOT_FOUND);
        public_pool.close().await;
        // Capture the typed FP representation after transfer. Staff must render
        // the saved report through that representation, not parse its projection.
        let captured_report: i64 = sqlx::query_scalar("INSERT INTO content.reports(board,post_id,reason) VALUES($1,$2,'Owned transferred presentation capture') RETURNING id")
            .bind(&f.boards[1]).bind(new_op).fetch_one(&f.owner).await.unwrap();
        let captured_fp: String = sqlx::query_scalar("SELECT payload->>'saved_presentation' FROM content.report_post_snapshots WHERE report_id=$1")
            .bind(captured_report).fetch_one(&f.staff).await.unwrap();
        assert!(captured_fp.starts_with("46503031"), "Report must retain the typed FP01 bytes");
        let captured_request = Request::get(format!("/reports?board={}", f.boards[1]))
            .header("cookie", format!("staff={}; staff-csrf={}", f.token, f.csrf)).body(Body::empty()).unwrap();
        let (captured_status, captured_html) = response(f.app.clone(), captured_request).await;
        assert_eq!(captured_status, StatusCode::OK, "{captured_html}");
        assert!(captured_html.contains(&format!("Post captured when report {captured_report} was admitted")));
        assert!(captured_html.contains("TransferNeedle"));
        assert!(!captured_html.contains("&amp;gt;&amp;gt;"), "Typed quote presentation must not be escaped a second time");
        // A later destructive move must qualify existing FP rather than parse
        // its projection as raw text, and erase both frozen storage columns.
        let again = format!("board={}&target={new_op}&destination={}&csrf={}&move_del=1", f.boards[1], f.boards[0], f.csrf);
        let (status, body) = f.send(again).await;
        assert_eq!(status, StatusCode::OK, "second frozen transfer: {body}");
        let remaining_capture: i64 = sqlx::query_scalar("SELECT count(*) FROM content.report_post_snapshots WHERE report_id=$1")
            .bind(captured_report).fetch_one(&f.staff).await.unwrap();
        assert_eq!(remaining_capture, 0, "Destructive transfer must erase the source report capture");
        let erased_frozen: i64 = sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1 AND content_erased AND deleted AND comment='' AND saved_presentation IS NULL AND saved_presentation_search IS NULL")
            .bind(&f.boards[1]).fetch_one(&f.owner).await.unwrap();
        assert_eq!(erased_frozen, 2);
        let unknown_countries: i64 = sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1 AND NOT deleted AND country='XX' AND country_name='Unknown' AND saved_presentation IS NOT NULL")
            .bind(&f.boards[0]).fetch_one(&f.owner).await.unwrap();
        assert_eq!(unknown_countries, 2, "source without country flags uses Unknown");
    }}).await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn transfer_http_refuses_ineligible_history_without_partial_changes() {
    let _serial = TRANSFERS.lock().await;
    let f = Fixture::new().await;
    let result = tokio::spawn({ let f = f.clone(); async move {
        for (set, reset) in [
            ("UPDATE content.threads SET closed=true WHERE id=$1", "UPDATE content.threads SET closed=false WHERE id=$1"),
            ("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1", "UPDATE content.threads SET archived_at=NULL,archive_expires_at=NULL WHERE id=$1"),
            ("UPDATE content.posts SET comment_format=0 WHERE id=$1", "UPDATE content.posts SET comment_format=104 WHERE id=$1"),
            ("UPDATE content.posts SET dice_result='1d6 = 4' WHERE id=$1", "UPDATE content.posts SET dice_result=NULL WHERE id=$1"),
            ("UPDATE content.posts SET fortune_text='Good luck',fortune_color='#00ff00' WHERE id=$1", "UPDATE content.posts SET fortune_text=NULL,fortune_color=NULL WHERE id=$1"),
            ("UPDATE content.posts SET drawing_time_seconds=10 WHERE id=$1", "UPDATE content.posts SET drawing_time_seconds=NULL WHERE id=$1"),
        ] {
            sqlx::query(set).bind(f.op).execute(&f.owner).await.unwrap();
            let before = f.snapshot().await;
            let (status, body) = f.send(f.form()).await;
            assert!(matches!(status, StatusCode::BAD_REQUEST | StatusCode::NOT_FOUND), "{set}: {status} {body}");
            assert_eq!(f.snapshot().await, before, "{set}");
            sqlx::query(reset).bind(f.op).execute(&f.owner).await.unwrap();
        }
        for flag in ["upload_board", "staff_only"] {
            // Static SQL only, even for synthetic owner fixtures.
            let (set, reset) = if flag == "upload_board" {
                ("UPDATE content.boards SET upload_board=true WHERE slug=$1", "UPDATE content.boards SET upload_board=false WHERE slug=$1")
            } else {
                ("UPDATE content.boards SET staff_only=true WHERE slug=$1", "UPDATE content.boards SET staff_only=false WHERE slug=$1")
            };
            for board in &f.boards {
                sqlx::query(set).bind(board).execute(&f.owner).await.unwrap();
                let before = f.snapshot().await;
                assert!(f.send(f.form()).await.0.is_client_error(), "{board} {flag}");
                assert_eq!(f.snapshot().await, before);
                sqlx::query(reset).bind(board).execute(&f.owner).await.unwrap();
            }
        }
        let asset = uuid::Uuid::new_v4().simple().to_string();
        sqlx::query("INSERT INTO media.assets(id,job_id,lease_token,sha256,bytes,width,height,state,approved_at,md5,thumbnail_sha256,thumbnail_bytes,thumbnail_width,thumbnail_height) VALUES($1,$1,$1,repeat('a',64),100,500,300,'approved',clock_timestamp(),repeat('b',32),repeat('c',64),50,250,150)")
            .bind(&asset).execute(&f.owner).await.unwrap();
        sqlx::query("INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler) VALUES($1,$2,$2,'owned.png',100,500,300,false)")
            .bind(f.reply).bind(&asset).execute(&f.owner).await.unwrap();
        for erased_file in [false, true] {
            sqlx::query("UPDATE content.post_media SET file_deleted=$2 WHERE post_id=$1").bind(f.reply).bind(erased_file).execute(&f.owner).await.unwrap();
            let before = f.snapshot().await;
            assert_eq!(f.send(f.form()).await.0, StatusCode::BAD_REQUEST, "structural media; file_deleted={erased_file}");
            assert_eq!(f.snapshot().await, before);
        }
        sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1").bind(f.reply).execute(&f.owner).await.unwrap();
        let before = f.snapshot().await;
        assert_eq!(f.send(f.form()).await.0, StatusCode::BAD_REQUEST, "deleted reply media still blocks transfer");
        assert_eq!(f.snapshot().await, before);
    }}).await;
    f.cleanup().await;
    result.unwrap();
}
