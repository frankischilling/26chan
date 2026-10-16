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
    revisions: [i64; 2],
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
            [0, 1].map(|_| format!("gq{}", &uuid::Uuid::new_v4().simple().to_string()[..8]));
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
            .bind(boards.to_vec()).bind(Vec::<String>::new()).fetch_one(&owner).await.unwrap();
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
        sqlx::query("UPDATE content.boards SET worksafe=false WHERE slug=$1")
            .bind(&boards[1])
            .execute(&owner)
            .await
            .unwrap();
        let mut revisions = [0; 2];
        for revision in &mut revisions {
            let categories: Vec<_> = (1..=20)
                .chain([31])
                .map(|id| {
                    serde_json::json!({
                        "id":id,"board":"","op_only":false,"reply_only":false,
                        "image_only":false,"exclude_boards":null,"title":"CatalogOnlyNeverRendered",
                        "weight":0.5,"filtered":0
                    })
                })
                .collect();
            *revision = sqlx::query_scalar("SELECT content.import_report_catalog($1::text::jsonb)")
                .bind(serde_json::json!({"version":1,"categories":categories}).to_string())
                .fetch_one(&owner)
                .await
                .unwrap();
        }
        Self {
            revisions,
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

    async fn cleanup(self) {
        let mut cleanup = self.owner.begin().await.unwrap();
        sqlx::query("SELECT slug FROM content.boards WHERE slug=ANY($1) ORDER BY slug FOR UPDATE")
            .bind(self.boards.to_vec())
            .execute(&mut *cleanup)
            .await
            .unwrap();
        sqlx::query(
            "SELECT id FROM content.threads WHERE board=ANY($1) ORDER BY board,id FOR UPDATE",
        )
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
        sqlx::query("SET LOCAL ROLE board_report_admission_owner")
            .execute(&mut *cleanup)
            .await
            .unwrap();
        for statement in [
            "DELETE FROM post_secrets.report_catalog_rows WHERE revision=ANY($1)",
            "DELETE FROM post_secrets.report_catalog_versions WHERE revision=ANY($1)",
        ] {
            sqlx::query(statement)
                .bind(self.revisions.to_vec())
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
    async fn post(&self, board: usize, comment: &str) -> i64 {
        let id: i64 =
            sqlx::query_scalar("INSERT INTO content.threads(board) VALUES($1) RETURNING id")
                .bind(&self.boards[board])
                .fetch_one(&self.owner)
                .await
                .unwrap();
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','',$3)")
            .bind(id).bind(&self.boards[board]).bind(comment).execute(&self.owner).await.unwrap();
        id
    }

    async fn report(
        &self,
        board: usize,
        post: i64,
        state: &str,
        reason: &str,
        category: Option<(usize, i64)>,
    ) -> i64 {
        let (revision, id, kind, weight) = match category {
            Some((revision, id)) => (
                Some(self.revisions[revision]),
                Some(id),
                Some(if id == 31 { 2_i16 } else { 1_i16 }),
                Some(0.5_f64),
            ),
            None => (None, None, None, None),
        };
        sqlx::query_scalar("INSERT INTO content.reports(board,post_id,state,reason,category_revision,category_id,category_kind,category_base_weight) VALUES($1,$2,$3,$4,$5,$6,$7,$8) RETURNING id")
            .bind(&self.boards[board]).bind(post).bind(state).bind(reason).bind(revision).bind(id).bind(kind).bind(weight)
            .fetch_one(&self.owner).await.unwrap()
    }

    async fn queue(&self, query: &str) -> String {
        let (status, html) = self.request(&format!("/reports{query}"), None).await;
        assert_eq!(status, StatusCode::OK, "{html}");
        html
    }

    async fn member(&self, board: usize, post: i64, report: i64, actor: &str, known: bool) {
        let mut tx = self.owner.begin().await.unwrap();
        sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
            .bind(&self.boards[board])
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("SET LOCAL ROLE board_report_admission_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        if known {
            sqlx::query("INSERT INTO post_secrets.report_weight_evidence(report_id,evaluator_version,known_or_verified,effective_weight,numeric_proof,evaluated_at) VALUES($1,1,false,0.5,'BaseEqualsFallback',clock_timestamp())")
                .bind(report).execute(&mut *tx).await.unwrap();
        }
        sqlx::query("INSERT INTO post_secrets.report_membership(report_id,actor_hash,board,post_id,thread_id,reported_at) VALUES($1,$2,$3,$4,$4,clock_timestamp())")
            .bind(report).bind(auth::hash(&format!("{}:{actor}", self.token))).bind(&self.boards[board]).bind(post)
            .execute(&mut *tx).await.unwrap();
        tx.commit().await.unwrap();
    }
}

fn article_ids(html: &str) -> Vec<i64> {
    html.split("<div id=\"report-")
        .skip(1)
        .map(|rest| rest.split('"').next().unwrap().parse().unwrap())
        .collect()
}

#[tokio::test]
async fn captured_presentation_survives_live_edits_and_clear_without_granting_actions_or_scope() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result = tokio::spawn({ let f=f.clone(); async move {
        let post=f.post(0,"CapturedOriginalBody <script>snapshot-attack</script>").await;
        let report=f.report(0,post,"open","PrivateCapturedReason",None).await;
        sqlx::query("UPDATE content.posts SET comment='LaterLiveBody',name='LaterLiveName',subject='LaterLiveSubject' WHERE id=$1")
            .bind(post).execute(&f.owner).await.unwrap();
        sqlx::query("UPDATE content.threads SET closed=true WHERE id=$1")
            .bind(post).execute(&f.owner).await.unwrap();
        f.role("janitor",&[f.boards[0].clone()],&[]).await;
        let html=f.queue(&format!("?board={}",f.boards[0])).await;
        let selected=article(&html,report);
        assert!(selected.contains("CapturedOriginalBody"));
        assert!(selected.contains("Post captured when report"));
        assert!(selected.contains("closed: true"),"Action state stays current");
        for hidden in ["LaterLiveBody","LaterLiveName","LaterLiveSubject","PrivateCapturedReason","<script>"] {
            assert!(!selected.contains(hidden),"{hidden}");
        }
        sqlx::query("UPDATE content.reports SET group_cleared_at=clock_timestamp(),group_cleared_by=$2,group_clear_inherited=false WHERE id=$1")
            .bind(report).bind(f.account).execute(&f.owner).await.unwrap();
        let path=format!("/reports/cleared?board={}",f.boards[0]);
        let (status,history)=f.request(&path,None).await;
        assert_eq!(status,StatusCode::OK,"{history}");
        assert!(history.contains("CapturedOriginalBody"));
        for hidden in ["LaterLiveBody","PrivateCapturedReason","Originating clear account","<script>","<form","/moderate"] {
            assert!(!history.contains(hidden),"{hidden}");
        }
        f.role("janitor",&[f.boards[1].clone()],&[]).await;
        let (status,denied)=f.request(&path,None).await;
        assert_eq!(status,StatusCode::FORBIDDEN);
        assert!(!denied.contains("CapturedOriginalBody"));
    }}).await;
    f.cleanup().await;
    result.unwrap();
}

fn article(html: &str, id: i64) -> &str {
    html.split(&format!("<div id=\"report-{id}\">"))
        .nth(1)
        .expect("selected group representative article")
        .split("</article>")
        .next()
        .unwrap()
}

#[tokio::test]
async fn groups_before_limit_and_keeps_resolved_dismissed_with_newest_open_representative() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result = tokio::spawn({ let f = f.clone(); async move {
        f.report(1, f.posts[1], "dismissed", "OlderClosedMember", None).await;
        let closed = f.report(1, f.posts[1], "resolved", "ResolvedGroup", None).await;
        let mut newest_open = 0;
        let mut previous_open = 0;
        for _ in 0..105 {
            previous_open = newest_open;
            newest_open = f.report(0, f.posts[0], "open", "RepeatedMember", None).await;
        }
        let other = f.post(0, "OtherGroupCurrentPreview").await;
        let other_open = f.report(0, other, "open", "OtherOpen", None).await;
        let dismissed = f.report(0, f.posts[0], "dismissed", "LatestClosedMember", None).await;
        let html = f.queue("").await;
        assert_eq!(article_ids(&html), vec![newest_open, other_open, closed]);
        assert!(!html.contains(&format!("id=\"report-{dismissed}\"")));
        assert!(article(&html, newest_open).contains("Scope fixture"));
        assert!(article(&html, closed).contains("resolved"));
        let (status, body) = f.request("/moderate", Some(format!("csrf={}&board={}&target={newest_open}&action=resolve", f.csrf, f.boards[0]))).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
        let states: Vec<(String,i64)> = sqlx::query_as("SELECT state,count(*) FROM content.reports WHERE board=$1 AND post_id=$2 GROUP BY state ORDER BY state")
            .bind(&f.boards[0]).bind(f.posts[0]).fetch_all(&f.owner).await.unwrap();
        assert_eq!(states, vec![("dismissed".into(),1),("open".into(),104),("resolved".into(),1)]);
        assert_eq!(article_ids(&f.queue("").await), vec![previous_open, other_open, closed]);
    }}).await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn cap_is_one_hundred_groups_with_a_one_hundred_and_first_sentinel() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result = tokio::spawn({
        let f = f.clone();
        async move {
            let mut ids = Vec::new();
            for n in 0..100 {
                let post = f.post(n % 2, &format!("QueuePreview{n:03}")).await;
                ids.push(f.report(n % 2, post, "open", "CapMember", None).await);
            }
            let html = f.queue("").await;
            assert_eq!(
                article_ids(&html),
                ids.iter().rev().copied().collect::<Vec<_>>()
            );
            assert!(!html.contains("More matching groups are not shown."));
            let post = f.post(0, "Newest101Preview").await;
            ids.push(f.report(0, post, "open", "CapMember", None).await);
            let html = f.queue("").await;
            assert_eq!(
                article_ids(&html),
                ids.iter().rev().take(100).copied().collect::<Vec<_>>()
            );
            assert!(
                html.contains("More matching groups are not shown."),
                "{html}"
            );
            assert!(!html.contains("QueuePreview000"));
            assert!(html.contains("Newest101Preview"));
        }
    })
    .await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn board_filters_validate_strictly_and_deny_precedes_existence() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result = tokio::spawn({
        let f = f.clone();
        async move {
            let ws = f
                .report(0, f.posts[0], "open", "WorksafeMember", None)
                .await;
            let nws = f
                .report(1, f.posts[1], "dismissed", "NonWorksafeMember", None)
                .await;
            for query in ["", "?board="] {
                assert_eq!(article_ids(&f.queue(query).await), vec![ws, nws]);
            }
            for (query, id) in [
                ("?board=_ws_".to_owned(), ws),
                ("?board=_nws_".to_owned(), nws),
                (format!("?board={}", f.boards[0]), ws),
            ] {
                assert_eq!(article_ids(&f.queue(&query).await), vec![id]);
            }
            for query in [
                "?board=a&board=b",
                "?board=&board=",
                "?other=x",
                "?board=a&unknown=x",
                "?board=%",
                "?board=%GG",
                "?board=%FF",
                "?board=a%00",
                "?board=A",
                "?board=All",
                "?board=a/b",
                "?board=+",
                "?board",
            ] {
                assert_eq!(
                    f.request(&format!("/reports{query}"), None).await.0,
                    StatusCode::BAD_REQUEST,
                    "{query}"
                );
            }
            assert_eq!(
                f.request(&format!("/reports?board={}", "a".repeat(257)), None)
                    .await
                    .0,
                StatusCode::BAD_REQUEST
            );
            let missing = format!("mq{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
            assert_eq!(
                f.request(&format!("/reports?board={missing}"), None)
                    .await
                    .0,
                StatusCode::FORBIDDEN
            );
            f.role(
                "moderator",
                &["all".into()],
                &[f.boards[0].clone(), missing.clone()],
            )
            .await;
            assert_eq!(
                f.request(&format!("/reports?board={missing}"), None)
                    .await
                    .0,
                StatusCode::FORBIDDEN
            );
            assert_eq!(
                f.request(&format!("/reports?board={}", f.boards[0]), None)
                    .await
                    .0,
                StatusCode::FORBIDDEN
            );
            f.role("moderator", &[f.boards[1].clone(), missing.clone()], &[])
                .await;
            assert_eq!(
                f.request(&format!("/reports?board={missing}"), None)
                    .await
                    .0,
                StatusCode::NOT_FOUND
            );
            assert_eq!(article_ids(&f.queue("").await), vec![nws]);
            assert!(article_ids(&f.queue("?board=_ws_").await).is_empty());
        }
    })
    .await;
    f.cleanup().await;
    result.unwrap();
}

fn contains_numeric_token(text: &str, expected: &str) -> bool {
    text.split(|value: char| !value.is_ascii_digit() && value != '.')
        .any(|value| value == expected)
}

#[test]
fn weight_guard_distinguishes_report_timestamps_from_private_numeric_values() {
    for visible in [
        "2026-10-14 13:02:50.579711 UTC",
        "2026-10-14 13:02:50.505940 UTC",
        "2026-10-14 13:02:00.500000 UTC",
    ] {
        assert!(!contains_numeric_token(visible, "0.5"));
    }
    for leaked in [
        "<p>Weight: 0.5</p>",
        "<span data-weight=\"0.5\">report</span>",
        r#"{"effective_weight":0.5}"#,
        "0.5",
    ] {
        assert!(contains_numeric_token(leaked, "0.5"));
    }
}

#[tokio::test]
async fn moderator_summaries_are_distinct_captured_bounded_escaped_and_absent_for_janitors() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result = tokio::spawn({
        let f = f.clone();
        async move {
            let title = "CapturedTitleSentinel<script>alert(1)</script>&";
            f.report(0, f.posts[0], "open", title, Some((0, 1))).await;
            f.report(0, f.posts[0], "resolved", title, Some((0, 1)))
                .await;
            f.report(0, f.posts[0], "dismissed", title, Some((1, 1)))
                .await;
            // Same revision/ID with a different captured title remains distinct.
            f.report(
                0,
                f.posts[0],
                "resolved",
                "DifferentCapturedTitle",
                Some((0, 1)),
            )
            .await;
            f.report(
                0,
                f.posts[0],
                "resolved",
                "CapturedIllegalSentinel",
                Some((0, 31)),
            )
            .await;
            let representative = f
                .report(0, f.posts[0], "open", "RepresentativeReasonSentinel", None)
                .await;
            let html = f.queue("").await;
            let group = article(&html, representative);
            assert!(group.contains("Uncleared retained reports: 6."));
            assert_eq!(group.matches("Captured category:").count(), 4);
            assert_eq!(
                group
                    .matches("CapturedTitleSentinel&#60;script&#62;alert(1)&#60;/script&#62;&#38;")
                    .count(),
                2
            );
            assert!(group.contains("DifferentCapturedTitle"));
            assert!(group.contains("CapturedIllegalSentinel"));
            assert!(group.contains("kind 2"));
            assert!(group.contains("RepresentativeReasonSentinel"));
            assert!(!html.contains("<script>alert(1)</script>"));
            assert!(!html.contains("CatalogOnlyNeverRendered"));
            for role in ["janitor", "moderator", "manager", "admin"] {
                f.role(role, &f.boards, &[]).await;
                let html = f.queue("").await;
                assert_eq!(article_ids(&html), vec![representative]);
                assert!(html.contains("Scope fixture"));
                assert!(html.contains(&format!("Selected report ID: {representative}.")));
                assert!(html.contains("/report-group-clear"));
                assert!(html.contains("/moderate"));
                if role == "janitor" {
                    for forbidden in [
                        "CapturedTitleSentinel",
                        "DifferentCapturedTitle",
                        "CapturedIllegalSentinel",
                        "RepresentativeReasonSentinel",
                        "report-count",
                        "report-categories",
                        "Captured category:",
                        "Uncleared retained reports:",
                        "category_revision",
                        "category_id",
                        "category_kind",
                        "effective_weight",
                    ] {
                        assert!(
                            !html.contains(forbidden),
                            "Janitor response leaked {forbidden}"
                        );
                    }
                    assert!(
                        !contains_numeric_token(&html, "0.5"),
                        "Janitor response leaked the private weight token"
                    );
                } else {
                    assert!(html.contains("Uncleared retained reports: 6."));
                    assert!(html.contains("CapturedTitleSentinel"));
                }
            }
            for id in 2..=20 {
                f.report(
                    0,
                    f.posts[0],
                    "resolved",
                    &format!("BoundedCategory{id:02}"),
                    Some((0, id)),
                )
                .await;
            }
            let html = f.queue("").await;
            let group = article(&html, representative);
            assert_eq!(group.matches("Captured category:").count(), 16);
            assert!(group.contains("More captured categories are retained"));
            assert!(group.contains("Uncleared retained reports: 25."));
            f.role("janitor", &f.boards, &[]).await;
            let html = f.queue("").await;
            assert!(!html.contains("BoundedCategory"));
            assert!(!html.contains("categories-truncated"));
            assert!(!html.contains("More captured categories"));
        }
    })
    .await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn clear_flags_and_deletion_cleanup_remove_targets_from_group_inventory() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result = tokio::spawn({ let f = f.clone(); async move {
        let live = f.report(0, f.posts[0], "dismissed", "RetainedDismissed", None).await;
        let reporter_cleared = f.report(0, f.posts[0], "open", "ReporterClearedHidden", None).await;
        let group_cleared = f.report(0, f.posts[0], "open", "GroupClearedHidden", None).await;
        sqlx::query("UPDATE content.reports SET reporter_cleared_at=clock_timestamp() WHERE id=$1")
            .bind(reporter_cleared).execute(&f.owner).await.unwrap();
        sqlx::query("UPDATE content.reports SET group_cleared_at=clock_timestamp(),group_cleared_by=$2,group_clear_inherited=false WHERE id=$1")
            .bind(group_cleared).bind(f.account).execute(&f.owner).await.unwrap();
        // Exercise deletion cleanup (0122) together with the queue. These
        // deletes retire reports; this does not independently prove SQL predicates.
        for deleted_thread in [false,true] {
            let post = f.post(1, "DeletedPreviewHidden").await;
            f.report(1, post, "open", "DeletedReportHidden", None).await;
            let sql = if deleted_thread { "UPDATE content.threads SET deleted=true WHERE id=$1" } else { "UPDATE content.posts SET deleted=true WHERE id=$1" };
            sqlx::query(sql).bind(post).execute(&f.owner).await.unwrap();
        }
        let html = f.queue("").await;
        assert_eq!(article_ids(&html), vec![live]);
        assert!(html.contains("Uncleared retained reports: 1."));
        for hidden in ["ReporterClearedHidden", "GroupClearedHidden", "DeletedPreviewHidden", "DeletedReportHidden"] {
            assert!(!html.contains(hidden), "{hidden}");
        }
    }}).await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn representative_actions_keep_reporter_and_group_clear_scopes_distinct() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result = tokio::spawn({ let f = f.clone(); async move {
        let first = f.report(0, f.posts[0], "open", "FirstActor", Some((0,1))).await;
        let selected = f.report(0, f.posts[0], "open", "SelectedActor", Some((0,1))).await;
        let same_actor_other_board = f.report(1, f.posts[1], "dismissed", "SelectedActorOtherBoard", Some((0,1))).await;
        f.member(0, f.posts[0], first, "first", true).await;
        f.member(0, f.posts[0], selected, "selected", true).await;
        f.member(1, f.posts[1], same_actor_other_board, "selected", true).await;
        let html = f.queue("").await;
        assert_eq!(article_ids(&html), vec![selected,same_actor_other_board]);
        assert!(!article(&html, selected).contains("/reporter-clear"));
        let (status, body) = f.request("/moderate", Some(format!("csrf={}&board={}&target={selected}&action=dismiss", f.csrf, f.boards[0]))).await;
        assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
        let first_state: String = sqlx::query_scalar("SELECT state FROM content.reports WHERE id=$1")
            .bind(first).fetch_one(&f.owner).await.unwrap();
        assert_eq!(first_state,"open");
        assert_eq!(article_ids(&f.queue("").await),vec![first,same_actor_other_board]);
        f.role("moderator", &["all".into()], &[]).await;
        let (status, body) = f.request("/reporter-clear", Some(format!("csrf={}&board={}&report_id={selected}", f.csrf, f.boards[0]))).await;
        assert_eq!(status, StatusCode::OK,"{body}");
        let marked: Vec<i64> = sqlx::query_scalar("SELECT id FROM content.reports WHERE board=ANY($1) AND reporter_cleared_at IS NOT NULL ORDER BY id")
            .bind(f.boards.to_vec()).fetch_all(&f.owner).await.unwrap();
        assert_eq!(marked,vec![selected,same_actor_other_board]);
        f.role("moderator", &f.boards, &[]).await;
        assert_eq!(article_ids(&f.queue("").await),vec![first]);
        let (status, body) = f.request("/report-group-clear", Some(format!("csrf={}&board={}&post_id={}", f.csrf, f.boards[0], f.posts[0]))).await;
        assert_eq!(status, StatusCode::OK,"{body}");
        assert!(article_ids(&f.queue("").await).is_empty());
        let evidence: i64 = {
            let mut tx=f.owner.begin().await.unwrap();
            sqlx::query("SET LOCAL ROLE board_report_admission_owner").execute(&mut *tx).await.unwrap();
            let count=sqlx::query_scalar("SELECT count(*) FROM post_secrets.report_weight_evidence WHERE report_id=ANY($1)")
                .bind(vec![first,selected,same_actor_other_board]).fetch_one(&mut *tx).await.unwrap();
            tx.rollback().await.unwrap();
            count
        };
        assert_eq!(evidence,3,"clear must retain private evidence");
    }}).await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn grouped_read_does_not_expand_runtime_private_acl() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result = tokio::spawn({
        let f = f.clone();
        async move {
            f.report(0, f.posts[0], "open", "ACLReadSentinel", Some((0, 1)))
                .await;
            assert!(f.queue("").await.contains("ACLReadSentinel"));
            for sql in [
                "SELECT actor_hash,automatic_identity FROM post_secrets.report_membership LIMIT 0",
                "SELECT * FROM post_secrets.report_weight_evidence LIMIT 0",
                "SELECT * FROM post_secrets.report_group LIMIT 0",
                "SELECT * FROM post_secrets.report_catalog_rows LIMIT 0",
                "UPDATE content.reports SET reporter_cleared_at=clock_timestamp() WHERE false",
                "UPDATE content.reports SET group_cleared_at=clock_timestamp() WHERE false",
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

// Pause only this fixture's pool acquisition, after the HTTP authority guard.
// No global relation lock, runtime timeout override, or scheduling guess is used.
async fn gated_pool() -> (
    PgPool,
    Arc<std::sync::atomic::AtomicBool>,
    Arc<tokio::sync::Notify>,
    Arc<tokio::sync::Notify>,
) {
    let armed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let reached = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .min_connections(1)
        .before_acquire({
            let armed = armed.clone();
            let reached = reached.clone();
            let release = release.clone();
            move |_, _| {
                let pause = armed.swap(false, std::sync::atomic::Ordering::SeqCst);
                let reached = reached.clone();
                let release = release.clone();
                Box::pin(async move {
                    if pause {
                        reached.notify_one();
                        release.notified().await;
                    }
                    Ok(true)
                })
            }
        })
        .connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
        .await
        .unwrap();
    // sqlx connect() opens and releases a connection before returning. The
    // before_acquire hook runs only for idle connections, so establish that
    // witness explicitly and prevent a second connection bypassing this gate.
    assert_eq!(pool.num_idle(), 1);
    (pool, armed, reached, release)
}

#[tokio::test]
async fn queue_holds_authority_during_read_and_next_request_observes_role_revocation() {
    let _serial = TEST.lock().await;
    let (pool, armed, reached, release) = gated_pool().await;
    let f = Fixture::with_staff_pool(Some(pool)).await;
    let result = tokio::spawn({
        let f = f.clone();
        let release = release.clone();
        async move {
            let id = f
                .report(
                    0,
                    f.posts[0],
                    "open",
                    "RoleGuardSensitiveSentinel",
                    Some((0, 1)),
                )
                .await;
            armed.store(true, std::sync::atomic::Ordering::SeqCst);
            let pending = tokio::spawn({
                let f = f.clone();
                async move { f.request("/reports", None).await }
            });
            tokio::time::timeout(Duration::from_secs(2), reached.notified())
                .await
                .expect("queue reached content pool under authority guard");
            let mut tx = f.owner.begin().await.unwrap();
            let error =
                sqlx::query("SELECT id FROM staff_identity.accounts WHERE id=$1 FOR UPDATE NOWAIT")
                    .bind(f.account)
                    .execute(&mut *tx)
                    .await
                    .unwrap_err();
            assert_eq!(
                error.as_database_error().unwrap().code().as_deref(),
                Some("55P03")
            );
            tx.rollback().await.unwrap();
            release.notify_one();
            let (status, html) = tokio::time::timeout(Duration::from_secs(2), pending)
                .await
                .unwrap()
                .unwrap();
            assert_eq!(status, StatusCode::OK, "{html}");
            assert_eq!(article_ids(&html), vec![id]);
            f.role("janitor", &f.boards, &[]).await;
            let html = f.queue("").await;
            assert!(!html.contains("RoleGuardSensitiveSentinel"));
            assert!(!html.contains("report-count"));
            sqlx::query(
                "UPDATE staff_identity.accounts SET revoked_at=clock_timestamp() WHERE id=$1",
            )
            .bind(f.account)
            .execute(&f.owner)
            .await
            .unwrap();
            let (status, html) = f.request("/reports", None).await;
            assert_eq!(status, StatusCode::UNAUTHORIZED);
            assert!(!html.contains("RoleGuardSensitiveSentinel"));
        }
    })
    .await;
    release.notify_one();
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn queue_rechecks_expiry_after_the_content_read_before_rendered_response_escapes() {
    let _serial = TEST.lock().await;
    let (pool, armed, reached, release) = gated_pool().await;
    let f = Fixture::with_staff_pool(Some(pool)).await;
    let result=tokio::spawn({let f=f.clone();let release=release.clone();async move {
        f.report(0,f.posts[0],"open","ExpiredResponseMustNotEscape",Some((0,1))).await;
        sqlx::query("UPDATE staff_identity.sessions SET expires_at=clock_timestamp()+interval '1 second' WHERE token_hash=$1")
            .bind(auth::hash(&f.token)).execute(&f.owner).await.unwrap();
        armed.store(true,std::sync::atomic::Ordering::SeqCst);
        let pending=tokio::spawn({let f=f.clone();async move {f.request("/reports",None).await}});
        tokio::time::timeout(Duration::from_secs(2),reached.notified()).await.expect("queue reached content pool before expiry");
        tokio::time::timeout(Duration::from_secs(2),async {
            loop {
                let expired:bool=sqlx::query_scalar("SELECT expires_at<=clock_timestamp() FROM staff_identity.sessions WHERE token_hash=$1")
                    .bind(auth::hash(&f.token)).fetch_one(&f.owner).await.unwrap();
                if expired {break;}
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        }).await.expect("database clock reaches this fixture's expiry");
        release.notify_one();
        let (status,html)=tokio::time::timeout(Duration::from_secs(2),pending).await.unwrap().unwrap();
        assert_eq!(status,StatusCode::UNAUTHORIZED,"{html}");
        assert!(!html.contains("ExpiredResponseMustNotEscape"));
        assert!(article_ids(&html).is_empty());
    }}).await;
    release.notify_one();
    f.cleanup().await;
    result.unwrap();
}

impl Fixture {
    async fn retained_and_private_snapshot(&self) -> String {
        let mut tx = self.owner.begin().await.unwrap();
        let reports: String = sqlx::query_scalar("SELECT coalesce(jsonb_agg(to_jsonb(r) ORDER BY id),'[]')::text FROM content.reports r WHERE board=ANY($1)")
            .bind(self.boards.to_vec()).fetch_one(&mut *tx).await.unwrap();
        let ids: Vec<i64> =
            sqlx::query_scalar("SELECT id FROM content.reports WHERE board=ANY($1) ORDER BY id")
                .bind(self.boards.to_vec())
                .fetch_all(&mut *tx)
                .await
                .unwrap();
        sqlx::query("SET LOCAL ROLE board_report_admission_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        let private: String = sqlx::query_scalar("SELECT jsonb_build_object('members',(SELECT coalesce(jsonb_agg(to_jsonb(m) ORDER BY report_id),'[]') FROM post_secrets.report_membership m WHERE board=ANY($1)),'evidence',(SELECT coalesce(jsonb_agg(to_jsonb(e) ORDER BY report_id),'[]') FROM post_secrets.report_weight_evidence e WHERE report_id=ANY($2)),'groups',(SELECT coalesce(jsonb_agg(to_jsonb(g) ORDER BY board,post_id),'[]') FROM post_secrets.report_group g WHERE board=ANY($1)))::text")
            .bind(self.boards.to_vec()).bind(ids).fetch_one(&mut *tx).await.unwrap();
        tx.rollback().await.unwrap();
        format!("{reports}\n{private}")
    }
}

#[tokio::test]
async fn failed_group_clear_preserves_retained_inventory_and_private_evidence() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result = tokio::spawn({
        let f = f.clone();
        async move {
            let retained_only = f
                .report(
                    0,
                    f.posts[0],
                    "resolved",
                    "RetainedWithoutMembership",
                    Some((0, 1)),
                )
                .await;
            let known = f
                .report(1, f.posts[1], "open", "KnownWeightMember", Some((0, 1)))
                .await;
            let unknown = f
                .report(
                    1,
                    f.posts[1],
                    "dismissed",
                    "UnknownWeightMember",
                    Some((0, 1)),
                )
                .await;
            f.member(1, f.posts[1], known, "known", true).await;
            f.member(1, f.posts[1], unknown, "unknown", false).await;
            let before = f.retained_and_private_snapshot().await;
            let inventory = f.queue("").await;
            assert_eq!(article_ids(&inventory), vec![known, retained_only]);
            assert!(article(&inventory, retained_only).contains("Uncleared retained reports: 1."));
            assert!(article(&inventory, known).contains("Uncleared retained reports: 2."));
            for (board, expected) in [(0, StatusCode::NOT_FOUND), (1, StatusCode::CONFLICT)] {
                let (status, body) = f
                    .request(
                        "/report-group-clear",
                        Some(format!(
                            "csrf={}&board={}&post_id={}",
                            f.csrf, f.boards[board], f.posts[board]
                        )),
                    )
                    .await;
                assert_eq!(status, expected, "{body}");
                if board == 1 {
                    assert!(body.contains(
                        "Report group cannot be cleared because report weights are unavailable."
                    ));
                }
                assert_eq!(f.retained_and_private_snapshot().await, before);
                assert_eq!(f.queue("").await, inventory);
            }
            let audits: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM content.moderation_audit WHERE account_id=$1",
            )
            .bind(f.account)
            .fetch_one(&f.owner)
            .await
            .unwrap();
            assert_eq!(audits, 0);
        }
    })
    .await;
    f.cleanup().await;
    result.unwrap();
}
