#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, StatusCode},
};
use http_body_util::BodyExt;
use rand_core::{OsRng, RngCore};
use sqlx::PgPool;
use tower::ServiceExt;

const ORIGIN: &str = "http://127.0.0.1:3000";

async fn request(
    app: &Router,
    method: &str,
    path: &str,
    origin: &str,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("origin", origin)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let (parts, body) = response.into_parts();
    (
        parts.status,
        parts.headers,
        body.collect().await.unwrap().to_bytes().to_vec(),
    )
}

// Explicit high IDs exercise the actual persisted HTML path without changing
// the shared sequence or coercing a post number through a floating-point value.
async fn seed_thread(owner: &PgPool, board: &str, id: i64) {
    let comment = "Owned semantic route fixture";
    let mut prepared = board_domain::wordfiltered_comment::prepare(
        comment,
        board_domain::comment_markup::MarkupPolicy::default(),
        board_domain::wordfilter::Profile::Global,
        None,
    )
    .unwrap();
    prepared.freeze_format(board);
    let payload: String = prepared
        .encode()
        .unwrap()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let search = board_domain::formatting::plain_text(&board_domain::filtered_formatting::lines(
        &prepared, board,
    ));
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SELECT set_config('board.wordfilter_payload',$1,true),set_config('board.wordfilter_search',$2,true)")
        .bind(payload).bind(search).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO content.threads(id,board) VALUES($1,$2)")
        .bind(id)
        .bind(board)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','Owned semantic route',$3)")
        .bind(id).bind(board).bind(comment).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
}

async fn exercise(owner: PgPool, public: PgPool, board: String, private: String, id: i64) {
    seed_thread(&owner, &board, id).await;
    seed_thread(&owner, &private, id + 1).await;
    let (web, api) = board_public::routers(public, ORIGIN.into(), false);
    let canonical = format!("/{board}/thread/{id}");
    let ordinary = request(&web, "GET", &canonical, ORIGIN).await;
    assert_eq!(ordinary.0, StatusCode::OK);
    assert!(String::from_utf8_lossy(&ordinary.2).contains(&format!("data-thread=\"{id}\"")));
    let json = request(&web, "GET", &format!("{canonical}.json"), ORIGIN).await;
    assert_eq!(json.0, StatusCode::OK);
    let json: serde_json::Value = serde_json::from_slice(&json.2).unwrap();
    assert_eq!(json["posts"][0]["no"].as_i64(), Some(id));
    let context = json["posts"][0]["semantic_url"].as_str().unwrap();
    assert_eq!(context, "owned-semantic-route");
    let reply_link = format!("[<a href=\"{canonical}/{context}\">Reply</a>]");
    for path in [format!("/{board}/"), canonical.clone()] {
        let response = request(&web, "GET", &path, ORIGIN).await;
        assert_eq!(response.0, StatusCode::OK);
        let html = String::from_utf8(response.2).unwrap();
        assert!(html.contains(&reply_link), "exact persisted ID: {path}");
        assert!(html.contains(&format!("href=\"{canonical}#p{id}\"")));
        assert!(html.contains(&format!("href=\"{canonical}?quote={id}#reply\"")));
    }
    for (path, page, preview) in [
        (format!("/_watch/{board}/thread/{id}/posts"), false, false),
        (format!("/_watch/{board}/page/0"), true, false),
        (format!("/_watch/{board}/post/{id}"), false, true),
    ] {
        let response = request(&web, "GET", &path, ORIGIN).await;
        assert_eq!(response.0, StatusCode::OK);
        let value: serde_json::Value = serde_json::from_slice(&response.2).unwrap();
        let post = if page {
            assert_eq!(value["threads"][0]["thread"], id.to_string());
            &value["threads"][0]["posts"][0]
        } else if preview {
            &value["post"]
        } else {
            &value["posts"][0]
        };
        assert_eq!(post["no"], id.to_string());
        let html = post["html"].as_str().unwrap();
        assert!(html.contains(&reply_link), "exact fragment ID: {path}");
        assert!(html.contains(&format!("href=\"{canonical}#p{id}\"")));
        assert!(html.contains(&format!("href=\"{canonical}?quote={id}#reply\"")));
    }

    // These are shapes emitted by cleanup_context_string: single words,
    // lowercased subjects with spaces joined by hyphens, and the 49-byte bound.
    // Context is cosmetic: stale words must not select another thread.
    for context in [
        "owned-semantic-route",
        "archive",
        "catalog",
        "stale-title-123",
        "a",
        &"a".repeat(49),
    ] {
        let alias = format!("{canonical}/{context}");
        assert_eq!(request(&web, "GET", &alias, ORIGIN).await, ordinary);
        let head = request(&web, "HEAD", &alias, ORIGIN).await;
        let canonical_head = request(&web, "HEAD", &canonical, ORIGIN).await;
        assert_eq!(head, canonical_head);
        assert!(head.2.is_empty());
        assert_eq!(
            request(&api, "GET", &alias, ORIGIN).await.0,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            request(&web, "POST", &alias, ORIGIN).await.0,
            StatusCode::METHOD_NOT_ALLOWED
        );
        assert_eq!(
            request(&web, "POST", &alias, "https://untrusted.invalid")
                .await
                .0,
            StatusCode::FORBIDDEN
        );
    }
    // The original query reaches the existing handler unchanged, including its
    // quote selection, invalid/missing targets, and unknown query policy.
    for (query, status) in [
        (format!("?quote={id}"), StatusCode::OK),
        ("?quote=invalid".into(), StatusCode::BAD_REQUEST),
        (format!("?quote={}", id + 1), StatusCode::NOT_FOUND),
        (format!("?quote={id}&quote={id}"), StatusCode::BAD_REQUEST),
        ("?other=ignored".into(), StatusCode::OK),
    ] {
        let response = request(
            &web,
            "GET",
            &format!("{canonical}/owned-semantic-route{query}"),
            ORIGIN,
        )
        .await;
        assert_eq!(response.0, status);
        assert_eq!(
            response,
            request(&web, "GET", &format!("{canonical}{query}"), ORIGIN).await
        );
    }
    for context in [
        "",
        "-word",
        "word-",
        "word--word",
        "Upper",
        "has_under",
        "has.dot",
        "%61",
        "%2F",
        "%2e%2e",
        "%3Cscript%3E",
        "one/two",
        "subject/",
        &"a".repeat(50),
    ] {
        let response = request(&web, "GET", &format!("{canonical}/{context}"), ORIGIN).await;
        assert_eq!(response.0, StatusCode::NOT_FOUND, "{context}");
        assert!(!String::from_utf8_lossy(&response.2).contains("Owned semantic route fixture"));
        assert!(!response.1.contains_key("location"));
        assert!(
            response.1["content-security-policy"]
                .to_str()
                .unwrap()
                .contains("script-src 'none'")
        );
    }
    for key in [
        "0",
        "-1",
        "+1",
        "01",
        "1.0",
        "9223372036854775808",
        &format!("%39{}", &id.to_string()[1..]),
        &format!("{id}.json"),
        &format!("{id}-tail.json"),
    ] {
        assert_eq!(
            request(
                &web,
                "GET",
                &format!("/{board}/thread/{key}/subject"),
                ORIGIN
            )
            .await
            .0,
            StatusCode::NOT_FOUND,
            "{key}"
        );
    }
    for path in [
        format!("/missing216/thread/{id}/subject"),
        format!("/%73{}/thread/{id}/subject", &board[1..]),
        format!("/{private}/thread/{}/subject", id + 1),
        format!("/{board}/thread/{}/subject", id + 1),
    ] {
        assert_eq!(
            request(&web, "GET", &path, ORIGIN).await.0,
            StatusCode::NOT_FOUND,
            "{path}"
        );
    }
    // The canonical JSON route stays JSON, with no inferred semantic alias.
    let json = request(&web, "GET", &format!("{canonical}.json"), ORIGIN).await;
    assert_eq!(json.0, StatusCode::OK);
    assert!(
        json.1["content-type"]
            .to_str()
            .unwrap()
            .starts_with("application/json")
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&json.2).unwrap()["posts"][0]["no"].as_i64(),
        Some(id)
    );
    sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
        .bind(id)
        .execute(&owner)
        .await
        .unwrap();
    let archived = request(&web, "GET", &canonical, ORIGIN).await;
    assert_eq!(archived.0, StatusCode::OK);
    let archived = String::from_utf8(archived.2).unwrap();
    assert!(archived.contains(&format!(
        "[<a href=\"{canonical}/owned-semantic-route\">View thread</a>]"
    )));
    assert!(archived.contains(&format!("href=\"{canonical}#p{id}\"")));
    assert!(!archived.contains("?quote="));
    for (path, preview) in [
        (format!("/_watch/{board}/thread/{id}/posts"), false),
        (format!("/_watch/{board}/post/{id}"), true),
    ] {
        let response = request(&web, "GET", &path, ORIGIN).await;
        assert_eq!(response.0, StatusCode::OK);
        let value: serde_json::Value = serde_json::from_slice(&response.2).unwrap();
        let post = if preview {
            &value["post"]
        } else {
            &value["posts"][0]
        };
        let html = post["html"].as_str().unwrap();
        assert!(html.contains(&format!(
            "[<a href=\"{canonical}/owned-semantic-route\">View thread</a>]"
        )));
        assert!(html.contains(&format!("href=\"{canonical}#p{id}\"")));
        assert!(!html.contains("?quote="));
    }
    sqlx::query("UPDATE content.threads SET deleted=true WHERE id=$1")
        .bind(id)
        .execute(&owner)
        .await
        .unwrap();
    assert_eq!(
        request(&web, "GET", &format!("{canonical}/subject"), ORIGIN)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn persisted_semantic_aliases_keep_canonical_html_and_security_boundaries() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let random = OsRng.next_u32();
    let board = format!("su{random:08x}");
    let private = format!("sp{random:08x}");
    for (slug, staff_only) in [(&board, false), (&private, true)] {
        sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,staff_only,archive_retention_seconds) VALUES($1,'Semantic route fixture','Owned route test',4000,100,75,100,10,$2,3600)")
            .bind(slug).bind(staff_only).execute(&owner).await.unwrap();
    }
    let id = 9_007_199_254_740_993 + i64::from(random) * 2;
    let result = tokio::spawn(exercise(
        owner.clone(),
        public.clone(),
        board.clone(),
        private.clone(),
        id,
    ))
    .await;
    public.close().await;
    for slug in [&board, &private] {
        for statement in [
            "DELETE FROM content.posts WHERE board=$1",
            "DELETE FROM content.threads WHERE board=$1",
            "DELETE FROM content.boards WHERE slug=$1",
        ] {
            sqlx::query(statement)
                .bind(slug)
                .execute(&owner)
                .await
                .unwrap();
        }
    }
    result.unwrap();
}
