#![cfg(feature = "database-tests")]

use axum::{body::Body, http::Request};
use http_body_util::BodyExt;
use rand_core::{OsRng, RngCore};
use sqlx::PgPool;
use tower::ServiceExt;

async fn submit(
    app: &axum::Router,
    path: &str,
    parent: i64,
    option: &str,
    comment: &str,
) -> axum::response::Response {
    let fields = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("resto", &parent.to_string())
        .append_pair("email", option)
        .append_pair("com", comment)
        .append_pair("password", "synthetic-password-123")
        .finish();
    app.clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("origin", "http://127.0.0.1:3000")
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(fields))
                .unwrap(),
        )
        .await
        .unwrap()
}

async fn exercise(owner: PgPool, public: PgPool, slug: String) {
    let app = board_public::router(public.clone(), "http://127.0.0.1:3000".into(), false);
    let mut failures = Vec::new();
    let mut first_thread = None;
    for (alias_index, alias) in ["post", "imgboard.php"].into_iter().enumerate() {
        let path = format!("/{slug}/{alias}");
        for (option, sage, board_return) in [
            ("", false, false),
            ("sage", true, false),
            ("nonoko", false, true),
            ("nonokosage", true, true),
            ("NONOKO", false, true),
            ("sageNONOKOSaGe", true, true),
            ("nonoko sage", true, false),
            ("nonokononokosage", true, false),
            ("message", true, false),
            ("<script>fold</script>", false, false),
            ("ordinary options", false, false),
        ] {
            let comment = format!("owned-{alias_index}-{option}-op");
            let response = submit(&app, &path, 0, option, &comment).await;
            if response.status() != 303 {
                failures.push(format!("{alias} {option}: {}", response.status()));
                continue;
            }
            let id: i64 =
                sqlx::query_scalar("SELECT id FROM content.posts WHERE board=$1 AND comment=$2")
                    .bind(&slug)
                    .bind(&comment)
                    .fetch_one(&public)
                    .await
                    .unwrap();
            first_thread.get_or_insert(id);
            let expected = if board_return {
                format!("/{slug}/")
            } else {
                format!("/{slug}/thread/{id}#p{id}")
            };
            assert_eq!(response.headers()["location"], expected);
            assert_eq!(response.headers()["cache-control"], "no-store");
            let post = board_store::find_post(&public, &slug, id).await.unwrap();
            assert_eq!(post.thread_id, id);
            assert_eq!(post.name, "Anonymous");
            // A fixed old timestamp proves bump suppression without sleeps.
            sqlx::query("UPDATE content.threads SET bumped_at='2026-01-01T00:00:00Z' WHERE id=$1")
                .bind(id)
                .execute(&owner)
                .await
                .unwrap();
            let before = board_store::thread(&public, &slug, id).await.unwrap();
            let reply_comment = format!("owned-{alias_index}-{option}-reply");
            let reply = submit(&app, &path, id, option, &reply_comment).await;
            assert_eq!(reply.status(), 303, "{alias} {option}");
            let reply_id: i64 =
                sqlx::query_scalar("SELECT id FROM content.posts WHERE board=$1 AND comment=$2")
                    .bind(&slug)
                    .bind(&reply_comment)
                    .fetch_one(&public)
                    .await
                    .unwrap();
            let expected = if board_return {
                format!("/{slug}/")
            } else {
                format!("/{slug}/thread/{id}#p{reply_id}")
            };
            assert_eq!(reply.headers()["location"], expected);
            let after = board_store::thread(&public, &slug, id).await.unwrap();
            assert_eq!(after.reply_count, 1);
            assert_eq!(
                after.bumped_at == before.bumped_at,
                sage,
                "{alias} {option}"
            );
            if !sage {
                assert!(after.bumped_at > before.bumped_at);
            }
            let posts = board_store::posts(&public, &slug, id).await.unwrap();
            assert_eq!(posts.len(), 2);
            assert_eq!(posts[1].id, reply_id);
        }
        for invalid in ["a".repeat(101), format!("{}a", "😀".repeat(25))] {
            let before: i64 =
                sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1")
                    .bind(&slug)
                    .fetch_one(&public)
                    .await
                    .unwrap();
            let response =
                submit(&app, &path, 0, &invalid, "invalid-option-must-not-persist").await;
            assert_eq!(response.status(), 422);
            assert!(!response.headers().contains_key("location"));
            let after: i64 =
                sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1")
                    .bind(&slug)
                    .fetch_one(&public)
                    .await
                    .unwrap();
            assert_eq!(before, after);
        }
    }
    assert!(
        failures.is_empty(),
        "documented options rejected: {failures:?}"
    );
    let id = first_thread.unwrap();
    let has_editor = |path: String| {
        let app = app.clone();
        async move {
            let response = app
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            let bytes = response.into_body().collect().await.unwrap().to_bytes();
            std::str::from_utf8(&bytes)
                .unwrap()
                .contains("class=\"postEditor\"")
        }
    };
    assert!(has_editor(format!("/{slug}/thread/{id}")).await);
    sqlx::query("UPDATE content.threads SET closed=true WHERE id=$1")
        .bind(id)
        .execute(&owner)
        .await
        .unwrap();
    assert!(!has_editor(format!("/{slug}/thread/{id}")).await);
    assert!(has_editor(format!("/{slug}/")).await);
    for alias in ["post", "imgboard.php"] {
        let response = submit(
            &app,
            &format!("/{slug}/{alias}"),
            id,
            "nonokosage",
            "closed-thread-must-not-persist",
        )
        .await;
        assert_eq!(response.status(), 409);
        assert!(!response.headers().contains_key("location"));
    }
    assert_eq!(
        board_store::posts(&public, &slug, id).await.unwrap().len(),
        2
    );
    sqlx::query("UPDATE content.threads SET closed=false WHERE id=$1")
        .bind(id)
        .execute(&owner)
        .await
        .unwrap();
    assert!(has_editor(format!("/{slug}/thread/{id}")).await);
    let accepted = submit(
        &app,
        &format!("/{slug}/imgboard.php"),
        id,
        &"😀".repeat(25),
        "100-byte-options-reply",
    )
    .await;
    assert_eq!(accepted.status(), 303);
    assert_eq!(
        board_store::posts(&public, &slug, id).await.unwrap().len(),
        3
    );
    for (option, expected_name) in [
        ("capcode_admin", "Anonymous"),
        ("SaGecapcode_mod", "Anonymous"),
        ("CAPCODE_admin", "Named control"),
    ] {
        let fields = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("resto", &id.to_string())
            .append_pair("email", option)
            .append_pair("name", "Named control")
            .append_pair("com", "Owned unprivileged option")
            .append_pair("password", "synthetic-password-123")
            .finish();
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/{slug}/imgboard.php"))
                    .header("origin", "http://127.0.0.1:3000")
                    .header("content-type", "application/x-www-form-urlencoded")
                    .body(Body::from(fields))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 303);
        let post_id: i64 = response.headers()["location"]
            .to_str()
            .unwrap()
            .rsplit_once("#p")
            .unwrap()
            .1
            .parse()
            .unwrap();
        assert_eq!(
            board_store::find_post(&public, &slug, post_id)
                .await
                .unwrap()
                .name,
            expected_name
        );
    }
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/{slug}/thread/{id}.json"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let value: serde_json::Value =
        serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert!(
        value["posts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|post| post.get("capcode").is_none())
    );
}

#[tokio::test]
async fn documented_options_preserve_posts_redirects_and_bump_rules() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut random = [0_u8; 5];
    OsRng.fill_bytes(&mut random);
    let slug: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Posting options','Owned posting fixture',1000,100,50,100,10)")
        .bind(&slug).execute(&owner).await.unwrap();
    let result = tokio::spawn(exercise(owner.clone(), public.clone(), slug.clone())).await;
    public.close().await;
    sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)")
        .bind(&slug).execute(&owner).await.unwrap();
    sqlx::query("DELETE FROM content.posts WHERE board=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.threads WHERE board=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.boards WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    owner.close().await;
    result.unwrap();
}
