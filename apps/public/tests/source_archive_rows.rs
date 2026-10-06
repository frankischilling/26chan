#![cfg(feature = "database-tests")]

#[path = "support/posting.rs"]
mod posting_fixture;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
    response::Response,
};
use board_store::{NewPost, StoreError};
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;

const ORIGIN: &str = "http://127.0.0.1:3000";

async fn request(app: &Router, path: &str, method: &str, tag: Option<&str>) -> Response {
    let mut request = Request::builder().uri(path).method(method);
    if let Some(tag) = tag {
        request = request.header("if-none-match", tag);
    }
    app.clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}
async fn body(response: Response) -> String {
    String::from_utf8(
        to_bytes(response.into_body(), 2 * 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}
fn row(html: &str, id: i64) -> &str {
    html.split("<tr>")
        .find(|row| row.contains(&format!("<td>{id}</td>")))
        .unwrap_or_else(|| panic!("missing archive row {id}"))
        .split("</tr>")
        .next()
        .unwrap()
}
fn teaser(html: &str, id: i64) -> String {
    row(html, id)
        .split_once("<td class=\"teaser-col\">")
        .unwrap()
        .1
        .split_once("</td>")
        .unwrap()
        .0
        .trim()
        .replace("&#39;", "'")
        .replace("&#x27;", "'")
}
async fn post(public: &PgPool, slug: &str, subject: &str, comment: &str, options: &str) -> i64 {
    posting_fixture::create_post_with_metadata(
        public,
        slug,
        0,
        &NewPost {
            name: "Anonymous".into(),
            subject: subject.into(),
            comment: comment.into(),
            deletion_hash: "owned-archive-excerpt-hash".into(),
            sage: false,
        },
        None,
        board_store::PostingContext {
            request_start: chrono::Utc::now(),
            peer: None,
            op_password_proof: None,
        },
        board_store::PostMetadata {
            keys: board_store::PostIdentityKeys {
                tripcode: None,
                poster_id: None,
            },
            country_database: None,
            flag: "",
            options,
            spoiler: false,
        },
    )
    .await
    .unwrap()
}
async fn archive(owner: &PgPool, slug: &str) {
    sqlx::query("UPDATE content.threads SET archived_at=now(),archive_expires_at=now()+interval '1 hour',bumped_at=date_trunc('day',now()) WHERE board=$1")
        .bind(slug).execute(owner).await.unwrap();
}

async fn contract(owner: PgPool, public: PgPool, slug: String) {
    let (web, api) = posting_fixture::routers(public.clone(), &slug, ORIGIN.into(), false);
    let path = format!("/{slug}/archive");
    let empty = body(request(&web, &path, "GET", None).await).await;
    assert!(empty.contains("Displaying 0 expired threads from the past 3 days"));
    assert!(empty.contains("id=\"arc-list\" class=\"flashListing\""));
    assert!(!empty.contains("class=\"teaser-col\""));

    // Spaces prevent the independent word-wrap stage from adding <wbr>, so
    // these lengths independently pin serialized Unicode scalar boundaries.
    let unicode99 = format!("{}界", "界 ".repeat(49));
    let plain94 = format!("{}aa", "a ".repeat(46));
    let ascii99 = format!("{}a", "a ".repeat(49));
    let cases: Vec<(&str, String, String)> = vec![
        ("Subject only", "".into(), "Subject only".into()),
        ("Both", "body".into(), "<b>Both:</b> body".into()),
        ("", "comment only".into(), "comment only".into()),
        (
            "0",
            "zero subject fallback".into(),
            "zero subject fallback".into(),
        ),
        (
            "SPOILER<>literal",
            "".into(),
            "SPOILER&#60;&#62;literal".into(),
        ),
        (
            "\"quoted\"",
            "\"body\"".into(),
            "<b>'quoted':</b> 'body'".into(),
        ),
        ("", "one\n\ntwo".into(), "one two".into()),
        (
            "",
            "[spoiler]secret[/spoiler]".into(),
            "<s>secret</s>".into(),
        ),
        // Source tests length BEFORE strip_tags; the shorter stripped result
        // still receives an ellipsis. Default keep_spoilers is false.
        (
            "",
            format!("[spoiler]{plain94}[/spoiler]"),
            format!("{plain94}…"),
        ),
        ("", unicode99.clone(), unicode99.clone()),
        ("", format!("{unicode99}界"), format!("{unicode99}界")),
        ("", format!("{unicode99}界界"), format!("{unicode99}界…")),
        ("", format!("{ascii99}&"), format!("{ascii99}…")),
        ("", "[sjis]字[/sjis]".into(), "[SJIS]".into()),
        (
            "",
            "<script>alert(1)</script>".into(),
            "&#60;script&#62;alert(1)&#60;/script&#62;".into(),
        ),
    ];
    let mut ids = Vec::new();
    for (subject, comment, expected) in &cases {
        ids.push((
            post(&public, &slug, subject, comment, "").await,
            expected.clone(),
        ));
    }
    sqlx::query("UPDATE content.boards SET word_filter_enabled=true,dice_roll=true,fortune_trip=true WHERE slug=$1")
        .bind(&slug).execute(&owner).await.unwrap();
    let filtered = post(&public, &slug, "", "soy fam CUCK", "").await;
    let dice = post(&public, &slug, "", "ordinary body", "dice 1d1").await;
    let fortune = post(&public, &slug, "", "fortune body", "fortune").await;
    archive(&owner, &slug).await;
    let response = request(&web, &path, "GET", None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers().get("etag").is_none(),
        "HTML keeps its existing cache contract"
    );
    let before = body(response).await;
    let json_response = request(&api, &format!("/{slug}/archive.json"), "GET", None).await;
    let tag = json_response.headers()["etag"].to_str().unwrap().to_owned();
    let _ = body(json_response).await;
    assert!(before.contains("Displaying 18 expired threads from the past 3 days"));
    assert_eq!(before.matches("class=\"teaser-col\"").count(), 18);
    for (id, expected) in &ids {
        assert_eq!(teaser(&before, *id), *expected, "saved OP {id}");
    }
    assert!(!before.contains("<script>alert"));
    assert_eq!(teaser(&before, filtered), "onions senpai KEK");
    assert!(teaser(&before, dice).contains("Rolled 1 (1d1)"));
    assert!(!teaser(&before, fortune).is_empty());
    // Compare each View URL with its independent thread JSON semantic URL.
    // This includes '0' and literal SPOILER<> subjects, where teaser choices
    // differ from original saved subject/comment context.
    let mut all: Vec<i64> = ids.iter().map(|(id, _)| *id).collect();
    all.extend([filtered, dice, fortune]);
    for id in &all {
        let thread: Value = serde_json::from_str(
            &body(request(&api, &format!("/{slug}/thread/{id}.json"), "GET", None).await).await,
        )
        .unwrap();
        let suffix = thread["posts"][0]["semantic_url"]
            .as_str()
            .map(|value| format!("/{value}"))
            .unwrap_or_default();
        assert!(
            row(&before, *id).contains(&format!("href=\"/{slug}/thread/{id}{suffix}\">View</a>"))
        );
    }
    let rendered: Vec<i64> = before
        .split("<tr>")
        .filter_map(|row| {
            let id = row
                .trim_start()
                .strip_prefix("<td>")?
                .split_once("</td>")?
                .0;
            id.parse().ok()
        })
        .collect();
    assert_eq!(rendered, all.iter().rev().copied().collect::<Vec<_>>());
    for router in [&web, &api] {
        let json_body =
            body(request(router, &format!("/{slug}/archive.json"), "GET", None).await).await;
        assert_eq!(
            serde_json::from_str::<Value>(&json_body).unwrap(),
            json!(all)
        );
    }
    for (method, conditional, expected) in [
        ("HEAD", None, 200),
        ("GET", Some(tag.as_str()), 304),
        ("HEAD", Some(tag.as_str()), 304),
    ] {
        let response = request(&web, &format!("/{slug}/archive.json"), method, conditional).await;
        assert_eq!(response.status().as_u16(), expected);
        assert_eq!(response.headers()["etag"], tag);
        assert!(body(response).await.is_empty());
    }
    let head = request(&web, &path, "HEAD", Some(&tag)).await;
    assert_eq!(head.status(), StatusCode::OK);
    assert!(head.headers().get("etag").is_none());
    assert!(body(head).await.is_empty());
    // Today's policy cannot remove/reroll saved filters, randomizers or markup.
    // The source archive-only SJIS replacement still uses current SJIS_TAGS.
    sqlx::query("UPDATE content.boards SET word_filter_enabled=false,dice_roll=false,fortune_trip=false,comment_spoiler_cleanup=false,comment_sjis_spacing=false,comment_code_spacing=false,op_markup=false WHERE slug=$1")
        .bind(&slug).execute(&owner).await.unwrap();
    let after = body(request(&web, &path, "GET", None).await).await;
    let sjis_id = ids[13].0;
    for id in &all {
        if *id != sjis_id {
            assert_eq!(teaser(&after, *id), teaser(&before, *id));
        }
    }
    assert_eq!(teaser(&after, sjis_id), "<span class=\"sjis\">字</span>");

    // HTTP output limits remain fail-closed even for HEAD/conditional GET.
    let limits = board_config::PublicRequestLimits::from_lookup(|name| {
        (name == "PUBLIC_MAX_RESPONSE_BYTES").then(|| "1024".to_owned())
    })
    .unwrap();
    let (limited, _) =
        board_public::routers_with_limits(public.clone(), ORIGIN.into(), false, None, limits);
    for (method, conditional) in [("GET", None), ("HEAD", None), ("GET", Some(tag.as_str()))] {
        let response = request(&limited, &path, method, conditional).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(!body(response).await.contains("teaser-col"));
    }

    // 132 owner-only 64,000-byte legal comments cross 8 MiB in aggregate.
    // SQL constructs these bounded values; the test fetches only their IDs,
    // never allocating a giant request or loading the rejected comment bodies.
    let large: Vec<i64> = sqlx::query_scalar("WITH roots AS (INSERT INTO content.threads(board,bumped_at,archived_at,archive_expires_at) SELECT $1,now(),now(),now()+interval '1 hour' FROM generate_series(1,132) RETURNING id,board), posts AS (INSERT INTO content.posts(id,board,thread_id,name,subject,comment) SELECT id,board,id,'Anonymous','',repeat('𠮷',16000) FROM roots RETURNING id) SELECT id FROM posts")
        .bind(&slug).fetch_all(&owner).await.unwrap();
    for query in [
        "ANALYZE content.boards",
        "ANALYZE content.threads",
        "ANALYZE content.posts",
    ] {
        sqlx::query(query).execute(&owner).await.unwrap();
    }
    assert!(matches!(
        board_store::archive_page_snapshot(&public, &slug).await,
        Err(StoreError::ReadLimit)
    ));
    for method in ["GET", "HEAD"] {
        let response = request(&web, &path, method, Some(&tag)).await;
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
        assert!(!body(response).await.contains("teaser-col"));
    }
    assert_eq!(
        request(&api, &format!("/{slug}/archive.json"), "GET", None)
            .await
            .status(),
        200
    );
    // The byte preflight covers the SELECTED source window, not excluded rows.
    sqlx::query("UPDATE content.threads SET bumped_at=now()-interval '73 hours' WHERE id=ANY($1)")
        .bind(&large)
        .execute(&owner)
        .await
        .unwrap();
    assert!(
        board_store::archive_page_snapshot(&public, &slug)
            .await
            .is_ok()
    );
    let window = body(request(&web, &path, "GET", None).await).await;
    assert!(window.contains("Displaying 18 expired threads from the past 3 days"));
    assert!(!window.contains(&format!("<td>{}</td>", large[0])));
    sqlx::query("UPDATE content.boards SET staff_only=true WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    for router in [&web, &api] {
        for suffix in ["archive", "archive.json"] {
            let response = request(router, &format!("/{slug}/{suffix}"), "GET", Some(&tag)).await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
            assert!(!body(response).await.contains("zero subject fallback"));
        }
    }
}

#[tokio::test]
async fn source_archive_rows_use_saved_typed_excerpts_and_bounded_snapshots() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let seed: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&owner)
        .await
        .unwrap();
    let slug = format!("ar{seed:x}");
    sqlx::query("INSERT INTO content.boards(slug,title,description,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,archive_retention_seconds,archive_limit,comment_code_spacing,comment_spoiler_cleanup,comment_sjis_spacing,op_markup) VALUES($1,'Archive excerpts','Owned regression fixture',0,0,0,16000,100,100,100,20,3600,100,true,true,true,true)")
        .bind(&slug).execute(&owner).await.unwrap();
    let result = tokio::spawn(contract(owner.clone(), public.clone(), slug.clone())).await;
    posting_fixture::cleanup_posting(&owner, &slug).await;
    for query in [
        "DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(query)
            .bind(&slug)
            .execute(&owner)
            .await
            .unwrap();
    }
    public.close().await;
    owner.close().await;
    result.unwrap();
}
