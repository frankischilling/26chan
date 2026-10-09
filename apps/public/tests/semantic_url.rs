#![cfg(feature = "database-tests")]

#[path = "support/posting.rs"]
mod posting_fixture;

use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use board_store::NewPost;
use rand_core::{OsRng, RngCore};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

async fn get_text(app: &Router, path: &str) -> String {
    let response = app
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{path}");
    String::from_utf8(
        to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

async fn get(app: &Router, path: &str) -> Value {
    serde_json::from_str(&get_text(app, path).await).unwrap()
}

fn assert_op_links(html: &str, slug: &str, id: i64, context: Option<&str>) {
    let marker = format!("id=\"pc{id}\"");
    let op = html.split_once(&marker).expect("OP article").1;
    let op = op.split_once("</article>").unwrap().0;
    let base = format!("/{slug}/thread/{id}");
    // Source JSON may retain whitespace which cannot be used as an alias.
    // Keep that JSON value unchanged and use the canonical HTML destination.
    let href = match context {
        Some(context) if !context.contains(char::is_whitespace) => {
            format!("{base}/{context}")
        }
        _ => base.clone(),
    };
    assert!(
        op.contains(&format!("[<a href=\"{href}\">Reply</a>]")),
        "OP {id}: {href}"
    );
    for link in [
        format!("href=\"{base}#p{id}\" title=\"Link to this post\">No.</a>"),
        format!("href=\"{base}?quote={id}#reply\" title=\"Reply to this post\">{id}</a>"),
    ] {
        assert_eq!(op.matches(&link).count(), 2, "desktop/mobile OP {id}");
    }
}

async fn exercise(owner: PgPool, public: PgPool, slug: String) {
    let long = "a".repeat(50);
    let boundary = "a".repeat(49);
    let cases = [
        ("Foo-bar Don't", "ignored", Some("foobar-dont")),
        (boundary.as_str(), "ignored", Some(boundary.as_str())),
        ("tab\tcontext", "ignored", Some("tab\tcontext")),
        ("日本語", "!!!", None),
        ("", "First line\nSecond line", Some("first-line")),
        ("!!!", "Fallback words", Some("fallback-words")),
        (
            long.as_str(),
            "Whole word fallback",
            Some("whole-word-fallback"),
        ),
        ("&lt;Thing&gt;", "ignored", Some("ltthinggt")),
        ("SPOILER<>literal", "ignored", Some("spoilerliteral")),
        ("", "foo[spoiler]bar[/spoiler] baz", Some("foo-bar-baz")),
        (
            "",
            "[spoiler]hidden words[/spoiler] next",
            Some("hidden-words-next"),
        ),
        ("", "https://example.test\nAfter URL", Some("after-url")),
        ("!!!", "???", None),
        (
            "",
            "<strong class=\"x\">literal</strong>",
            Some("strong-classxliteralstrong"),
        ),
        ("", ">>123 hello", Some("123-hello")),
    ];
    let mut ids = Vec::new();
    for (subject, comment, expected) in cases {
        let id = posting_fixture::create_post(
            &public,
            &slug,
            0,
            &NewPost {
                name: "Anonymous".into(),
                subject: subject.into(),
                comment: comment.into(),
                deletion_hash: "semantic-context-fixture".into(),
                sage: false,
            },
        )
        .await
        .unwrap();
        if subject.contains('\t') {
            // Historical source subjects can retain whitespace that today's
            // posting normalizer expands. Seed only this owned post's text.
            sqlx::query("UPDATE content.posts SET subject=$2 WHERE id=$1 AND board=$3")
                .bind(id)
                .bind(subject)
                .bind(&slug)
                .execute(&owner)
                .await
                .unwrap();
        }
        let reply = posting_fixture::create_post(
            &public,
            &slug,
            id,
            &NewPost {
                name: "Anonymous".into(),
                subject: "Reply title".into(),
                comment: "Reply body".into(),
                deletion_hash: "semantic-context-fixture".into(),
                sage: false,
            },
        )
        .await
        .unwrap();
        // Tail publication starts at twice the configured tail size.
        posting_fixture::create_post(
            &public,
            &slug,
            id,
            &NewPost {
                name: "Anonymous".into(),
                subject: String::new(),
                comment: "Second reply".into(),
                deletion_hash: "semantic-context-fixture".into(),
                sage: false,
            },
        )
        .await
        .unwrap();
        ids.push((id, reply, expected));
    }
    // The two existing replies plus four more exceed the default five-reply
    // preview by one. The omission link stays canonical even when the OP's
    // Reply link carries source context.
    for _ in 0..4 {
        posting_fixture::create_post(
            &public,
            &slug,
            ids[0].0,
            &NewPost {
                name: "Anonymous".into(),
                subject: String::new(),
                comment: "Omitted-reply fixture".into(),
                deletion_hash: "semantic-context-fixture".into(),
                sage: false,
            },
        )
        .await
        .unwrap();
    }
    sqlx::query("UPDATE content.boards SET dice_roll=true,fortune_trip=true,word_filter_enabled=true WHERE slug=$1")
        .bind(&slug).execute(&owner).await.unwrap();
    let mut generated = Vec::new();
    for (options, comment, expected) in [
        ("dice 1d1", "ordinary body", Some("rolled-1-1d1")),
        ("fortune", "!!!", None),
        (
            "",
            "[spoiler]soy fam CUCK[/spoiler]",
            Some("onions-senpai-kek"),
        ),
    ] {
        let id = posting_fixture::create_post_with_metadata(
            &public,
            &slug,
            0,
            &NewPost {
                name: "Anonymous".into(),
                subject: String::new(),
                comment: comment.into(),
                deletion_hash: "semantic-context-fixture".into(),
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
        .unwrap();
        let saved = board_store::find_post(&public, &slug, id).await.unwrap();
        match options {
            "dice 1d1" => assert_eq!(saved.dice_result.as_deref(), Some("Rolled 1 (1d1)")),
            "fortune" => {
                assert!(saved.fortune_text.is_some());
                assert!(saved.fortune_color.is_some());
            }
            _ => assert!(saved.wordfilter_payload.is_some()),
        }
        generated.push((id, expected));
    }
    let before: Vec<String> = sqlx::query_scalar(
        "SELECT to_jsonb(p)::text FROM content.posts p WHERE board=$1 ORDER BY id",
    )
    .bind(&slug)
    .fetch_all(&owner)
    .await
    .unwrap();
    // Formatting is read from each saved post rather than today's board flag.
    sqlx::query("UPDATE content.boards SET comment_spoiler_cleanup=false,dice_roll=false,fortune_trip=false,word_filter_enabled=false WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    let (app, api) = posting_fixture::routers(public, &slug, "http://127.0.0.1:3000".into(), false);
    let board_html = get_text(&app, &format!("/{slug}/")).await;
    let page = get(&app, &format!("/_watch/{slug}/page/0")).await;
    assert!(board_html.contains(&format!(
        "<p class=\"omitted\">1 posts omitted. <a href=\"/{slug}/thread/{}\">View thread</a></p>",
        ids[0].0
    )));
    for (id, expected) in ids
        .iter()
        .map(|(id, _, expected)| (*id, *expected))
        .chain(generated.iter().copied())
    {
        let json = get(&app, &format!("/{slug}/thread/{id}.json")).await;
        let context = json["posts"][0].get("semantic_url").and_then(Value::as_str);
        assert_eq!(context, expected, "source JSON OP {id}");
        assert_op_links(&board_html, &slug, id, context);
        let thread_html = get_text(&app, &format!("/{slug}/thread/{id}")).await;
        assert_op_links(&thread_html, &slug, id, context);
        let expected_thread_id = id.to_string();
        let preview = page["threads"]
            .as_array()
            .unwrap()
            .iter()
            .find(|thread| thread["thread"].as_str() == Some(expected_thread_id.as_str()))
            .unwrap();
        assert_op_links(
            preview["posts"][0]["html"].as_str().unwrap(),
            &slug,
            id,
            context,
        );
        let snapshot = get(&app, &format!("/_watch/{slug}/thread/{id}/posts")).await;
        assert_op_links(
            snapshot["posts"][0]["html"].as_str().unwrap(),
            &slug,
            id,
            context,
        );
        let preview = get(&app, &format!("/_watch/{slug}/post/{id}")).await;
        assert_op_links(
            preview["post"]["html"].as_str().unwrap(),
            &slug,
            id,
            context,
        );
    }
    for router in [&app, &api] {
        let catalog = get(router, &format!("/{slug}/catalog.json")).await;
        let index = get(router, &format!("/{slug}/1.json")).await;
        for (id, expected) in &generated {
            let thread = get(router, &format!("/{slug}/thread/{id}.json")).await;
            assert_eq!(
                thread["posts"][0]
                    .get("semantic_url")
                    .and_then(Value::as_str),
                *expected
            );
            let entry = catalog
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|page| page["threads"].as_array().unwrap())
                .find(|entry| entry["no"] == *id)
                .unwrap();
            assert_eq!(entry.get("semantic_url").and_then(Value::as_str), *expected);
        }
        for (id, reply, expected) in &ids {
            let thread = get(router, &format!("/{slug}/thread/{id}.json")).await;
            assert_eq!(
                thread["posts"][0]
                    .get("semantic_url")
                    .and_then(Value::as_str),
                *expected,
                "OP {id}"
            );
            assert_eq!(thread["posts"][1]["no"], *reply);
            assert!(thread["posts"][1].get("semantic_url").is_none());
            let tail = get(router, &format!("/{slug}/thread/{id}-tail.json")).await;
            assert!(tail["posts"][0].get("semantic_url").is_none());
            let entry = catalog
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|page| page["threads"].as_array().unwrap())
                .find(|entry| entry["no"] == *id)
                .unwrap();
            assert_eq!(entry.get("semantic_url").and_then(Value::as_str), *expected);
            assert!(entry["last_replies"][0].get("semantic_url").is_none());
            let preview = index["threads"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["posts"][0]["no"] == *id)
                .unwrap();
            assert_eq!(
                preview["posts"][0]
                    .get("semantic_url")
                    .and_then(Value::as_str),
                *expected
            );
        }
    }
    let after: Vec<String> = sqlx::query_scalar(
        "SELECT to_jsonb(p)::text FROM content.posts p WHERE board=$1 ORDER BY id",
    )
    .bind(&slug)
    .fetch_all(&owner)
    .await
    .unwrap();
    assert_eq!(before, after, "read-derived context must not rewrite posts");
    sqlx::query("UPDATE content.boards SET staff_only=true WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    for router in [&app, &api] {
        for path in [
            format!("/{slug}/thread/{}.json", ids[0].0),
            format!("/{slug}/catalog.json"),
            format!("/{slug}/1.json"),
        ] {
            let response = router
                .clone()
                .oneshot(Request::get(&path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), 404, "private projection: {path}");
        }
    }
}

#[tokio::test]
async fn source_context_agrees_across_json_board_and_watch_html_without_changing_post_controls() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut random = [0u8; 5];
    OsRng.fill_bytes(&mut random);
    let slug: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    // Keep every semantic-context OP for projection comparisons on this owned
    // board without exhausting the unrelated actor thread quota.
    sqlx::query("INSERT INTO content.boards(posting_reply_seconds,posting_image_seconds,posting_thread_seconds,slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,comment_spoiler_cleanup,json_tail_size,user_thread_limit) VALUES(0,0,0,$1,'Semantic context','Owned fixture',2000,100,100,100,20,true,1,100)")
        .bind(&slug).execute(&owner).await.unwrap();
    let result = tokio::spawn(exercise(owner.clone(), public.clone(), slug.clone())).await;
    public.close().await;
    sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)")
        .bind(&slug).execute(&owner).await.unwrap();
    for statement in [
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(statement)
            .bind(&slug)
            .execute(&owner)
            .await
            .unwrap();
    }
    owner.close().await;
    result.unwrap();
}
