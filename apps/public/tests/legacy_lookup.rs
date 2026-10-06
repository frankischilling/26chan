#![cfg(feature = "database-tests")]

#[path = "support/posting.rs"]
mod posting;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode, header},
};
use serde_json::Value;
use sqlx::{PgConnection, PgPool};
use tower::ServiceExt;

const ORIGIN: &str = "https://lookup.example";

async fn response(
    app: &Router,
    path: &str,
    referer: Option<&str>,
    cookie: Option<&str>,
) -> (StatusCode, HeaderMap, String) {
    let mut builder = Request::get(path);
    if let Some(referer) = referer {
        builder = builder.header(header::REFERER, referer);
    }
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
    }
    let mut request = builder.body(Body::empty()).unwrap();
    request
        .extensions_mut()
        .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
            [192, 0, 2, 216],
            40000,
        ))));
    let (parts, body) = app.clone().oneshot(request).await.unwrap().into_parts();
    (
        parts.status,
        parts.headers,
        String::from_utf8(to_bytes(body, 1_048_576).await.unwrap().to_vec()).unwrap(),
    )
}

fn no_lookup_popup(headers: &HeaderMap, body: &str) {
    assert!(!headers.contains_key(header::SET_COOKIE));
    let csp = headers[header::CONTENT_SECURITY_POLICY].to_str().unwrap();
    assert!(
        !csp.contains("report-popup"),
        "lookup responses cannot acquire the report script capability"
    );
    for marker in [
        "report-popup-context",
        "report-popup-close",
        "report-form",
        "data-result=",
        "done-report-",
    ] {
        assert!(
            !body.contains(marker),
            "lookup unexpectedly contains {marker}"
        );
    }
}

async fn seed_thread(
    c: &mut PgConnection,
    board: &str,
    explicit: Option<(i64, i64)>,
) -> (i64, i64) {
    let (op, reply) = if let Some(ids) = explicit {
        ids
    } else {
        let op = sqlx::query_scalar("SELECT nextval('content.post_number')")
            .fetch_one(&mut *c)
            .await
            .unwrap();
        let reply = sqlx::query_scalar("SELECT nextval('content.post_number')")
            .fetch_one(&mut *c)
            .await
            .unwrap();
        (op, reply)
    };
    sqlx::query("INSERT INTO content.threads(id,board) VALUES($1,$2)")
        .bind(op)
        .bind(board)
        .execute(&mut *c)
        .await
        .unwrap();
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','Synthetic legacy lookup','Owned lookup OP')")
        .bind(op).bind(board).execute(&mut *c).await.unwrap();
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$3,'Anonymous','','Owned lookup reply')")
        .bind(reply).bind(board).bind(op).execute(c).await.unwrap();
    (op, reply)
}

async fn snapshot(owner: &PgPool, boards: &[String]) -> Value {
    let mut tx = owner.begin().await.unwrap();
    let raw: String = sqlx::query_scalar("SELECT jsonb_build_object('boards',(SELECT jsonb_agg(to_jsonb(b) ORDER BY slug) FROM content.boards b WHERE slug=ANY($1)),'threads',(SELECT jsonb_agg(to_jsonb(t) ORDER BY id) FROM content.threads t WHERE board=ANY($1)),'posts',(SELECT jsonb_agg(to_jsonb(p) ORDER BY id) FROM content.posts p WHERE board=ANY($1)),'reports',(SELECT coalesce(jsonb_agg(to_jsonb(r) ORDER BY id),'[]') FROM content.reports r WHERE board=ANY($1)),'sessions',(SELECT coalesce(jsonb_agg(to_jsonb(s) ORDER BY token_hash),'[]') FROM post_secrets.anonymous_sessions s))::text")
        .bind(boards).fetch_one(&mut *tx).await.unwrap();
    let mut result: Value = serde_json::from_str(&raw).unwrap();
    sqlx::query("SET LOCAL ROLE board_report_admission_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    let raw: String = sqlx::query_scalar("SELECT coalesce(jsonb_agg(to_jsonb(m) ORDER BY report_id),'[]')::text FROM post_secrets.report_membership m WHERE board=ANY($1)")
        .bind(boards).fetch_one(&mut *tx).await.unwrap();
    result["report_membership"] = serde_json::from_str(&raw).unwrap();
    tx.rollback().await.unwrap();
    result
}

#[tokio::test]
async fn legacy_res_lookup_is_exact_public_read_only_and_keeps_report_get_separate() {
    assert!(
        std::env::var("BOARD_TEST_CLUSTER").is_ok_and(|path| path
            .strip_prefix("/tmp/board-postgres.")
            .is_some_and(
                |tag| tag.len() == 8 && tag.bytes().all(|byte| byte.is_ascii_alphanumeric())
            )),
        "Committed lookup fixtures require the explicit disposable cluster marker"
    );
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let owner_role: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&owner)
        .await
        .unwrap();
    assert_eq!(owner_role, "board_migrator");
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let actual: (String, String) = sqlx::query_as("SELECT session_user::text,current_user::text")
        .fetch_one(&public)
        .await
        .unwrap();
    assert_eq!(actual, ("board_public".into(), "board_public".into()));
    let seed: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&owner)
        .await
        .unwrap();
    let boards = [format!("lr{seed:x}"), format!("lp{seed:x}")];
    let mut setup = owner.begin().await.unwrap();
    for (board, private) in [(&boards[0], false), (&boards[1], true)] {
        sqlx::query("INSERT INTO content.boards(slug,title,description,staff_only,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,archive_retention_seconds) VALUES($1,'Synthetic lookup board','Owned HTTP fixture',$2,2000,100,100,100,10,3600)")
            .bind(board).bind(private).execute(&mut *setup).await.unwrap();
    }
    let ordinary = seed_thread(&mut setup, &boards[0], None).await;
    // Allocate explicit owned IDs without advancing or resetting the shared
    // post sequence. They are above JavaScript's exact-integer range.
    let large_op = i64::MAX.checked_sub(seed.checked_mul(2).unwrap()).unwrap() - 1;
    let large = seed_thread(&mut setup, &boards[0], Some((large_op, large_op + 1))).await;
    assert!(large.0 > 9_007_199_254_740_991);
    let retained = seed_thread(&mut setup, &boards[0], None).await;
    sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp()-interval '1 minute',archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
        .bind(retained.0).execute(&mut *setup).await.unwrap();
    let expired = seed_thread(&mut setup, &boards[0], None).await;
    sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp()-interval '2 hours',archive_expires_at=clock_timestamp()-interval '1 hour' WHERE id=$1")
        .bind(expired.0).execute(&mut *setup).await.unwrap();
    let deleted_thread = seed_thread(&mut setup, &boards[0], None).await;
    sqlx::query("UPDATE content.threads SET deleted=true WHERE id=$1")
        .bind(deleted_thread.0)
        .execute(&mut *setup)
        .await
        .unwrap();
    let deleted_post = seed_thread(&mut setup, &boards[0], None).await;
    sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1")
        .bind(deleted_post.1)
        .execute(&mut *setup)
        .await
        .unwrap();
    let private = seed_thread(&mut setup, &boards[1], None).await;
    let missing: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&mut *setup)
        .await
        .unwrap();
    setup.commit().await.unwrap();
    let result = tokio::spawn({
        let owner = owner.clone();
        let public = public.clone();
        let boards = boards.clone();
        async move {
            let board = &boards[0];
            let app = posting::router(public.clone(), board, ORIGIN.into(), false);
            let baseline = snapshot(&owner, &boards).await;
            for (post, parent) in [
                (ordinary.0, ordinary.0),
                (ordinary.1, ordinary.0),
                (large.0, large.0),
                (large.1, large.0),
                (retained.0, retained.0),
                (retained.1, retained.0),
            ] {
                assert_eq!(
                    board_store::legacy_res_thread(&public, board, post)
                        .await
                        .unwrap(),
                    parent
                );
                for referer in [
                    None,
                    Some("http://evil.invalid/force-downgrade"),
                    Some("https://elsewhere.invalid/path"),
                ] {
                    let path = format!("/{board}/imgboard.php?res={post}");
                    let (status, headers, body) = response(
                        &app,
                        &path,
                        referer,
                        Some("board-anon=invalid; board-anon=ambiguous"),
                    )
                    .await;
                    assert_eq!(status, StatusCode::MOVED_PERMANENTLY, "{body}");
                    assert_eq!(
                        headers[header::LOCATION],
                        format!("/{board}/thread/{parent}#p{post}")
                    );
                    assert_eq!(headers[header::CACHE_CONTROL], "public, max-age=2");
                    no_lookup_popup(&headers, &body);
                }
            }
            let encoded: String = large
                .1
                .to_string()
                .bytes()
                .map(|byte| format!("%{byte:02X}"))
                .collect();
            let (status, headers, body) = response(
                &app,
                &format!("/{board}/imgboard.php?res={encoded}"),
                None,
                None,
            )
            .await;
            assert_eq!(status, StatusCode::MOVED_PERMANENTLY);
            assert_eq!(
                headers[header::LOCATION],
                format!("/{board}/thread/{}#p{}", large.0, large.1)
            );
            no_lookup_popup(&headers, &body);

            for (slug, post, parent) in [
                (board.as_str(), missing, ordinary.0),
                (board.as_str(), deleted_post.1, deleted_post.0),
                (board.as_str(), deleted_thread.0, deleted_thread.0),
                (board.as_str(), deleted_thread.1, deleted_thread.0),
                (board.as_str(), expired.0, expired.0),
                (board.as_str(), expired.1, expired.0),
                (boards[1].as_str(), private.0, private.0),
                (boards[1].as_str(), private.1, private.0),
                (board.as_str(), private.1, private.0),
            ] {
                assert!(matches!(
                    board_store::legacy_res_thread(&public, slug, post).await,
                    Err(board_store::StoreError::NotFound)
                ));
                let (status, headers, body) = response(
                    &app,
                    &format!("/{slug}/imgboard.php?res={post}"),
                    None,
                    None,
                )
                .await;
                assert_eq!(status, StatusCode::NOT_FOUND, "{body}");
                assert!(!headers.contains_key(header::LOCATION));
                assert_eq!(headers[header::CACHE_CONTROL], "public, max-age=2");
                assert!(
                    !body.contains(&parent.to_string()),
                    "invisible parent must not be disclosed"
                );
                assert!(!body.contains("/thread/"));
                no_lookup_popup(&headers, &body);
            }
            let missing_board = format!("lm{seed:x}");
            let (status, headers, body) = response(
                &app,
                &format!("/{missing_board}/imgboard.php?res={}", ordinary.1),
                None,
                None,
            )
            .await;
            assert_eq!(status, StatusCode::NOT_FOUND);
            assert!(!headers.contains_key(header::LOCATION));
            no_lookup_popup(&headers, &body);
            assert_eq!(snapshot(&owner, &boards).await, baseline);

            for query in [
                "res=",
                "res=0",
                "res=-1",
                "res=01",
                "res=+1",
                "res=%2B1",
                "res=%201",
                "res=1%20",
                "res=1.0",
                "res=1e3",
                "res=9223372036854775808",
                "res=NaN",
                "res=%ZZ",
                "res=1&res=1",
                "res=1&%72es=1",
                "res=1&no=1",
                "res=1&mode=usrdel",
                "res=1&mode=regist",
                "res=1&board=other",
                "res=1&unexpected=1",
            ] {
                let (status, headers, body) =
                    response(&app, &format!("/{board}/imgboard.php?{query}"), None, None).await;
                assert_eq!(status, StatusCode::BAD_REQUEST, "{query}: {body}");
                assert!(!headers.contains_key(header::LOCATION));
                no_lookup_popup(&headers, &body);
            }
            for slug in ["UPPER", "a-b", "abcdefghijk"] {
                let (status, headers, body) = response(
                    &app,
                    &format!("/{slug}/imgboard.php?res={}", ordinary.0),
                    None,
                    None,
                )
                .await;
                assert_eq!(status, StatusCode::BAD_REQUEST);
                assert!(!headers.contains_key(header::LOCATION));
                no_lookup_popup(&headers, &body);
            }
            let long = format!("res={}", "1".repeat(129));
            let (status, headers, body) =
                response(&app, &format!("/{board}/imgboard.php?{long}"), None, None).await;
            assert_eq!(status, StatusCode::BAD_REQUEST);
            no_lookup_popup(&headers, &body);

            let (status, headers, body) = response(
                &app,
                &format!("/{board}/imgboard.php?mode=report&no={}", ordinary.1),
                None,
                None,
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{body}");
            assert_eq!(headers[header::CACHE_CONTROL], "private, no-store");
            assert!(!headers.contains_key(header::LOCATION));
            assert!(!headers.contains_key(header::SET_COOKIE));
            assert!(body.contains("id=\"report-popup-context\""));
            assert!(body.contains("data-result=\"form\""));
            assert!(body.contains("id=\"report-form\""));
            assert!(
                headers[header::CONTENT_SECURITY_POLICY]
                    .to_str()
                    .unwrap()
                    .contains(&format!("script-src {ORIGIN}/static/report-popup.v1.js;"))
            );
            // Explicit report mode keeps its pre-existing error shell. It must
            // still reject conflicting fields instead of redirecting or writing.
            for query in [
                format!("mode=report&no={}&res={}", ordinary.1, ordinary.1),
                format!("res={}&mode=report&no={}", ordinary.1, ordinary.1),
                format!("mode=report&mode=report&no={}", ordinary.1),
            ] {
                let (status, headers, body) =
                    response(&app, &format!("/{board}/imgboard.php?{query}"), None, None).await;
                assert_eq!(status, StatusCode::BAD_REQUEST);
                assert!(!headers.contains_key(header::LOCATION));
                assert!(!headers.contains_key(header::SET_COOKIE));
                assert!(!body.contains("data-result=\"success\""));
            }
            assert_eq!(
                snapshot(&owner, &boards).await,
                baseline,
                "all lookup/report GET paths must remain read-only"
            );
        }
    })
    .await;
    for query in [
        "DELETE FROM content.reports WHERE board=ANY($1)",
        "DELETE FROM content.posts WHERE board=ANY($1)",
        "DELETE FROM content.threads WHERE board=ANY($1)",
        "DELETE FROM content.boards WHERE slug=ANY($1)",
    ] {
        sqlx::query(query)
            .bind(boards.to_vec())
            .execute(&owner)
            .await
            .unwrap();
    }
    public.close().await;
    owner.close().await;
    result.unwrap();
}
