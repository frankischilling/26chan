#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{HeaderMap, Request, StatusCode},
};
use rand_core::{OsRng, RngCore};
use sqlx::PgPool;
use tower::ServiceExt;

async fn request(
    app: &Router,
    path: &str,
    headers: &[(&str, &str)],
) -> (StatusCode, HeaderMap, String) {
    let mut request = Request::get(path);
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let body = String::from_utf8(
        to_bytes(response.into_body(), 8 * 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    (status, headers, body)
}

async fn get(app: &Router, path: &str) -> String {
    let (status, _, body) = request(app, path, &[]).await;
    assert_eq!(status, StatusCode::OK, "{path}: {body}");
    body
}

async fn number(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn thread(pool: &PgPool, board: &str) -> i64 {
    sqlx::query_scalar("INSERT INTO content.threads(board) VALUES($1) RETURNING id")
        .bind(board)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn post(pool: &PgPool, board: &str, id: i64, thread: i64, comment: &str) {
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$3,'Anonymous','Quote fixture',$4)")
        .bind(id).bind(board).bind(thread).bind(comment).execute(pool).await.unwrap();
}

fn link(href: &str, label: &str) -> String {
    format!("<a class=\"quotelink\" href=\"{href}\">{label}</a>")
}
fn dead(id: i64) -> String {
    format!("<span class=\"deadlink\">&gt;&gt;{id}</span>")
}
fn comments(value: &serde_json::Value, out: &mut String) {
    match value {
        serde_json::Value::Object(object) => {
            for (key, value) in object {
                if matches!(key.as_str(), "com" | "html") {
                    if let Some(text) = value.as_str() {
                        out.push_str(text);
                    }
                } else {
                    comments(value, out);
                }
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                comments(value, out);
            }
        }
        _ => {}
    }
}
fn rendered(body: &str) -> String {
    if let Ok(value) = serde_json::from_str(body) {
        let mut out = String::new();
        comments(&value, &mut out);
        out
    } else {
        body.into()
    }
}

async fn exercise(owner: PgPool, public: PgPool, board: String, other: String) {
    let source = thread(&owner, &board).await;
    let reply = number(&owner).await;
    let last = number(&owner).await;
    let target = thread(&owner, &board).await;
    let target_reply = number(&owner).await;
    let missing = number(&owner).await;
    let foreign = thread(&owner, &other).await;
    post(&owner, &other, foreign, foreign, "Wrong-board target").await;
    post(&owner, &board, target, target, "Target OP").await;
    post(&owner, &board, target_reply, target, "Target reply").await;
    let text = format!(
        "resolutionneedle >>{source} >>{reply} >>{target} >>{target_reply} >>{missing} >>{foreign} >>0{target} >>>/zzzzzzzzzz/{target} <script>alert(1)</script>"
    );
    post(&owner, &board, source, source, &text).await;
    post(&owner, &board, reply, source, "Intermediate reply").await;
    post(&owner, &board, last, source, &text).await;
    let (web, api) = board_public::routers(public.clone(), "http://127.0.0.1:3000".into(), false);
    let local_op = link(&format!("#p{source}"), &format!("&gt;&gt;{source}"));
    let local_reply = link(&format!("#p{reply}"), &format!("&gt;&gt;{reply}"));
    let remote_op = link(
        &format!("/{board}/thread/{target}#p{target}"),
        &format!("&gt;&gt;{target}"),
    );
    let remote_reply = link(
        &format!("/{board}/thread/{target}#p{target_reply}"),
        &format!("&gt;&gt;{target_reply}"),
    );
    let thread_paths = [
        format!("/{board}/thread/{source}"),
        format!("/_watch/{board}/thread/{source}/posts"),
        format!("/_watch/{board}/thread/{source}/posts-tail"),
        format!("/_watch/{board}/post/{last}"),
    ];
    for path in &thread_paths {
        let html = rendered(&get(&web, path).await);
        for expected in [
            &local_op,
            &local_reply,
            &remote_op,
            &remote_reply,
            &dead(missing),
            &dead(foreign),
        ] {
            assert!(html.contains(expected), "{path}: missing {expected}");
        }
        assert!(html.contains(&link(
            &format!("/{board}/post/{target}"),
            &format!("&gt;&gt;0{target}")
        )));
        assert!(!html.contains(&format!("href=\"/zzzzzzzzzz/post/{target}")));
        assert!(!html.contains("<script>alert(1)</script>"));
    }
    for app in [&web, &api] {
        for suffix in [
            format!("thread/{source}.json"),
            format!("thread/{source}-tail.json"),
        ] {
            let html = rendered(&get(app, &format!("/{board}/{suffix}")).await);
            assert!(html.contains(&local_reply));
            assert!(html.contains(&remote_reply));
            assert!(html.contains(&dead(missing)));
        }
        for suffix in ["1.json", "catalog.json"] {
            let body = get(app, &format!("/{board}/{suffix}")).await;
            let html = rendered(&body);
            assert!(html.contains(&link(
                &format!("/{board}/thread/{source}#p{reply}"),
                &format!("&gt;&gt;{reply}")
            )));
            assert!(html.contains(&remote_reply));
            for private in [
                "quote_targets",
                "comment_format",
                "fingerprint",
                "archive_expires_at",
            ] {
                assert!(!body.contains(private));
            }
        }
    }
    for path in [format!("/{board}/"), format!("/_watch/{board}/page/0")] {
        let html = rendered(&get(&web, &path).await);
        assert!(html.contains(&link(
            &format!("/{board}/thread/{source}#p{reply}"),
            &format!("&gt;&gt;{reply}")
        )));
    }
    let catalog = get(&web, &format!("/{board}/catalog")).await;
    assert!(!catalog.contains("class=\"deadlink\""));
    assert!(catalog.contains(&format!("&#62;&#62;{source}")));
    let search = rendered(&get(&web, &format!("/search/api?q=resolutionneedle&b={board}")).await);
    assert!(!search.contains("class=\"deadlink\""));
    assert!(search.contains(&format!("&gt;&gt;{missing}")));

    // A target changes while the source row and thread remain untouched. Every
    // full/tail API and native representation must revalidate from its body.
    let source_before: String =
        sqlx::query_scalar("SELECT http_modified_at::text FROM content.threads WHERE id=$1")
            .bind(source)
            .fetch_one(&owner)
            .await
            .unwrap();
    let mut paths = Vec::new();
    for which in 0..2 {
        for suffix in [
            format!("thread/{source}.json"),
            format!("thread/{source}-tail.json"),
        ] {
            paths.push((which, format!("/{board}/{suffix}")));
        }
    }
    paths.push((0, format!("/_watch/{board}/thread/{source}/posts")));
    paths.push((0, format!("/_watch/{board}/thread/{source}/posts-tail")));
    paths.push((0, format!("/_watch/{board}/post/{last}")));
    let mut tags = Vec::new();
    for (which, path) in &paths {
        let app = if *which == 0 { &web } else { &api };
        let (_, headers, _) = request(app, path, &[]).await;
        assert!(!headers.contains_key("last-modified"));
        tags.push(headers["etag"].to_str().unwrap().to_owned());
    }
    post(&owner, &board, missing, target, "Previously missing target").await;
    for present in [true, false] {
        if !present {
            sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1")
                .bind(missing)
                .execute(&owner)
                .await
                .unwrap();
        }
        for (index, (which, path)) in paths.iter().enumerate() {
            let app = if *which == 0 { &web } else { &api };
            let (status, headers, body) = request(
                app,
                path,
                &[
                    ("if-none-match", &tags[index]),
                    ("if-modified-since", "Fri, 01 Jan 2100 00:00:00 GMT"),
                ],
            )
            .await;
            assert_eq!(status, StatusCode::OK, "changed dependency: {path}");
            assert_ne!(headers["etag"].to_str().unwrap(), tags[index]);
            let html = rendered(&body);
            let expected = if present {
                link(
                    &format!("/{board}/thread/{target}#p{missing}"),
                    &format!("&gt;&gt;{missing}"),
                )
            } else {
                dead(missing)
            };
            assert!(html.contains(&expected), "{path}: {expected}");
            tags[index] = headers["etag"].to_str().unwrap().into();
            assert_eq!(
                request(
                    app,
                    path,
                    &[
                        ("if-none-match", &tags[index]),
                        ("if-modified-since", "Thu, 01 Jan 1970 00:00:00 GMT")
                    ]
                )
                .await
                .0,
                StatusCode::NOT_MODIFIED
            );
            assert_eq!(
                request(
                    app,
                    path,
                    &[("if-modified-since", "Fri, 01 Jan 2100 00:00:00 GMT")]
                )
                .await
                .0,
                StatusCode::OK
            );
        }
    }
    let source_after: String =
        sqlx::query_scalar("SELECT http_modified_at::text FROM content.threads WHERE id=$1")
            .bind(source)
            .fetch_one(&owner)
            .await
            .unwrap();
    assert_eq!(source_before, source_after);
    for mutation in [
        "UPDATE content.posts SET deleted=true WHERE id=$1",
        "UPDATE content.threads SET deleted=true WHERE id=$1",
        "UPDATE content.threads SET archived_at=clock_timestamp()-interval '2 days',archive_expires_at=clock_timestamp()-interval '1 day' WHERE id=$1",
    ] {
        // Fresh deletion is irreversible. Each visibility transition needs its
        // own owned target instead of reviving an erased fixture.
        let target = thread(&owner, &board).await;
        let target_reply = number(&owner).await;
        post(&owner, &board, target, target, "Independent target").await;
        post(&owner, &board, target_reply, target, "Independent reply").await;
        sqlx::query("UPDATE content.posts SET comment=$2 WHERE id=$1")
            .bind(source)
            .bind(format!(">>{target} >>{target_reply}"))
            .execute(&owner)
            .await
            .unwrap();
        sqlx::query(mutation)
            .bind(target)
            .execute(&owner)
            .await
            .unwrap();
        let html = rendered(&get(&web, &format!("/{board}/thread/{source}.json")).await);
        assert!(html.contains(&dead(target)));
        assert!(html.contains(&dead(target_reply)));
    }
    sqlx::query("UPDATE content.posts SET comment=$2 WHERE id=$1")
        .bind(source)
        .bind(&text)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("UPDATE content.threads SET deleted=true WHERE id=$1")
        .bind(target)
        .execute(&owner)
        .await
        .unwrap();
    // Persisted pre-source profiles retain their historical unconditional links.
    let historical = thread(&owner, &board).await;
    post(
        &owner,
        &board,
        historical,
        historical,
        &format!(">>{target} >>>/other/{target}"),
    )
    .await;
    for format in [0_i16, 8, 24, 40, 56] {
        sqlx::query("UPDATE content.posts SET comment_format=$2 WHERE id=$1")
            .bind(historical)
            .bind(format)
            .execute(&owner)
            .await
            .unwrap();
        let html = rendered(&get(&web, &format!("/{board}/thread/{historical}.json")).await);
        assert!(html.contains(&link(
            &format!("/{board}/post/{target}"),
            &format!("&gt;&gt;{target}")
        )));
        assert!(html.contains(&link(
            &format!("/other/post/{target}"),
            &format!("&gt;&gt;&gt;/other/{target}")
        )));
        assert!(!html.contains("class=\"deadlink\""));
    }
    sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 day' WHERE id=$1")
        .bind(source).execute(&owner).await.unwrap();
    assert!(
        get(&web, &format!("/{board}/thread/{source}"))
            .await
            .contains(&dead(target_reply))
    );
    let archive = get(&web, &format!("/{board}/archive")).await;
    assert!(!archive.contains("class=\"deadlink\""));
    assert!(archive.contains(&format!("&#62;&#62;{source}")));
}

#[tokio::test]
async fn public_quote_resolution_and_dependency_cache_validation() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = PgPool::connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut random = [0_u8; 4];
    OsRng.fill_bytes(&mut random);
    let token: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    let board = format!("q{token}");
    let other = format!("x{token}");
    for slug in [&board, &other] {
        sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,archive_retention_seconds,json_tail_size) VALUES($1,'Quote fixture','Owned quote resolution fixture',2000,100,50,10,10,86400,1)")
            .bind(slug).execute(&owner).await.unwrap();
    }
    let result = tokio::spawn(exercise(
        owner.clone(),
        public.clone(),
        board.clone(),
        other.clone(),
    ))
    .await;
    public.close().await;
    for query in [
        "DELETE FROM content.posts WHERE board=ANY($1)",
        "DELETE FROM content.threads WHERE board=ANY($1)",
        "DELETE FROM content.boards WHERE slug=ANY($1)",
    ] {
        sqlx::query(query)
            .bind([&board, &other])
            .execute(&owner)
            .await
            .unwrap();
    }
    owner.close().await;
    result.unwrap();
}
