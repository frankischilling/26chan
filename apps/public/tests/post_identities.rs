#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
};
use serde_json::{Value, json};
use tower::ServiceExt;

const ORIGIN: &str = "http://127.0.0.1:3000";

async fn get(app: &Router, path: &str) -> String {
    let response = app
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), 200, "{path}");
    String::from_utf8(
        to_bytes(response.into_body(), 1_000_000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}

async fn post(app: &Router, board: &str, parent: i64, name: &str, index: usize) -> Value {
    let fields = [
        ("mode", "regist".to_owned()),
        ("resto", parent.to_string()),
        ("name", name.to_owned()),
        ("sub", "Owned identity".into()),
        ("com", "Owned harmless comment".into()),
        ("pwd", "owned-deletion-password".into()),
    ];
    let (kind, body) = if index & 1 == 0 {
        (
            "application/x-www-form-urlencoded",
            url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(fields.iter().map(|(key, value)| (*key, value)))
                .finish(),
        )
    } else {
        let mut body = String::new();
        for (name, value) in fields {
            body.push_str(&format!("--owned-identity\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"));
        }
        body.push_str("--owned-identity--\r\n");
        ("multipart/form-data; boundary=owned-identity", body)
    };
    let route = if index & 2 == 0 {
        "post"
    } else {
        "imgboard.php"
    };
    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/{board}/{route}"))
                .header("origin", ORIGIN)
                .header("accept", "application/json")
                .header("content-type", kind)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let text =
        String::from_utf8(to_bytes(response.into_body(), 4096).await.unwrap().to_vec()).unwrap();
    assert!(!text.contains("owned-private-identity-secret"));
    serde_json::from_str(&text).unwrap()
}

#[tokio::test]
async fn identities_persist_across_posting_forms_json_and_escaped_fragments() {
    let owner = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT substr(replace(gen_random_uuid()::text,'-',''),1,10)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Posting identities','Owned synthetic fixture',1000,100,100,100,10)").bind(&board).execute(&owner).await.unwrap();
    let (app, api) = board_public::routers_with_options(
        public.clone(),
        board_public::PublicRouterOptions {
            country_database: None,
            origin: ORIGIN.into(),
            production: false,
            media: None,
            limits: board_config::PublicRequestLimits::default(),
            proxy_uid: None,
            poster_id_key: None,
            tripcode_key: Some(std::sync::Arc::new(
                board_domain::identity::SecureKey::parse(&"1".repeat(64)).unwrap(),
            )),
        },
    );
    let result = post(&app, &board, 0, "User#password", 0).await;
    let thread = result["pid"].as_i64().unwrap();
    let mut ids = vec![thread];
    for (index, raw) in [
        "#password",
        "Plain name",
        "<script>owned</script>#password",
        "User##owned-private-identity-secret",
        "Other##owned-private-identity-secret",
    ]
    .into_iter()
    .enumerate()
    {
        let result = post(&app, &board, thread, raw, index + 1).await;
        assert!(result.get("error").is_none(), "{result}");
        ids.push(result["pid"].as_i64().unwrap());
    }
    let snapshot = board_store::thread_snapshot(&public, &board, thread)
        .await
        .unwrap();
    let saved = &snapshot.posts;
    assert_eq!(saved[0].name, "User");
    assert_eq!(saved[0].trip.as_deref(), Some("!ozOtJW9BFA"));
    assert_eq!(saved[1].name, "");
    assert_eq!(saved[1].trip, saved[0].trip);
    assert_eq!(saved[2].name, "Plain name");
    assert_eq!(saved[2].trip, None);
    assert_eq!(saved[3].name, "<script>owned</script>");
    assert_eq!(saved[3].trip, saved[0].trip);
    assert_eq!(saved[4].name, "User");
    assert_eq!(saved[4].trip.as_ref().unwrap().len(), 13);
    assert_eq!(saved[5].name, "Other");
    assert_eq!(saved[5].trip, saved[4].trip);
    for router in [&app, &api] {
        let data: Value =
            serde_json::from_str(&get(router, &format!("/{board}/thread/{thread}.json")).await)
                .unwrap();
        for (index, saved) in saved.iter().enumerate() {
            if saved.name.is_empty() && saved.trip.is_some() {
                assert!(data["posts"][index].get("name").is_none());
            } else {
                assert_eq!(
                    data["posts"][index]["name"],
                    board_domain::source_html_entities(&saved.name)
                );
            }
            assert_eq!(
                data["posts"][index].get("trip").and_then(Value::as_str),
                saved.trip.as_deref()
            );
        }
        let page: Value =
            serde_json::from_str(&get(router, &format!("/{board}/1.json")).await).unwrap();
        assert_eq!(page["threads"][0]["posts"], data["posts"]);
        let catalog: Value =
            serde_json::from_str(&get(router, &format!("/{board}/catalog.json")).await).unwrap();
        assert_eq!(catalog[0]["threads"][0]["trip"], "!ozOtJW9BFA");
        assert_eq!(
            catalog[0]["threads"][0]["last_replies"][4]["trip"],
            json!(saved[5].trip)
        );
    }
    for path in [
        format!("/{board}/"),
        format!("/{board}/thread/{thread}"),
        format!("/{board}/catalog"),
    ] {
        let html = get(&app, &path).await;
        assert!(
            html.contains("class=\"postertrip\">!ozOtJW9BFA</span>"),
            "{path}"
        );
        assert!(!html.contains("owned-private-identity-secret"));
        assert!(!html.contains("User#password"));
        assert!(!html.contains("<script>owned</script>"));
    }
    for id in ids {
        let preview: Value =
            serde_json::from_str(&get(&app, &format!("/_watch/{board}/post/{id}")).await).unwrap();
        let html = preview["post"]["html"].as_str().unwrap();
        assert!(!html.contains("owned-private-identity-secret"));
        assert!(!html.contains("#password"));
        assert!(!html.contains("<script>owned</script>"));
    }
    // Missing authority rejects a secure identity atomically and does not fall
    // back to publishing its password as an ordinary name.
    let unkeyed = board_public::router(public.clone(), ORIGIN.into(), false);
    let before = board_store::thread(&public, &board, thread).await.unwrap();
    let rejected = post(
        &unkeyed,
        &board,
        thread,
        "User##owned-private-identity-secret",
        0,
    )
    .await;
    assert_eq!(rejected["error"], "Secure tripcodes are unavailable.");
    let after = board_store::thread(&public, &board, thread).await.unwrap();
    assert_eq!(before.reply_count, after.reply_count);
    assert_eq!(before.modified_at, after.modified_at);
    sqlx::query("UPDATE content.boards SET forced_anon=true WHERE slug=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    let accepted = post(
        &unkeyed,
        &board,
        thread,
        "User##owned-private-identity-secret",
        1,
    )
    .await;
    let forced = board_store::find_post(&public, &board, accepted["pid"].as_i64().unwrap())
        .await
        .unwrap();
    assert_eq!(forced.name, "Anonymous");
    assert_eq!(forced.trip, None);
    for query in [
        "DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
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
    public.close().await;
    owner.close().await;
}

#[tokio::test]
async fn source_names_reach_saved_posts_and_every_json_projection() {
    let owner = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT substr(replace(gen_random_uuid()::text,'-',''),1,10)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Source name cases','Owned fixture',1000,100,100,100,10)")
        .bind(&board).execute(&owner).await.unwrap();
    // This corpus exceeds the production write budget through one router.
    // http_limits.rs separately checks the production write limit.
    let limits = board_config::PublicRequestLimits::from_lookup(|name| {
        (name == "PUBLIC_WRITES_PER_MINUTE").then(|| "1000".into())
    })
    .unwrap();
    let (app, api) = board_public::routers_with_options(
        public.clone(),
        board_public::PublicRouterOptions {
            country_database: None,
            origin: ORIGIN.into(),
            production: false,
            media: None,
            limits,
            proxy_uid: None,
            poster_id_key: None,
            tripcode_key: Some(std::sync::Arc::new(
                board_domain::identity::SecureKey::parse(&"1".repeat(64)).unwrap(),
            )),
        },
    );
    let source: Value = serde_json::from_str(include_str!(
        "../../../crates/domain/tests/fixtures/public-name.json"
    ))
    .unwrap();
    let expanded = format!("A{}B", "\t".repeat(63));
    let selected = [
        "Name#かみ",
        "Name#ｋａｍｉ",
        "#password",
        "Name#①a😀",
        "Name#p#q#r",
        "Name##é",
        "Name##€",
        "Name#pa\tssword",
        "＃Name﹟!",
        "<owned>&\"'",
        " ! ! ",
        "Name#password###",
        "Name#",
        expanded.as_str(),
    ];
    let mut checked = 0;
    for group in source["groups"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|group| group["board"] == "g" || group["board"] == "jp")
    {
        sqlx::query("UPDATE content.boards SET comment_code_spacing=$2,comment_sjis_spacing=$3 WHERE slug=$1")
            .bind(&board).bind(group["code"].as_bool().unwrap()).bind(group["sjis"].as_bool().unwrap())
            .execute(&owner).await.unwrap();
        let mut thread = 0;
        let mut expected = Vec::new();
        for case in group["cases"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|case| selected.contains(&case["input"].as_str().unwrap()))
        {
            checked += 1;
            let result = post(
                &app,
                &board,
                thread,
                case["input"].as_str().unwrap(),
                checked,
            )
            .await;
            assert!(result.get("error").is_none(), "{result}");
            let id = result["pid"].as_i64().unwrap();
            if thread == 0 {
                thread = id;
            }
            let saved = board_store::find_post(&public, &board, id).await.unwrap();
            assert_eq!(saved.name, case["name"].as_str().unwrap());
            assert_eq!(saved.trip.as_deref(), case["modern_trip"].as_str());
            let preview: Value =
                serde_json::from_str(&get(&app, &format!("/_watch/{board}/post/{id}")).await)
                    .unwrap();
            let html = preview["post"]["html"].as_str().unwrap();
            // Askama uses decimal references; the source uses named references
            // and a padded apostrophe. Compare exactly after these five aliases.
            let canonical = html
                .replace("&#60;", "&lt;")
                .replace("&#62;", "&gt;")
                .replace("&#38;", "&amp;")
                .replace("&#34;", "&quot;")
                .replace("&#39;", "&#039;");
            assert!(canonical.contains(&format!(
                "<span class=\"name\">{}</span>",
                case["name_html"].as_str().unwrap()
            )));
            assert!(!html.contains("<owned>"));
            expected.push(case);
        }
        assert_eq!(expected.len(), selected.len());
        for router in [&app, &api] {
            let snapshot: Value =
                serde_json::from_str(&get(router, &format!("/{board}/thread/{thread}.json")).await)
                    .unwrap();
            assert_eq!(snapshot["posts"].as_array().unwrap().len(), expected.len());
            for (post, case) in snapshot["posts"].as_array().unwrap().iter().zip(&expected) {
                if case["name"] == "" && !case["modern_trip"].is_null() {
                    assert!(post.get("name").is_none());
                } else {
                    assert_eq!(post["name"], case["name_html"]);
                }
                assert_eq!(
                    post.get("trip").and_then(Value::as_str),
                    case["modern_trip"].as_str()
                );
            }
            let page: Value =
                serde_json::from_str(&get(router, &format!("/{board}/1.json")).await).unwrap();
            let page_thread = page["threads"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["posts"][0]["no"] == thread)
                .unwrap();
            assert_eq!(page_thread["posts"].as_array().unwrap().len(), 6);
            assert_eq!(page_thread["posts"][0]["omitted_posts"], expected.len() - 6);
            let expected_page = std::iter::once(&snapshot["posts"][0])
                .chain(snapshot["posts"].as_array().unwrap()[expected.len() - 5..].iter());
            for (excerpt, full) in page_thread["posts"]
                .as_array()
                .unwrap()
                .iter()
                .zip(expected_page)
            {
                assert_eq!(excerpt["no"], full["no"]);
                assert_eq!(excerpt.get("name"), full.get("name"));
                assert_eq!(excerpt.get("trip"), full.get("trip"));
            }
            let catalog: Value =
                serde_json::from_str(&get(router, &format!("/{board}/catalog.json")).await)
                    .unwrap();
            let root = catalog[0]["threads"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["no"] == thread)
                .unwrap();
            assert_eq!(root["name"], expected[0]["name_html"]);
            assert_eq!(root["last_replies"].as_array().unwrap().len(), 5);
            for (reply, case) in root["last_replies"].as_array().unwrap().iter().zip(
                expected
                    .iter()
                    .rev()
                    .take(5)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev(),
            ) {
                if case["name"] == "" && !case["modern_trip"].is_null() {
                    assert!(reply.get("name").is_none());
                } else {
                    assert_eq!(reply["name"], case["name_html"]);
                }
                assert_eq!(
                    reply.get("trip").and_then(Value::as_str),
                    case["modern_trip"].as_str()
                );
            }
        }
        let last = post(&app, &board, thread, "#password", 0).await;
        assert!(last.get("error").is_none());
        let last_id = last["pid"].as_i64().unwrap();
        let catalog_html = get(&app, &format!("/{board}/catalog")).await;
        assert!(catalog_html.contains(&format!(
            "data-reply-id=\"{last_id}\">Last reply by <span class=\"post-author\"></span> <span class=\"postertrip\">!ozOtJW9BFA</span>"
        )));
        let before = board_store::thread(&public, &board, thread).await.unwrap();
        let rejected = post(&app, &board, thread, &"\"".repeat(43), 0).await;
        assert_eq!(rejected["error"], "Name or subject is too long.");
        let after = board_store::thread(&public, &board, thread).await.unwrap();
        assert_eq!(
            (before.reply_count, before.modified_at),
            (after.reply_count, after.modified_at)
        );
    }
    assert_eq!(checked, 42);
    for query in [
        "DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
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
    public.close().await;
    owner.close().await;
}
