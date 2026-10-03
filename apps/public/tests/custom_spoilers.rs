#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

const ORIGIN: &str = "http://127.0.0.1:3000";

async fn get(app: &Router, path: &str) -> Value {
    let response = app
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "{path}");
    serde_json::from_slice(&to_bytes(response.into_body(), 1_048_576).await.unwrap()).unwrap()
}

fn field(actual: &Value, name: &str, expected: &Value) {
    if expected.is_null() {
        assert!(actual.get(name).is_none(), "unexpected {name}: {actual}");
    } else {
        assert_eq!(&actual[name], expected, "{name}");
    }
}

async fn post(app: &Router, board: &str, parent: i64) -> i64 {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs([
            ("resto", parent.to_string()),
            ("pwd", "owned-custom-spoiler-password".into()),
            ("sub", "Owned custom-spoiler metadata".into()),
            ("com", "Owned custom-spoiler text".into()),
        ])
        .finish();
    let response = app
        .clone()
        .oneshot(
            Request::post(format!("/{board}/imgboard.php"))
                .header("origin", ORIGIN)
                .header("accept", "application/json")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let value: Value =
        serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap();
    assert!(value.get("error").is_none(), "{value}");
    value["pid"].as_i64().unwrap()
}

async fn exercise(owner: &PgPool, public: &PgPool, slug: &str) {
    let reference: Value =
        serde_json::from_str(include_str!("fixtures/custom-spoilers.json")).unwrap();
    for board in reference["boards"].as_array().unwrap() {
        let configured = board_store::board(owner, board["board"].as_str().unwrap())
            .await
            .unwrap();
        assert_eq!(
            i64::from(configured.custom_spoiler_count),
            board["count"].as_i64().unwrap()
        );
    }
    let limits = board_config::PublicRequestLimits::from_lookup(|name| {
        (name == "PUBLIC_WRITES_PER_MINUTE").then(|| "1000".into())
    })
    .unwrap();
    let plain =
        board_public::routers_with_limits(public.clone(), ORIGIN.into(), false, None, limits);
    // No decoding occurs here: only router presence and the public JSON projection
    // are under test. The real media pipeline has separate qualification.
    let media = board_public::routers_with_limits(
        public.clone(),
        ORIGIN.into(),
        false,
        Some(
            board_config::PublicMediaSettings::development(
                "127.0.0.1:1",
                &"a".repeat(64),
                "http://localhost:3002",
            )
            .unwrap(),
        ),
        limits,
    );
    for row in reference["policy_cases"].as_array().unwrap() {
        sqlx::query("UPDATE content.boards SET comment_spoiler_cleanup=$2,custom_spoiler_count=$3 WHERE slug=$1")
            .bind(slug).bind(row["enabled"].as_bool().unwrap()).bind(row["count"].as_i64().unwrap() as i32)
            .execute(owner).await.unwrap();
        let op = post(&plain.0, slug, 0).await;
        let reply = post(&plain.0, slug, op).await;
        let last_reply = post(&plain.0, slug, op).await;
        for cap in [0, 1000] {
            sqlx::query("UPDATE content.boards SET image_limit=$2 WHERE slug=$1")
                .bind(slug)
                .bind(cap)
                .execute(owner)
                .await
                .unwrap();
            for app in [&plain.0, &plain.1, &media.0, &media.1] {
                let directory = get(app, "/boards.json").await;
                let board = directory["boards"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|board| board["board"] == slug)
                    .unwrap();
                field(board, "spoilers", &row["board_spoilers"]);
                field(board, "custom_spoilers", &row["board_custom_spoilers"]);
                let thread = get(app, &format!("/{slug}/thread/{op}.json")).await;
                field(
                    &thread["posts"][0],
                    "custom_spoiler",
                    &row["op_metadata_custom_spoiler"],
                );
                assert_eq!(thread["posts"][1]["no"], reply);
                assert!(thread["posts"][1].get("custom_spoiler").is_none());
                assert!(thread["posts"][0].get("tim").is_none());
                let tail = get(app, &format!("/{slug}/thread/{op}-tail.json")).await;
                field(
                    &tail["posts"][0],
                    "custom_spoiler",
                    &row["op_metadata_custom_spoiler"],
                );
                assert_eq!(tail["posts"].as_array().unwrap().len(), 2);
                assert_eq!(tail["posts"][1]["no"], last_reply);
                assert!(tail["posts"][1].get("custom_spoiler").is_none());
                assert!(tail["posts"][0].get("com").is_none());
                assert!(tail["posts"][0].get("name").is_none());
                let index = get(app, &format!("/{slug}/1.json")).await;
                let posts = index["threads"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|thread| thread["posts"][0]["no"] == op)
                    .unwrap();
                field(
                    &posts["posts"][0],
                    "custom_spoiler",
                    &row["op_metadata_custom_spoiler"],
                );
                let catalog = get(app, &format!("/{slug}/catalog.json")).await;
                let entry = catalog
                    .as_array()
                    .unwrap()
                    .iter()
                    .flat_map(|page| page["threads"].as_array().unwrap())
                    .find(|thread| thread["no"] == op)
                    .unwrap();
                field(entry, "custom_spoiler", &row["op_metadata_custom_spoiler"]);
                for last in entry["last_replies"].as_array().unwrap() {
                    assert!(last.get("custom_spoiler").is_none());
                }
            }
        }
    }
    for key in [
        "TEST_PUBLIC_DATABASE_URL",
        "STAFF_DATABASE_URL",
        "AUTH_DATABASE_URL",
        "MEDIA_DATABASE_URL",
        "MEDIA_READ_DATABASE_URL",
        "INTAKE_DATABASE_URL",
        "MONITOR_DATABASE_URL",
    ] {
        let runtime = PgPool::connect(&std::env::var(key).unwrap()).await.unwrap();
        assert!(
            sqlx::query("UPDATE content.boards SET custom_spoiler_count=1 WHERE slug=$1")
                .bind(slug)
                .execute(&runtime)
                .await
                .is_err(),
            "{key} wrote custom-spoiler policy"
        );
        runtime.close().await;
    }
}

#[tokio::test]
async fn custom_spoiler_metadata_matches_source_policy_and_keeps_runtime_authority() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let slug: String =
        sqlx::query_scalar("SELECT 'cs'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,json_tail_size) VALUES($1,'Owned custom spoilers','Synthetic metadata',2000,100,100,100,10,1)").bind(&slug).execute(&owner).await.unwrap();
    let owned = owner.clone();
    let runtime = public.clone();
    let board = slug.clone();
    let outcome = tokio::spawn(async move { exercise(&owned, &runtime, &board).await }).await;
    for query in [
        "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
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
    outcome.unwrap();
}
