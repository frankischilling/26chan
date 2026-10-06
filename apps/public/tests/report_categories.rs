#![cfg(feature = "database-tests")]

#[path = "support/posting.rs"]
mod posting;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use board_domain::anonymous_session::Capability;
use serde_json::json;
use sqlx::PgPool;
use tower::ServiceExt;

const ORIGIN: &str = "http://127.0.0.1:3000";

fn request(
    path: &str,
    fields: Option<&[(&str, &str)]>,
    multipart: bool,
    cookie: Option<&str>,
    peer: u8,
) -> Request<Body> {
    let mut request = if fields.is_some() {
        Request::post(path).header("origin", ORIGIN)
    } else {
        Request::get(path)
    };
    if let Some(cookie) = cookie {
        request = request.header(header::COOKIE, cookie);
    }
    let body = match fields {
        None => String::new(),
        Some(fields) if multipart => {
            request = request.header(
                header::CONTENT_TYPE,
                "multipart/form-data; boundary=owned-categories",
            );
            let mut body = String::new();
            for (name, value) in fields {
                body.push_str(&format!("--owned-categories\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"));
            }
            body.push_str("--owned-categories--\r\n");
            body
        }
        Some(fields) => {
            request = request.header(header::CONTENT_TYPE, "application/x-www-form-urlencoded");
            url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(fields.iter().copied())
                .finish()
        }
    };
    let mut request = request.body(Body::from(body)).unwrap();
    request
        .extensions_mut()
        .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
            [192, 0, 2, peer],
            40000,
        ))));
    request
}

async fn response(
    app: &Router,
    request: Request<Body>,
) -> (StatusCode, axum::http::HeaderMap, String) {
    let response = app.clone().oneshot(request).await.unwrap();
    let (parts, body) = response.into_parts();
    (
        parts.status,
        parts.headers,
        String::from_utf8(to_bytes(body, 1_048_576).await.unwrap().to_vec()).unwrap(),
    )
}

async fn rejected(app: &Router, request: Request<Body>, message: Option<&str>) {
    let (status, headers, body) = response(app, request).await;
    assert!(status.is_client_error(), "{status}: {body}");
    assert_ne!(
        status,
        StatusCode::TOO_MANY_REQUESTS,
        "The transport limiter must not mask report validation: {body}"
    );
    assert!(
        !headers.contains_key(header::SET_COOKIE),
        "Rejected reports cannot publish an identity"
    );
    assert!(!body.contains("data-result=\"success\""));
    if let Some(message) = message {
        assert!(body.contains(message), "{body}");
    }
}

async fn activate(owner: &PgPool, revision: Option<i64>) {
    sqlx::query("SELECT content.set_report_catalog_active($1)")
        .bind(revision)
        .execute(owner)
        .await
        .unwrap();
}

async fn state(owner: &PgPool, board: &str) -> (i64, String) {
    let reports = sqlx::query_scalar("SELECT count(*) FROM content.reports WHERE board=$1")
        .bind(board)
        .fetch_one(owner)
        .await
        .unwrap();
    let sessions = sqlx::query_scalar("SELECT coalesce(jsonb_agg(to_jsonb(s) ORDER BY s.token_hash),'[]'::jsonb)::text FROM post_secrets.anonymous_sessions s").fetch_one(owner).await.unwrap();
    (reports, sessions)
}

// This test changes the global opt-in mode and commits one immutable catalog.
// Run only in the owned disposable database used by the database-test harness,
// separately from tests which expect the default inactive mode. Never point
// these credentials at an operator's deployment or reusable catalog database.
#[tokio::test]
async fn categorical_http_mode_is_read_only_until_an_authoritative_valid_submission() {
    let cluster =
        std::env::var("BOARD_TEST_CLUSTER").expect("Fresh disposable BOARD_TEST_CLUSTER required");
    let suffix = cluster
        .strip_prefix("/tmp/board-postgres.")
        .expect("Refusing non-disposable database cluster");
    assert!(
        suffix.len() == 8 && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric()),
        "Refusing non-disposable database cluster"
    );
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let role: (String, String) = sqlx::query_as("SELECT session_user::text,current_user::text")
        .fetch_one(&owner)
        .await
        .unwrap();
    assert_eq!(role, ("board_migrator".into(), "board_migrator".into()));
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT 'rc'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) VALUES($1,'Owned categorical HTTP','Synthetic',2000,100,100,100,10,0,0,0)").bind(&board).execute(&owner).await.unwrap();
    let (a, p, b) = (owner.clone(), public.clone(), board.clone());
    // Catch assertion panics so cleanup always restores the inactive mode.
    let outcome = tokio::spawn(async move {
        let app = posting::router(p.clone(), &b, ORIGIN.into(), false);
        let mut targets = Vec::new();
        let mut existing_cookie = String::new();
        for index in 0..4 {
            let comment = format!("Owned categorical target {index}");
            let (status, headers, body) = response(&app, request(&format!("/{b}/post"), Some(&[("com", &comment)]), false, None, 200 + index)).await;
            assert_eq!(status, StatusCode::SEE_OTHER, "{body}");
            let location = headers[header::LOCATION].to_str().unwrap();
            targets.push(location.rsplit('/').next().unwrap().split('#').next().unwrap().to_owned());
            existing_cookie = headers.get_all(header::SET_COOKIE).iter().find_map(|h| h.to_str().unwrap().strip_prefix("board-anon=").map(|v| format!("board-anon={}", v.split(';').next().unwrap()))).unwrap();
        }
        let no = &targets[0];
        assert_eq!(board_store::category_form(&p, &b, no.parse().unwrap()).await.unwrap().revision, None);
        let long_title = "L".repeat(4096);
        let html_title = "<script>alert(\"owned\")</script>&";
        let rows = [(7, html_title), (8, ""), (9, long_title.as_str()), (31, "<b>Owned illegal</b>")].map(|(id, title)| json!({"id":id,"board":"","op_only":false,"reply_only":false,"image_only":false,"exclude_boards":null,"title":title,"weight":1.25,"filtered":19}));
        let revision: i64 = sqlx::query_scalar("SELECT content.import_report_catalog($1::text::jsonb)").bind(json!({"version":1,"categories":rows}).to_string()).fetch_one(&a).await.unwrap();
        activate(&a, Some(revision)).await;
        let revision = revision.to_string();
        let path = format!("/{b}/imgboard.php?mode=report&no={no}");
        let unknown = format!("board-anon={}", Capability::generate().unwrap().credential());
        let baseline = state(&a, &b).await;
        for cookie in [None, Some(existing_cookie.as_str()), Some(unknown.as_str())] {
            let (status, headers, body) = response(&app, request(&path, None, false, cookie, 210)).await;
            assert_eq!(status, StatusCode::OK, "{body}");
            assert!(!headers.contains_key(header::SET_COOKIE));
            assert_eq!(headers[header::CACHE_CONTROL], "private, no-store");
            assert!(body.contains(&format!("action=\"/{b}/imgboard.php?mode=report&amp;no={no}\"")));
            assert!(body.contains(&format!("name=\"revision\" value=\"{revision}\"")));
            assert!(body.contains("name=\"cat_id\""));
            assert!(body.contains("name=\"cat\" value=\"31\""));
            assert!(body.contains("&#60;script&#62;") && body.contains("&#60;/script&#62;&#38;"));
            assert!(!body.contains(html_title));
            assert!(body.contains(&long_title), "Titles must not be truncated");
            assert!(body.contains("<option value=\"8\">Category 8</option>"));
            assert!(!body.contains("name=\"reason\"") && !body.contains("base_weight") && !body.contains("filtered"));
        }
        assert_eq!(state(&a, &b).await, baseline, "GET must not allocate sessions or update activity");
        // Independent parser cases use distinct trusted transport peers so each
        // reaches form/category validation under the unchanged production limiter.
        // Peers 200+ remain reserved for posting and intentional identity/quota cases.
        let mut parser_peer = 0_u8;
        let mut next_parser_peer = || {
            parser_peer = parser_peer.checked_add(1).expect("Parser peer budget exhausted");
            assert!(parser_peer < 200, "Parser cases must not overlap identity fixtures");
            parser_peer
        };
        for multipart in [false, true] {
            for category in ["00", "01", "+7", " 7", "7 ", "-7", "7.0", "7e0", "9223372036854775808", "garbage"] {
                rejected(&app, request(&path, Some(&[("cat", category), ("cat_id", "7"), ("revision", &revision)]), multipart, None, next_parser_peer()), None).await;
            }
            for fields in [
                vec![("cat_id", "7"), ("cat_id", "7"), ("revision", &revision)],
                vec![("cat", "7"), ("cat", "31"), ("revision", &revision)],
                vec![("cat_id", "7"), ("revision", &revision), ("revision", &revision)],
                vec![("cat_id", "7"), ("revision", &revision), ("mode", "regist")],
                vec![("cat_id", "7"), ("revision", &revision), ("board", "other")],
                vec![("cat_id", "7"), ("revision", &revision), ("no", &targets[1])],
                vec![("cat_id", "7"), ("revision", &revision), ("reason", "bypass")],
                vec![("cat_id", "7"), ("revision", &revision), ("token", "forged")],
                vec![("cat_id", "7"), ("revision", "01")],
                vec![("cat_id", "7")],
            ] {
                rejected(&app, request(&path, Some(&fields), multipart, None, next_parser_peer()), None).await;
            }
            for suffix in ["&no=1", "&mode=regist", "&board=other"] {
                rejected(&app, request(&format!("{path}{suffix}"), Some(&[("cat_id", "7"), ("revision", &revision)]), multipart, None, next_parser_peer()), None).await;
            }
            rejected(&app, request(&path, Some(&[("cat_id", "999"), ("revision", &revision)]), multipart, None, next_parser_peer()), Some("Invalid category selected.")).await;
            rejected(&app, request(&path, Some(&[("cat_id", "7"), ("revision", "9223372036854775807")]), multipart, None, next_parser_peer()), Some("Report categories changed.")).await;
        }
        for bad_no in ["0", "01", "-1", "1.0", "9223372036854775808"] {
            let invalid_path = format!("/{b}/imgboard.php?mode=report&no={bad_no}");
            rejected(&app, request(&invalid_path, None, false, None, next_parser_peer()), None).await;
            rejected(&app, request(&invalid_path, Some(&[("cat_id", "7"), ("revision", &revision)]), false, None, next_parser_peer()), None).await;
        }
        rejected(&app, request(&format!("/{b}/report"), Some(&[("no", no), ("reason", "Free-text bypass")]), false, None, next_parser_peer()), None).await;
        rejected(&app, request(&format!("/{b}/report"), Some(&[("no", no), ("cat_id", "7"), ("revision", &revision)]), false, None, next_parser_peer()), None).await;
        assert_eq!(state(&a, &b).await, baseline, "Rejected submissions must roll back sessions and reports");
        let ambiguous = format!("{unknown}; {existing_cookie}");
        let missing = format!("/{b}/imgboard.php?mode=report&no=9223372036854775807");
        let (status, headers, _) = response(&app, request(&missing, Some(&[("cat_id", "7"), ("revision", &revision)]), false, Some(&ambiguous), next_parser_peer())).await;
        assert_eq!(status, StatusCode::NOT_FOUND, "Target policy must precede ambiguous cookies");
        assert!(!headers.contains_key(header::SET_COOKIE));
        rejected(&app, request(&path, Some(&[("cat_id", "7"), ("revision", &revision)]), false, Some(&ambiguous), next_parser_peer()), None).await;
        // Actual no-JavaScript form shapes, including PHP's falsey string "0".
        for (index, (cat, category, expected)) in [(Some(""), "7", 7_i64), (Some("0"), "8", 8), (Some("31"), "garbage", 31), (None, "9", 9)].into_iter().enumerate() {
            let path = format!("/{b}/imgboard.php?mode=report&no={}", targets[index]);
            let mut fields = vec![("cat_id", category), ("revision", revision.as_str())];
            if let Some(cat) = cat { fields.push(("cat", cat)); }
            // Body target hints are optional, but must match when included.
            if index != 0 { fields.extend([("mode", "report"), ("board", b.as_str()), ("no", targets[index].as_str())]); }
            let (status, headers, body) = response(&app, request(&path, Some(&fields), index % 2 == 1, None, 220 + index as u8)).await;
            assert_eq!(status, StatusCode::OK, "{body}");
            assert!(body.contains("data-result=\"success\""));
            assert!(headers.get_all(header::SET_COOKIE).iter().any(|v| v.to_str().unwrap().starts_with("board-anon=")));
            let stored: (i64, i64, String) = sqlx::query_as("SELECT category_id,category_revision,reason FROM content.reports WHERE board=$1 AND post_id=$2").bind(&b).bind(targets[index].parse::<i64>().unwrap()).fetch_one(&a).await.unwrap();
            assert_eq!(stored.0, expected);
            assert_eq!(stored.1.to_string(), revision);
            assert_eq!(stored.2, if expected == 7 { html_title } else if expected == 8 { "" } else if expected == 9 { long_title.as_str() } else { "<b>Owned illegal</b>" });
            // A known duplicate outranks stale or invalid category selection.
            rejected(&app, request(&path, Some(&[("cat_id", "999"), ("revision", "9223372036854775807")]), false, None, 220 + index as u8), Some("already reported this post")).await;
        }
        activate(&a, None).await;
        rejected(&app, request(&path, Some(&[("cat_id", "7"), ("revision", &revision)]), false, None, 230), Some("Categorical reporting is not active.")).await;
        let (status, _, body) = response(&app, request(&path, None, false, None, 230)).await;
        assert_eq!(status, StatusCode::OK);
        assert!(body.contains("name=\"reason\""));
    }).await;
    // Restore mode before any other cleanup which could itself fail.
    activate(&owner, None).await;
    let tokens: Vec<Vec<u8>> = sqlx::query_scalar("SELECT a.token_hash FROM post_secrets.anonymous_posts a JOIN content.posts p ON p.id=a.post_id WHERE p.board=$1 UNION SELECT a.token_hash FROM post_secrets.anonymous_reports a JOIN content.reports r ON r.id=a.report_id WHERE r.board=$1").bind(&board).fetch_all(&owner).await.unwrap();
    posting::cleanup_posting(&owner, &board).await;
    for query in [
        "DELETE FROM content.reports WHERE board=$1",
        "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(query)
            .bind(&board)
            .execute(&owner)
            .await
            .unwrap();
    }
    for token in tokens {
        sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
            .bind(token)
            .execute(&owner)
            .await
            .unwrap();
    }
    outcome.unwrap();
}
