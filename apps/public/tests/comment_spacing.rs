#![cfg(feature = "database-tests")]

use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use board_store::{NewPost, StoreError};
use rand_core::{OsRng, RngCore};
use sqlx::PgPool;
use std::time::Duration;
use tower::ServiceExt;

fn post(comment: &str) -> NewPost {
    NewPost {
        name: "Anonymous".into(),
        subject: String::new(),
        comment: comment.into(),
        deletion_hash: "owned-test-hash".into(),
        sage: false,
    }
}

async fn exercise(owner: PgPool, public: PgPool, slug: String) {
    let thread = board_store::create_post(&public, &slug, 0, &post("Owned OP"))
        .await
        .unwrap();
    // Policy changes apply only to new posts, including an existing unnormalized row.
    let historical = " historical\t  text\r\n\r\n\r\n\r\n ";
    sqlx::query("UPDATE content.posts SET comment=$2 WHERE id=$1")
        .bind(thread)
        .bind(historical)
        .execute(&owner)
        .await
        .unwrap();
    let app = board_public::router(public.clone(), "http://127.0.0.1:3000".into(), false);
    let raw = " \t A\t  B\r\n \r\n　\r\n\t\r\nC <script> \r\n";
    for (index, (code, sjis, expected)) in [
        (false, false, "A B\nC <script>"),
        (true, false, "A      B\n \n \n    \nC <script>"),
        (false, true, "A      B\n \n　\n    \nC <script>"),
        (true, true, "A      B\n \n　\n    \nC <script>"),
    ]
    .into_iter()
    .enumerate()
    {
        sqlx::query("UPDATE content.boards SET comment_code_spacing=$2,comment_sjis_spacing=$3 WHERE slug=$1")
            .bind(&slug).bind(code).bind(sjis).execute(&owner).await.unwrap();
        let id = board_store::create_post(&public, &slug, thread, &post(raw))
            .await
            .unwrap();
        assert_eq!(
            board_store::find_post(&public, &slug, id)
                .await
                .unwrap()
                .comment,
            expected
        );
        // Both public aliases and encodings reach the same persisted policy.
        let route = if index % 2 == 0 {
            "post"
        } else {
            "imgboard.php"
        };
        let fields = [
            ("mode", "regist".to_owned()),
            ("resto", thread.to_string()),
            ("com", raw.to_owned()),
            ("pwd", "owned-password".to_owned()),
        ];
        let (content_type, body) = if index < 2 {
            let body = url::form_urlencoded::Serializer::new(String::new())
                .extend_pairs(fields.iter().map(|(key, value)| (*key, value)))
                .finish();
            ("application/x-www-form-urlencoded", body)
        } else {
            let mut body = String::new();
            for (name, value) in fields {
                body.push_str(&format!("--owned-spacing\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n"));
            }
            body.push_str("--owned-spacing--\r\n");
            ("multipart/form-data; boundary=owned-spacing", body)
        };
        let response = app
            .clone()
            .oneshot(
                Request::post(format!("/{slug}/{route}"))
                    .header("origin", "http://127.0.0.1:3000")
                    .header("accept", "application/json")
                    .header("content-type", content_type)
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let value: serde_json::Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        let id = value["pid"].as_i64().expect("accepted owned post");
        assert_eq!(
            board_store::find_post(&public, &slug, id)
                .await
                .unwrap()
                .comment,
            expected
        );
    }
    assert_eq!(
        board_store::find_post(&public, &slug, thread)
            .await
            .unwrap()
            .comment,
        historical
    );
    let html = app
        .clone()
        .oneshot(
            Request::get(format!("/{slug}/thread/{thread}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(html.status(), 200);
    let html = String::from_utf8(
        to_bytes(html.into_body(), 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(html.contains("&lt;script&gt;"));
    assert!(!html.contains("C <script>"));

    let denied = sqlx::query("UPDATE content.boards SET comment_code_spacing=true,comment_sjis_spacing=true WHERE slug=$1")
        .bind(&slug).execute(&public).await.unwrap_err();
    assert_eq!(
        denied.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );

    // Posting must use the committed policy after its actual board-lock wait.
    for code in [true, false] {
        let mut locked = owner.begin().await.unwrap();
        let pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *locked)
            .await
            .unwrap();
        sqlx::query("UPDATE content.boards SET comment_code_spacing=$2,comment_sjis_spacing=false WHERE slug=$1")
            .bind(&slug).bind(code).execute(&mut *locked).await.unwrap();
        let pending = {
            let public = public.clone();
            let slug = slug.clone();
            tokio::spawn(async move {
                board_store::create_post(&public, &slug, thread, &post("A\t  B")).await
            })
        };
        let observed = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let waiting: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE NOT granted AND $1=ANY(pg_blocking_pids(pid)))")
                    .bind(pid).fetch_one(&owner).await.unwrap();
                if waiting { break; }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }).await;
        locked.commit().await.unwrap();
        let result = pending.await.unwrap();
        observed.expect("actual posting transaction waited on the owned board lock");
        let id = result.unwrap();
        assert_eq!(
            board_store::find_post(&public, &slug, id)
                .await
                .unwrap()
                .comment,
            if code { "A      B" } else { "A B" }
        );
    }

    sqlx::query(
        "UPDATE content.boards SET max_comment_chars=3,comment_code_spacing=true WHERE slug=$1",
    )
    .bind(&slug)
    .execute(&owner)
    .await
    .unwrap();
    let id = board_store::create_post(&public, &slug, thread, &post("A\tB"))
        .await
        .unwrap();
    assert_eq!(
        board_store::find_post(&public, &slug, id)
            .await
            .unwrap()
            .comment,
        "A    B"
    );
    let before = board_store::thread(&public, &slug, thread).await.unwrap();
    assert!(matches!(
        board_store::create_post(&public, &slug, thread, &post(" A  ")).await,
        Err(StoreError::Invalid(_))
    ));
    sqlx::query("UPDATE content.boards SET max_comment_chars=16000 WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    assert!(matches!(
        board_store::create_post(
            &public,
            &slug,
            thread,
            &post(&format!("A{}B", "\t".repeat(4000)))
        )
        .await,
        Err(StoreError::Invalid(_))
    ));
    let after = board_store::thread(&public, &slug, thread).await.unwrap();
    assert_eq!(
        (after.reply_count, after.modified_at, after.http_modified_at),
        (
            before.reply_count,
            before.modified_at,
            before.http_modified_at
        )
    );
}

#[tokio::test]
async fn persisted_source_spacing_uses_locked_operator_policy_and_escaped_rendering() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut random = [0u8; 5];
    OsRng.fill_bytes(&mut random);
    let slug: String = random.iter().map(|b| format!("{b:02x}")).collect();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Spacing','Owned fixture',1000,100,100,10,10)")
        .bind(&slug).execute(&owner).await.unwrap();
    let result = tokio::spawn(exercise(owner.clone(), public.clone(), slug.clone())).await;
    public.close().await;
    sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)").bind(&slug).execute(&owner).await.unwrap();
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
