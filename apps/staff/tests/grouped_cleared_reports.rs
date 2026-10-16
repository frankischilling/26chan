#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use board_staff::{
    AppState, Config, Limits, access::Permissions, auth, cleared_report_queue, router,
};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use tower::ServiceExt;
use webauthn_rs::prelude::*;

// Every fixture owns its rows and catalog revisions; cleanup never resets shared data.
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
            [0, 1].map(|_| format!("ch{}", &uuid::Uuid::new_v4().simple().to_string()[..8]));
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
                        "image_only":false,"exclude_boards":null,"title":"CurrentCatalogMustNotRelabelHistory",
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

    fn session(&self, role: &str) -> auth::Session {
        auth::Session {
            account_id: self.account,
            role: role.into(),
            csrf_hash: auth::hash(&self.csrf),
            recent: true,
            permissions: Permissions {
                allow_boards: self.boards.to_vec(),
                ..Default::default()
            },
        }
    }

    async fn mark(&self, report: i64, origin: i64, actor: i64, inherited: bool) {
        sqlx::query("UPDATE content.reports SET group_cleared_at=to_timestamp($2::double precision),group_cleared_by=$3,group_clear_inherited=$4 WHERE id=$1")
            .bind(report).bind(origin as f64).bind(actor).bind(inherited)
            .execute(&self.owner).await.unwrap();
    }

    // Retained inventory fixtures intentionally have no private admission
    // membership. The separate group-clear suite exercises actual admissions.
    async fn cleared(
        &self,
        board: usize,
        post: i64,
        state: &str,
        reason: &str,
        category: Option<(usize, i64)>,
    ) -> i64 {
        let id = self.report(board, post, state, reason, category).await;
        self.mark(id, 1_700_000_000, self.account, false).await;
        id
    }

    async fn history(&self, board: usize) -> String {
        let (status, html) = self
            .request(
                &format!("/reports/cleared?board={}", self.boards[board]),
                None,
            )
            .await;
        assert_eq!(status, StatusCode::OK, "{html}");
        html
    }

    async fn page(&self, role: &str, board: usize) -> cleared_report_queue::ClearedReportQueuePage {
        cleared_report_queue::load(&self.staff, &self.session(role), &self.boards[board])
            .await
            .unwrap()
    }

    async fn snapshot(&self) -> String {
        let mut tx = self.owner.begin().await.unwrap();
        let reports: String = sqlx::query_scalar("SELECT coalesce(jsonb_agg(to_jsonb(r) ORDER BY id),'[]')::text FROM content.reports r WHERE board=ANY($1)")
            .bind(self.boards.to_vec()).fetch_one(&mut *tx).await.unwrap();
        let audit: String = sqlx::query_scalar("SELECT coalesce(jsonb_agg(to_jsonb(a) ORDER BY id),'[]')::text FROM content.moderation_audit a WHERE board=ANY($1)")
            .bind(self.boards.to_vec()).fetch_one(&mut *tx).await.unwrap();
        sqlx::query("SET LOCAL ROLE board_report_admission_owner")
            .execute(&mut *tx)
            .await
            .unwrap();
        let private: String = sqlx::query_scalar("SELECT jsonb_build_object('members',(SELECT coalesce(jsonb_agg(to_jsonb(m) ORDER BY report_id),'[]') FROM post_secrets.report_membership m WHERE board=ANY($1)),'groups',(SELECT coalesce(jsonb_agg(to_jsonb(g) ORDER BY board,post_id),'[]') FROM post_secrets.report_group g WHERE board=ANY($1)))::text")
            .bind(self.boards.to_vec()).fetch_one(&mut *tx).await.unwrap();
        tx.rollback().await.unwrap();
        format!("{reports}\n{audit}\n{private}")
    }
}

fn report_ids(html: &str) -> Vec<i64> {
    html.split("<article id=\"report-")
        .skip(1)
        .map(|rest| rest.split('"').next().unwrap().parse().unwrap())
        .collect()
}

fn group_ids(html: &str) -> Vec<i64> {
    html.split("id=\"cleared-report-group-")
        .skip(1)
        .map(|rest| rest.split('"').next().unwrap().parse().unwrap())
        .collect()
}

fn report_article(html: &str, id: i64) -> &str {
    html.split(&format!("<article id=\"report-{id}\">"))
        .nth(1)
        .expect("retained report article")
        .split("</article>")
        .next()
        .unwrap()
}

#[tokio::test]
async fn groups_before_limit_with_one_hundred_groups_and_a_distinct_overflow_sentinel() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result = tokio::spawn({
        let f = f.clone();
        async move {
            let mut posts = Vec::new();
            let mut singleton_ids = Vec::new();
            for n in 0..99 {
                let post = f.post(0, &format!("CapturedPostBody{n:03}")).await;
                posts.push(post);
                singleton_ids.push(
                    f.cleared(
                        0,
                        post,
                        "resolved",
                        &format!("SingletonHistory{n:03}"),
                        None,
                    )
                    .await,
                );
            }
            let mut crowded_ids = Vec::new();
            for _ in 0..105 {
                crowded_ids.push(
                    f.cleared(0, f.posts[0], "open", "CrowdedGroupMember", None)
                        .await,
                );
            }
            let newest = *crowded_ids.last().unwrap();
            let before = f.snapshot().await;
            let page = f.page("moderator", 0).await;
            assert_eq!(page.groups.len(), 100);
            assert!(
                !page.has_more,
                "105 reports from one target occupy one group slot"
            );
            assert_eq!(page.groups[0].representative.id, newest);
            let summary = page.groups[0].moderator_summary.as_ref().unwrap();
            assert_eq!(summary.report_count, 105);
            assert_eq!(
                summary
                    .additional_members
                    .iter()
                    .map(|r| r.id)
                    .collect::<Vec<_>>(),
                crowded_ids
                    .iter()
                    .rev()
                    .skip(1)
                    .take(15)
                    .copied()
                    .collect::<Vec<_>>()
            );
            assert!(summary.members_truncated);
            let html = f.history(0).await;
            let expected_groups: Vec<_> = [f.posts[0]]
                .into_iter()
                .chain(posts.iter().rev().copied())
                .collect();
            assert_eq!(group_ids(&html), expected_groups);
            assert_eq!(report_ids(&html).len(), 115);
            assert!(html.contains("cleared-members-truncated"));
            assert!(!html.contains("cleared-groups-truncated"));
            assert!(html.contains("CapturedPostBody000"));
            assert!(html.contains("Post captured when report"));
            assert_eq!(f.snapshot().await, before);

            let last_post = f.post(0, "NewestCapturedPostBody").await;
            let last_id = f
                .cleared(0, last_post, "dismissed", "NewestGroup", None)
                .await;
            let page = f.page("moderator", 0).await;
            assert_eq!(page.groups.len(), 100);
            assert!(page.has_more);
            assert_eq!(page.groups[0].representative.id, last_id);
            assert_eq!(page.groups[1].representative.id, newest);
            assert!(!page.groups.iter().any(|g| g.post_id == posts[0]));
            let html = f.history(0).await;
            let expected_groups: Vec<_> = [last_post, f.posts[0]]
                .into_iter()
                .chain(posts.iter().rev().take(98).copied())
                .collect();
            assert_eq!(group_ids(&html), expected_groups);
            assert!(html.contains("cleared-groups-truncated"));
            assert!(!html.contains("SingletonHistory000"));
            assert!(!html.contains("CapturedPostBody000"));
            assert!(html.contains("NewestCapturedPostBody"));
            assert!(!report_ids(&html).contains(&singleton_ids[0]));
            assert!(!report_ids(&html).contains(&crowded_ids[88]));
            assert!(report_ids(&html).contains(&crowded_ids[89]));

            f.role("janitor", &f.boards, &[]).await;
            let html = f.history(0).await;
            assert_eq!(group_ids(&html), expected_groups);
            assert_eq!(report_ids(&html).len(), 100);
            assert_eq!(report_ids(&html)[..2], [last_id, newest]);
            assert!(html.contains("cleared-groups-truncated"));
            for hidden in [
                "cleared-members-truncated",
                "cleared-categories-truncated",
                "CrowdedGroupMember",
                "SingletonHistory",
            ] {
                assert!(!html.contains(hidden), "Janitor leaked {hidden}");
            }
        }
    })
    .await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn mixed_dispositions_keep_each_origin_and_exclude_reporter_cleared_or_uncleared_rows() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result = tokio::spawn({
        let f = f.clone();
        async move {
            let first = f
                .cleared(0, f.posts[0], "open", "FirstOriginReason", Some((0, 1)))
                .await;
            let middle = f
                .cleared(
                    0,
                    f.posts[0],
                    "resolved",
                    "SecondOriginReason",
                    Some((1, 1)),
                )
                .await;
            let selected = f
                .cleared(
                    0,
                    f.posts[0],
                    "dismissed",
                    "InheritedReason<script>alert(1)</script>&",
                    Some((0, 31)),
                )
                .await;
            let actors = [771_234_567_890_i64, 771_234_567_891];
            f.mark(first, 1_600_000_001, actors[0], false).await;
            f.mark(middle, 1_700_000_002, actors[1], false).await;
            f.mark(selected, 1_600_000_001, actors[0], true).await;
            let reporter_cleared = f
                .cleared(
                    0,
                    f.posts[0],
                    "open",
                    "ReporterClearedReasonHidden",
                    Some((0, 2)),
                )
                .await;
            sqlx::query(
                "UPDATE content.reports SET reporter_cleared_at=clock_timestamp() WHERE id=$1",
            )
            .bind(reporter_cleared)
            .execute(&f.owner)
            .await
            .unwrap();
            let uncleared = f
                .report(0, f.posts[0], "open", "UnclearedReasonHidden", Some((0, 3)))
                .await;
            let excluded_post = f.post(0, "ExcludedTargetPreview").await;
            let excluded = f
                .cleared(
                    0,
                    excluded_post,
                    "resolved",
                    "ReporterClearedOnlyGroupHidden",
                    Some((0, 4)),
                )
                .await;
            sqlx::query(
                "UPDATE content.reports SET reporter_cleared_at=clock_timestamp() WHERE id=$1",
            )
            .bind(excluded)
            .execute(&f.owner)
            .await
            .unwrap();
            let other_board = f
                .cleared(
                    1,
                    f.posts[1],
                    "resolved",
                    "OtherBoardHistoryHidden",
                    Some((0, 5)),
                )
                .await;
            let before = f.snapshot().await;
            for role in ["janitor", "moderator", "manager", "admin"] {
                f.role(role, &f.boards, &[]).await;
                let page = f.page(role, 0).await;
                assert_eq!(page.groups.len(), 1);
                assert!(!page.has_more);
                let group = &page.groups[0];
                assert_eq!(group.post_id, f.posts[0]);
                assert_eq!(
                    group.representative.id, selected,
                    "newest eligible ID, regardless of state or clear time"
                );
                assert_eq!(group.representative.state, "dismissed");
                assert_eq!(
                    group.representative.group_cleared_at.timestamp(),
                    1_600_000_001
                );
                assert!(group.representative.group_clear_inherited);
                let html = f.history(0).await;
                assert_eq!(group_ids(&html), vec![f.posts[0]]);
                for hidden in [
                    "ReporterClearedReasonHidden",
                    "UnclearedReasonHidden",
                    "ReporterClearedOnlyGroupHidden",
                    "OtherBoardHistoryHidden",
                    "<script>alert(1)</script>",
                    "<form",
                    "<button",
                    "action=",
                    "csrf",
                    "/moderate",
                    "/reporter-clear",
                    "/report-group-clear",
                ] {
                    assert!(!html.contains(hidden), "{role} leaked {hidden}");
                }
                for hidden in [reporter_cleared, uncleared, excluded, other_board] {
                    assert!(!report_ids(&html).contains(&hidden));
                }
                let selected_article = report_article(&html, selected);
                assert!(selected_article.contains("Original report disposition: dismissed"));
                assert!(selected_article.contains("originating clear time:"));
                assert!(selected_article.contains("inherited clear: true"));
                if role == "janitor" {
                    assert!(group.moderator_summary.is_none());
                    assert!(group.representative.reason.is_none());
                    assert!(group.representative.category_id.is_none());
                    assert!(group.representative.group_cleared_by.is_none());
                    assert_eq!(report_ids(&html), vec![selected]);
                    for hidden in [
                        "FirstOriginReason",
                        "SecondOriginReason",
                        "InheritedReason",
                        "771234567890",
                        "771234567891",
                        "Originating clear account",
                        "Reason:",
                        "Category:",
                        "Captured category:",
                        "report-count",
                        "report-categories",
                        "cleared-members",
                        "cleared-categories",
                        "group_cleared_by",
                        "category_revision",
                        "category_id",
                        "category_kind",
                        "effective_weight",
                        "inherited clear: false",
                        "Original report disposition: open",
                        "Original report disposition: resolved",
                    ] {
                        assert!(
                            !html.contains(hidden),
                            "Janitor full response leaked {hidden}"
                        );
                    }
                } else {
                    assert_eq!(report_ids(&html), vec![selected, middle, first]);
                    let summary = group.moderator_summary.as_ref().unwrap();
                    assert_eq!(summary.report_count, 3);
                    assert_eq!(summary.categories.len(), 3);
                    assert!(!summary.members_truncated);
                    assert!(!summary.categories_truncated);
                    assert_eq!(
                        summary
                            .additional_members
                            .iter()
                            .map(|r| r.id)
                            .collect::<Vec<_>>(),
                        vec![middle, first]
                    );
                    assert_eq!(group.representative.group_cleared_by, Some(actors[0]));
                    for (record, id, state, time, actor) in [
                        (
                            &summary.additional_members[0],
                            middle,
                            "resolved",
                            1_700_000_002,
                            actors[1],
                        ),
                        (
                            &summary.additional_members[1],
                            first,
                            "open",
                            1_600_000_001,
                            actors[0],
                        ),
                    ] {
                        assert_eq!(record.id, id);
                        assert_eq!(record.state, state);
                        assert_eq!(record.group_cleared_at.timestamp(), time);
                        assert_eq!(record.group_cleared_by, Some(actor));
                        assert!(!record.group_clear_inherited);
                        let article = report_article(&html, id);
                        assert!(article.contains(&format!("Original report disposition: {state}")));
                        assert!(article.contains(&format!("Originating clear account: {actor}")));
                        assert!(article.contains("inherited clear: false"));
                    }
                    assert!(selected_article.contains("Originating clear account: 771234567890"));
                    assert!(
                        selected_article.contains(
                            "InheritedReason&#60;script&#62;alert(1)&#60;/script&#62;&#38;"
                        )
                    );
                }
            }
            assert_eq!(
                f.snapshot().await,
                before,
                "history reads do not change evidence, provenance, or audits"
            );
        }
    })
    .await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn captured_category_identity_and_member_limits_are_distinct_and_bounded() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result = tokio::spawn({
        let f = f.clone();
        async move {
            let title = "CapturedHistoricalTitle<script>x</script>&";
            for (state, revision, category, reason) in [
                ("open", 0, 1, title),
                ("resolved", 0, 1, title),
                ("dismissed", 1, 1, title),
                ("resolved", 0, 1, "DifferentCapturedTitle"),
                ("resolved", 0, 31, "CapturedIllegalTitle"),
            ] {
                f.cleared(0, f.posts[0], state, reason, Some((revision, category)))
                    .await;
            }
            f.cleared(0, f.posts[0], "open", "UncategorisedHistoricalReason", None)
                .await;
            let page = f.page("moderator", 0).await;
            let summary = page.groups[0].moderator_summary.as_ref().unwrap();
            assert_eq!(summary.report_count, 6);
            assert_eq!(summary.categories.len(), 4);
            assert_eq!(
                summary
                    .categories
                    .iter()
                    .filter(|c| c.title == title)
                    .count(),
                2
            );
            assert!(
                summary
                    .categories
                    .iter()
                    .any(|c| c.revision == f.revisions[0]
                        && c.id == 1
                        && c.kind == 1
                        && c.title == "DifferentCapturedTitle")
            );
            assert!(
                summary
                    .categories
                    .iter()
                    .any(|c| c.revision == f.revisions[0]
                        && c.id == 31
                        && c.kind == 2
                        && c.title == "CapturedIllegalTitle")
            );
            assert!(
                !summary
                    .categories
                    .iter()
                    .any(|c| c.title == "UncategorisedHistoricalReason")
            );
            let html = f.history(0).await;
            assert!(!html.contains("CurrentCatalogMustNotRelabelHistory"));
            assert!(!html.contains("<script>x</script>"));
            assert!(
                html.contains("CapturedHistoricalTitle&#60;script&#62;x&#60;/script&#62;&#38;")
            );

            let bounded_post = f.post(0, "BoundedPostContentHidden").await;
            let mut ids = Vec::new();
            for id in 1..=16 {
                ids.push(
                    f.cleared(
                        0,
                        bounded_post,
                        "resolved",
                        &format!("BoundedCapturedTitle{id:02}"),
                        Some((0, id)),
                    )
                    .await,
                );
            }
            let page = f.page("moderator", 0).await;
            let summary = page.groups[0].moderator_summary.as_ref().unwrap();
            assert_eq!(summary.report_count, 16);
            assert_eq!(summary.additional_members.len(), 15);
            assert_eq!(summary.categories.len(), 16);
            assert!(!summary.members_truncated);
            assert!(!summary.categories_truncated);
            assert!(!f.history(0).await.contains("cleared-members-truncated"));
            assert!(!f.history(0).await.contains("cleared-categories-truncated"));
            ids.push(
                f.cleared(
                    0,
                    bounded_post,
                    "dismissed",
                    "BoundedCapturedTitle17",
                    Some((0, 17)),
                )
                .await,
            );
            let page = f.page("moderator", 0).await;
            let group = &page.groups[0];
            let summary = group.moderator_summary.as_ref().unwrap();
            assert_eq!(group.representative.id, ids[16]);
            assert_eq!(summary.report_count, 17);
            assert_eq!(
                summary
                    .additional_members
                    .iter()
                    .map(|r| r.id)
                    .collect::<Vec<_>>(),
                ids.iter()
                    .rev()
                    .skip(1)
                    .take(15)
                    .copied()
                    .collect::<Vec<_>>()
            );
            assert_eq!(
                summary.categories.iter().map(|c| c.id).collect::<Vec<_>>(),
                (1..=16).collect::<Vec<_>>()
            );
            assert!(summary.members_truncated);
            assert!(summary.categories_truncated);
            let html = f.history(0).await;
            assert!(html.contains("cleared-members-truncated"));
            assert!(html.contains("cleared-categories-truncated"));
            assert!(!report_ids(&html).contains(&ids[0]));
            assert!(report_ids(&html).contains(&ids[1]));
            f.role("janitor", &f.boards, &[]).await;
            let html = f.history(0).await;
            assert_eq!(report_ids(&html).len(), 2);
            for hidden in [
                "BoundedCapturedTitle",
                "CapturedHistoricalTitle",
                "DifferentCapturedTitle",
                "CapturedIllegalTitle",
                "UncategorisedHistoricalReason",
                "truncated",
                "report-count",
                "report-categories",
            ] {
                assert!(!html.contains(hidden), "Janitor leaked {hidden}");
            }
        }
    })
    .await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn retained_history_preserves_available_capture_without_fetching_legacy_live_previews() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result = tokio::spawn({ let f = f.clone(); async move {
        let mut expected = Vec::new();
        for (index, target) in ["deleted-post", "deleted-thread", "archived", "expired-archive"].into_iter().enumerate() {
            let post = f.post(0, &format!("LivePrivateBody{index}")).await;
            // Set the target state before inserting synthetic retained rows.
            // Canonical new deletion transitions erase existing reports (0122),
            // which would otherwise mask an accidental live-target SQL filter.
            let sql = match target {
                "deleted-post" => "UPDATE content.posts SET deleted=true WHERE id=$1",
                "deleted-thread" => "UPDATE content.threads SET deleted=true WHERE id=$1",
                "archived" => "UPDATE content.threads SET closed=true,archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1",
                _ => "UPDATE content.threads SET closed=true,archived_at=clock_timestamp()-interval '2 hours',archive_expires_at=clock_timestamp()-interval '1 hour' WHERE id=$1",
            };
            sqlx::query(sql).bind(post).execute(&f.owner).await.unwrap();
            let report = f.cleared(0, post, "resolved", &format!("RetainedHistory{index}"), None).await;
            expected.push((post, report));
        }
        let before = f.snapshot().await;
        let page = f.page("moderator", 0).await;
        assert_eq!(page.groups.iter().map(|g| (g.post_id, g.representative.id)).collect::<Vec<_>>(), expected.iter().rev().copied().collect::<Vec<_>>());
        let html = f.history(0).await;
        assert_eq!(report_ids(&html), expected.iter().rev().map(|(_, id)| *id).collect::<Vec<_>>());
        for index in 0..4 {
            assert!(html.contains(&format!("RetainedHistory{index}")));
            assert_eq!(html.contains(&format!("LivePrivateBody{index}")), index==2,
                "Only the still-available archived post is captured at insertion");
        }
        assert!(!html.contains("<img"));
        assert!(!html.contains("<video"));
        assert_eq!(f.snapshot().await, before);
    }}).await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn strict_named_board_authority_preserves_empty_missing_history_and_private_acl() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result = tokio::spawn({ let f = f.clone(); async move {
        let report = f.cleared(0, f.posts[0], "resolved", "AuthorizedHistory", Some((0, 1))).await;
        let before = f.snapshot().await;
        for query in ["", "?board=", "?board", "?board=a&board=b", "?board=a&extra=b", "?other=a", "?board=%", "?board=%GG", "?board=%FF", "?board=a%00", "?board=A", "?board=a/b", "?board=+", "?board=_ws_", "?board=_nws_", "?board=abcdefghijk", "?board=polls"] {
            let (status, html) = f.request(&format!("/reports/cleared{query}"), None).await;
            assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {html}");
            assert!(!html.contains("AuthorizedHistory"));
        }
        let overlong = format!("/reports/cleared?board={}", "a".repeat(257));
        assert_eq!(f.request(&overlong, None).await.0, StatusCode::BAD_REQUEST);
        let missing = format!("mh{}", &uuid::Uuid::new_v4().simple().to_string()[..8]);
        for (allow, deny) in [
            (vec![f.boards[1].clone()], vec![]),
            (vec!["all".into()], vec![f.boards[0].clone(), missing.clone()]),
        ] {
            f.role("janitor", &allow, &deny).await;
            for board in [&f.boards[0], &missing] {
                assert_eq!(f.request(&format!("/reports/cleared?board={board}"), None).await.0, StatusCode::FORBIDDEN);
                let mut session = f.session("janitor");
                session.permissions.allow_boards = allow.clone();
                session.permissions.deny_boards = deny.clone();
                assert!(matches!(cleared_report_queue::load(&f.staff, &session, board).await, Err(board_staff::AppError::Forbidden)));
            }
        }
        f.role("janitor", &[f.boards[0].clone(), missing.clone()], &[]).await;
        // Cleared history preserves its existing empty result for an authorized
        // missing board. The active queue has a separate 404 contract.
        let missing_history = f.request(&format!("/reports/cleared?board={missing}"), None).await;
        assert_eq!(missing_history.0, StatusCode::OK);
        assert!(missing_history.1.contains("No cleared reports."));
        assert!(report_ids(&missing_history.1).is_empty());
        assert_eq!(report_ids(&f.history(0).await), vec![report]);
        f.role("moderator", &f.boards, &[]).await;
        assert!(report_ids(&f.history(1).await).is_empty());
        // Viewing history does not require the recent-auth gate used for clears.
        sqlx::query("UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp()-interval '11 minutes' WHERE token_hash=$1")
            .bind(auth::hash(&f.token)).execute(&f.owner).await.unwrap();
        assert!(f.history(0).await.contains("AuthorizedHistory"));
        for sql in [
            "SELECT actor_hash,automatic_identity FROM post_secrets.report_membership LIMIT 0",
            "SELECT * FROM post_secrets.report_weight_evidence LIMIT 0",
            "SELECT * FROM post_secrets.report_group LIMIT 0",
            "SELECT * FROM post_secrets.report_catalog_rows LIMIT 0",
            "UPDATE content.reports SET reporter_cleared_at=clock_timestamp() WHERE false",
            "UPDATE content.reports SET group_cleared_at=clock_timestamp() WHERE false",
        ] {
            let error = sqlx::query(sql).execute(&f.staff).await.unwrap_err();
            assert_eq!(error.as_database_error().unwrap().code().as_deref(), Some("42501"), "{sql}");
        }
        assert_eq!(f.snapshot().await, before);
    }}).await;
    f.cleanup().await;
    result.unwrap();
}

// The pause applies only to the test's content pool, after the HTTP authority
// guard. No global hook, relation lock, or runtime timeout override is installed.
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
    assert_eq!(pool.num_idle(), 1);
    (pool, armed, reached, release)
}

#[tokio::test]
async fn history_holds_authority_and_next_read_observes_role_change_and_revocation() {
    let _serial = TEST.lock().await;
    let (pool, armed, reached, release) = gated_pool().await;
    let f = Fixture::with_staff_pool(Some(pool)).await;
    let result = tokio::spawn({
        let f = f.clone();
        async move {
            tokio::time::timeout(Duration::from_secs(10), async {
                let report = f
                    .cleared(
                        0,
                        f.posts[0],
                        "resolved",
                        "GuardedHistoryReason",
                        Some((0, 1)),
                    )
                    .await;
                let path = format!("/reports/cleared?board={}", f.boards[0]);
                armed.store(true, std::sync::atomic::Ordering::SeqCst);
                let read = f.request(&path, None);
                let verify = async {
                    tokio::time::timeout(Duration::from_secs(2), reached.notified())
                        .await
                        .expect("history reached content pool under the authority guard");
                    let mut tx = f.owner.begin().await.unwrap();
                    let error = sqlx::query(
                        "SELECT id FROM staff_identity.accounts WHERE id=$1 FOR UPDATE NOWAIT",
                    )
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
                };
                let ((status, html), ()) = tokio::join!(read, verify);
                assert_eq!(status, StatusCode::OK, "{html}");
                assert_eq!(report_ids(&html), vec![report]);
                assert!(html.contains("GuardedHistoryReason"));
                f.role("janitor", &f.boards, &[]).await;
                let html = f.history(0).await;
                assert_eq!(report_ids(&html), vec![report]);
                assert!(!html.contains("GuardedHistoryReason"));
                sqlx::query(
                    "UPDATE staff_identity.accounts SET revoked_at=clock_timestamp() WHERE id=$1",
                )
                .bind(f.account)
                .execute(&f.owner)
                .await
                .unwrap();
                let (status, html) = f.request(&path, None).await;
                assert_eq!(status, StatusCode::UNAUTHORIZED);
                assert!(report_ids(&html).is_empty());
                assert!(!html.contains("GuardedHistoryReason"));
            })
            .await
            .expect("owned authority checks completed");
        }
    })
    .await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn expiry_during_history_read_prevents_the_rendered_response_from_escaping() {
    let _serial = TEST.lock().await;
    let (pool, armed, reached, release) = gated_pool().await;
    let f = Fixture::with_staff_pool(Some(pool)).await;
    let result = tokio::spawn({ let f = f.clone(); async move {
        tokio::time::timeout(Duration::from_secs(10), async {
            f.cleared(0, f.posts[0], "resolved", "ExpiredHistoryMustNotEscape", Some((0, 1))).await;
            let before = f.snapshot().await;
            sqlx::query("UPDATE staff_identity.sessions SET expires_at=clock_timestamp()+interval '1 second' WHERE token_hash=$1")
                .bind(auth::hash(&f.token)).execute(&f.owner).await.unwrap();
            armed.store(true, std::sync::atomic::Ordering::SeqCst);
            let path = format!("/reports/cleared?board={}", f.boards[0]);
            let read = f.request(&path, None);
            let expire = async {
                tokio::time::timeout(Duration::from_secs(2), reached.notified()).await
                    .expect("history reached content pool before expiry");
                tokio::time::timeout(Duration::from_secs(3), async {
                    loop {
                        let expired: bool = sqlx::query_scalar("SELECT expires_at<=clock_timestamp() FROM staff_identity.sessions WHERE token_hash=$1")
                            .bind(auth::hash(&f.token)).fetch_one(&f.owner).await.unwrap();
                        if expired { break; }
                        tokio::time::sleep(Duration::from_millis(5)).await;
                    }
                }).await.expect("database clock reached the owned session expiry");
                release.notify_one();
            };
            let ((status, html), ()) = tokio::join!(read, expire);
            assert_eq!(status, StatusCode::UNAUTHORIZED, "{html}");
            assert!(!html.contains("ExpiredHistoryMustNotEscape"));
            assert!(report_ids(&html).is_empty());
            assert_eq!(f.snapshot().await, before);
        }).await.expect("owned expiry check completed");
    }}).await;
    f.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn group_selection_member_evidence_and_categories_share_one_read_snapshot() {
    let _serial = TEST.lock().await;
    let f = Fixture::new().await;
    let result = tokio::spawn({ let f = f.clone(); async move {
        let first = f.cleared(0, f.posts[0], "open", "BeforeCapturedCategory", Some((0, 1))).await;
        let selected_id = f.cleared(0, f.posts[0], "resolved", "BeforeCapturedCategory", Some((0, 1))).await;
        f.mark(first, 1_600_000_001, f.account, false).await;
        f.mark(selected_id, 1_600_000_001, f.account, true).await;
        let new_post = f.post(0, "ConcurrentPostContentHidden").await;
        tokio::time::timeout(Duration::from_secs(20), async {
            let selected = tokio::sync::Notify::new();
            let resume = tokio::sync::Notify::new();
            let session = f.session("moderator");
            let read = cleared_report_queue::load_with_selection_checkpoint(
                &f.staff, &session, &f.boards[0], &selected, &resume,
            );
            let write = async {
                tokio::time::timeout(Duration::from_secs(5), selected.notified()).await
                    .expect("group selection established the history snapshot");
                let mut tx = f.owner.begin().await.unwrap();
                sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
                    .bind(&f.boards[0]).execute(&mut *tx).await.unwrap();
                sqlx::query("UPDATE content.reports SET reporter_cleared_at=clock_timestamp() WHERE id=$1")
                    .bind(selected_id).execute(&mut *tx).await.unwrap();
                let mut reports = Vec::new();
                for (post, category, title) in [
                    (f.posts[0], 2_i64, "AfterCapturedCategory"),
                    (new_post, 3_i64, "NewGroupCapturedCategory"),
                ] {
                    let report: i64 = sqlx::query_scalar("INSERT INTO content.reports(board,post_id,state,reason,category_revision,category_id,category_kind,category_base_weight,group_cleared_at,group_cleared_by,group_clear_inherited) VALUES($1,$2,'dismissed',$3,$4,$5,1,0.5,to_timestamp(1700000002),$6,false) RETURNING id")
                        .bind(&f.boards[0]).bind(post).bind(title).bind(f.revisions[0]).bind(category).bind(f.account)
                        .fetch_one(&mut *tx).await.unwrap();
                    reports.push(report);
                }
                tx.commit().await.unwrap();
                resume.notify_one();
                reports
            };
            // Both futures remain owned here; a panic or timeout releases the
            // paused transaction before cleanup rather than detaching a reader.
            let (before, new_reports) = tokio::join!(read, write);
            let before = before.unwrap();
            assert_eq!(before.groups.len(), 1);
            assert!(!before.has_more);
            let group = &before.groups[0];
            assert_eq!(group.representative.id, selected_id);
            assert_eq!(group.representative.state, "resolved");
            assert!(group.representative.group_clear_inherited);
            assert_eq!(group.representative.group_cleared_at.timestamp(), 1_600_000_001);
            let summary = group.moderator_summary.as_ref().unwrap();
            assert_eq!(summary.report_count, 2);
            assert_eq!(summary.additional_members.iter().map(|r| r.id).collect::<Vec<_>>(), vec![first]);
            assert!(!summary.additional_members[0].group_clear_inherited);
            assert_eq!(summary.categories.len(), 1);
            assert_eq!(summary.categories[0].title, "BeforeCapturedCategory");
            assert!(!summary.members_truncated);
            assert!(!summary.categories_truncated);

            let after = f.page("moderator", 0).await;
            assert_eq!(after.groups.len(), 2);
            assert_eq!(after.groups[0].representative.id, new_reports[1]);
            let group = &after.groups[1];
            assert_eq!(group.representative.id, new_reports[0]);
            assert_eq!(group.representative.state, "dismissed");
            assert_eq!(group.representative.group_cleared_at.timestamp(), 1_700_000_002);
            assert!(!group.representative.group_clear_inherited);
            let summary = group.moderator_summary.as_ref().unwrap();
            assert_eq!(summary.report_count, 2);
            assert_eq!(summary.additional_members.iter().map(|r| r.id).collect::<Vec<_>>(), vec![first]);
            assert_eq!(summary.categories.iter().map(|c| (c.id, c.title.as_str())).collect::<Vec<_>>(), vec![(1, "BeforeCapturedCategory"), (2, "AfterCapturedCategory")]);
            assert!(summary.categories.iter().all(|c| c.revision == f.revisions[0] && c.kind == 1));
            assert!(!summary.members_truncated);
            assert!(!summary.categories_truncated);
        }).await.expect("bounded concurrent history snapshot check completed");
    }}).await;
    f.cleanup().await;
    result.unwrap();
}
