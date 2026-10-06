#![cfg(feature = "database-tests")]

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

async fn get(app: &Router, path: &str) -> Value {
    let response = app
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{path}");
    serde_json::from_slice(
        &to_bytes(response.into_body(), 4 * 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap()
}

async fn exercise(owner: PgPool, public: PgPool, slug: String) {
    let long = "a".repeat(50);
    let cases = [
        ("Foo-bar Don't", "ignored", Some("foobar-dont")),
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
        let id = board_store::create_post(
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
        let reply = board_store::create_post(
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
        board_store::create_post(
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
        let id = board_store::create_post_with_metadata(
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
    let (app, api) = board_public::routers(public, "http://127.0.0.1:3000".into(), false);
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
async fn source_context_is_consistent_across_json_projections_and_omits_empty_or_reply_values() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut random = [0u8; 5];
    OsRng.fill_bytes(&mut random);
    let slug: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,comment_spoiler_cleanup,json_tail_size) VALUES($1,'Semantic context','Owned fixture',2000,100,100,100,20,true,1)")
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
