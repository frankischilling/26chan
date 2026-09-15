use axum::{
    Router,
    body::{Body, to_bytes},
    http::Request,
    response::Response,
};
use tower::ServiceExt;

const ORIGIN: &str = "http://127.0.0.1:3000";

async fn request(app: &Router, method: &str, path: &str, etag: Option<&str>) -> Response {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("origin", ORIGIN);
    if let Some(etag) = etag {
        request = request.header("if-none-match", etag);
    }
    app.clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

#[tokio::test]
async fn preview_route_has_strict_ids_no_queries_or_writes_and_no_api_listener_route() {
    let pool = sqlx::postgres::PgPoolOptions::new()
        .connect_lazy("postgres://unused:synthetic@127.0.0.1:9/unavailable")
        .unwrap();
    let (web, api) = board_public::routers(pool, ORIGIN.into(), false);
    for key in ["0", "-1", "+1", "01", "1.json", "9223372036854775808"] {
        assert_eq!(
            request(&web, "GET", &format!("/_watch/test/post/{key}"), None)
                .await
                .status(),
            404,
            "{key}"
        );
    }
    for board in ["TEST", "bad!", "longboardname"] {
        assert_eq!(
            request(&web, "GET", &format!("/_watch/{board}/post/1"), None)
                .await
                .status(),
            404,
            "{board}"
        );
    }
    for query in [
        "?",
        "?thread=1",
        "?callback=run",
        "?url=https://example.org",
    ] {
        assert_eq!(
            request(&web, "GET", &format!("/_watch/test/post/1{query}"), None)
                .await
                .status(),
            400
        );
    }
    for method in ["POST", "PUT", "PATCH", "DELETE"] {
        let response = request(&web, method, "/_watch/test/post/1", None).await;
        assert_eq!(response.status(), 405, "{method}");
        assert!(response.headers().get("set-cookie").is_none());
    }
    for method in ["GET", "HEAD"] {
        let response = request(&api, method, "/_watch/test/post/1", None).await;
        assert_eq!(response.status(), 404);
        assert!(response.headers().get("set-cookie").is_none());
        assert!(to_bytes(response.into_body(), 4096).await.is_ok());
    }
}

#[cfg(feature = "database-tests")]
mod database {
    use super::*;
    use board_store::NewPost;
    use rand_core::{OsRng, RngCore};
    use serde_json::Value;
    use sqlx::PgPool;

    async fn json(response: Response) -> Value {
        assert_eq!(response.status(), 200);
        serde_json::from_slice(&to_bytes(response.into_body(), 262_144).await.unwrap()).unwrap()
    }

    async fn coherent_during_commit(owner: &PgPool, slug: &str, id: i64) {
        let public = sqlx::postgres::PgPoolOptions::new()
            .max_connections(1)
            .connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
            .await
            .unwrap();
        let before = board_store::post_snapshot(&public, slug, id).await.unwrap();
        let reader_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&public)
            .await
            .unwrap();
        let mut writer = owner.begin().await.unwrap();
        let writer_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
            .fetch_one(&mut *writer)
            .await
            .unwrap();
        sqlx::query("LOCK TABLE content.posts IN ACCESS EXCLUSIVE MODE")
            .execute(&mut *writer)
            .await
            .unwrap();
        let commit = async {
            // The board read has completed once the second query blocks on the
            // posts table. A transaction with READ COMMITTED is insufficient:
            // it would combine the old board with the newly committed post.
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                loop {
                    let blocked: bool = sqlx::query_scalar("SELECT $1=ANY(pg_blocking_pids($2))")
                        .bind(writer_pid)
                        .bind(reader_pid)
                        .fetch_one(owner)
                        .await
                        .unwrap();
                    if blocked {
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(5)).await;
                }
            })
            .await
            .expect("the public snapshot must reach the owned writer barrier");
            sqlx::query("UPDATE content.boards SET title='After preview commit' WHERE slug=$1")
                .bind(slug)
                .execute(&mut *writer)
                .await
                .unwrap();
            sqlx::query("UPDATE content.threads SET closed=true WHERE id=$1")
                .bind(id)
                .execute(&mut *writer)
                .await
                .unwrap();
            sqlx::query("UPDATE content.posts SET comment='After preview commit' WHERE id=$1")
                .bind(id)
                .execute(&mut *writer)
                .await
                .unwrap();
            writer.commit().await.unwrap();
        };
        let (during, ()) = tokio::join!(board_store::post_snapshot(&public, slug, id), commit);
        let during = during.unwrap();
        assert_eq!(during.board.title, before.board.title);
        assert_eq!(during.thread.closed, before.thread.closed);
        assert_eq!(during.post.comment, before.post.comment);
        let after = board_store::post_snapshot(&public, slug, id).await.unwrap();
        assert_eq!(after.board.title, "After preview commit");
        assert!(after.thread.closed);
        assert_eq!(after.post.comment, "After preview commit");
        // Restore this owned fixture before the separate visibility/HTTP checks.
        sqlx::query("UPDATE content.boards SET title=$2 WHERE slug=$1")
            .bind(slug)
            .bind(before.board.title)
            .execute(owner)
            .await
            .unwrap();
        sqlx::query("UPDATE content.threads SET closed=$2 WHERE id=$1")
            .bind(id)
            .bind(before.thread.closed)
            .execute(owner)
            .await
            .unwrap();
        sqlx::query("UPDATE content.posts SET comment=$2 WHERE id=$1")
            .bind(id)
            .bind(before.post.comment)
            .execute(owner)
            .await
            .unwrap();
        public.close().await;
    }

    async fn exercise(owner: PgPool, public: PgPool, slug: String) {
        let (web, api) = board_public::routers(public.clone(), ORIGIN.into(), false);
        let draft = NewPost {
            name: "Owned <name>".into(),
            subject: "Owned <subject>".into(),
            comment: "<script>window.bad=true</script>\n>Owned text".into(),
            deletion_hash: "private-preview-fixture-hash".into(),
            sage: false,
        };
        let thread = board_store::create_post(&public, &slug, 0, &draft)
            .await
            .unwrap();
        let reply = board_store::create_post(&public, &slug, thread, &draft)
            .await
            .unwrap();
        let sibling = board_store::create_post(&public, &slug, thread, &draft)
            .await
            .unwrap();
        coherent_during_commit(&owner, &slug, thread).await;
        let thread_path = format!("/{slug}/thread/{thread}");
        let html = request(&web, "GET", &thread_path, None).await;
        assert_eq!(html.status(), 200);
        let html = String::from_utf8(
            to_bytes(html.into_body(), 1_000_000)
                .await
                .unwrap()
                .to_vec(),
        )
        .unwrap();
        for id in [thread, reply] {
            let path = format!("/_watch/{slug}/post/{id}");
            let response = request(&web, "GET", &path, None).await;
            assert_eq!(response.status(), 200);
            assert_eq!(response.headers()["content-type"], "application/json");
            assert_eq!(
                response.headers()["cache-control"],
                "public, max-age=0, must-revalidate"
            );
            assert_eq!(response.headers()["x-content-type-options"], "nosniff");
            assert!(response.headers().get("set-cookie").is_none());
            assert!(
                response
                    .headers()
                    .get("access-control-allow-origin")
                    .is_none()
            );
            assert!(response.headers().get("last-modified").is_none());
            let csp = response.headers()["content-security-policy"]
                .to_str()
                .unwrap();
            assert!(csp.contains("script-src 'none'"));
            assert!(csp.contains("connect-src 'none'"));
            let etag = response.headers()["etag"].to_str().unwrap().to_owned();
            let value = json(response).await;
            assert_eq!(value["version"], 1);
            assert_eq!(value["board"], slug);
            assert_eq!(value["thread"], thread.to_string());
            assert_eq!(value["post"]["no"], id.to_string());
            assert_eq!(value["post"]["file_deleted"], false);
            assert!(value.get("posts").is_none());
            let fragment = value["post"]["html"].as_str().unwrap().trim();
            assert!(
                html.contains(fragment),
                "preview must use the normal post renderer"
            );
            assert!(!fragment.contains("<script>"));
            assert!(!fragment.contains("private-preview-fixture-hash"));
            assert!(!fragment.contains(&format!("id=\"pc{sibling}\"")));

            let head = request(&web, "HEAD", &path, None).await;
            assert_eq!(head.status(), 200);
            assert_eq!(head.headers()["etag"], etag);
            assert!(to_bytes(head.into_body(), 1).await.unwrap().is_empty());
            let cached = request(&web, "GET", &path, Some(&etag)).await;
            assert_eq!(cached.status(), 304);
            assert!(to_bytes(cached.into_body(), 1).await.unwrap().is_empty());
            assert_eq!(request(&api, "GET", &path, None).await.status(), 404);
            assert_eq!(
                request(&web, "GET", &format!("/_watch/demo/post/{id}"), None)
                    .await
                    .status(),
                404
            );
        }

        let path = format!("/_watch/{slug}/post/{reply}");
        let original = request(&web, "GET", &path, None).await;
        let etag = original.headers()["etag"].to_str().unwrap().to_owned();
        let before = json(original).await;
        let op_path = format!("/_watch/{slug}/post/{thread}");
        let op_etag = request(&web, "GET", &op_path, None).await.headers()["etag"]
            .to_str()
            .unwrap()
            .to_owned();
        // A state change must be reflected in the shared OP fragment and must
        // not expose a stale cached post once archive visibility expires.
        sqlx::query("UPDATE content.threads SET archived_at=now(),archive_expires_at=now()+interval '1 hour',closed=true WHERE id=$1")
            .bind(thread).execute(&owner).await.unwrap();
        assert_eq!(json(request(&web, "GET", &path, None).await).await, before);
        let archived_op = json(request(&web, "GET", &op_path, Some(&op_etag)).await).await;
        assert!(
            archived_op["post"]["html"]
                .as_str()
                .unwrap()
                .contains("View thread")
        );
        sqlx::query(
            "UPDATE content.threads SET archived_at=now()-interval '1 hour',archive_expires_at=now()-interval '1 second' WHERE id=$1",
        )
        .bind(thread)
        .execute(&owner)
        .await
        .unwrap();
        let expired = request(&web, "GET", &path, Some(&etag)).await;
        assert_eq!(expired.status(), 404);
        assert_eq!(expired.headers()["cache-control"], "no-store");
        sqlx::query("UPDATE content.threads SET archived_at=NULL,archive_expires_at=NULL,closed=false WHERE id=$1")
            .bind(thread).execute(&owner).await.unwrap();
        assert_eq!(request(&web, "GET", &path, None).await.status(), 200);
        board_store::delete_post(&public, &slug, reply)
            .await
            .unwrap();
        assert_eq!(request(&web, "GET", &path, Some(&etag)).await.status(), 404);
        let sibling_path = format!("/_watch/{slug}/post/{sibling}");
        let surviving = request(&web, "GET", &sibling_path, None).await;
        assert_eq!(surviving.status(), 200);
        let sibling_etag = surviving.headers()["etag"].to_str().unwrap().to_owned();
        board_store::delete_post(&public, &slug, thread)
            .await
            .unwrap();
        assert_eq!(
            request(&web, "GET", &sibling_path, Some(&sibling_etag))
                .await
                .status(),
            404
        );
        assert_eq!(
            request(
                &web,
                "GET",
                &format!("/_watch/{slug}/post/{}", i64::MAX),
                None
            )
            .await
            .status(),
            404
        );
    }

    #[tokio::test]
    async fn owned_preview_uses_public_visibility_and_tracks_deletion_and_archive_expiry() {
        let owner = PgPool::connect(
            &std::env::var("MIGRATION_DATABASE_URL").expect("disposable owner database required"),
        )
        .await
        .unwrap();
        let public = board_store::connect_public(
            &std::env::var("TEST_PUBLIC_DATABASE_URL")
                .expect("disposable public database required"),
        )
        .await
        .unwrap();
        let mut random = [0_u8; 5];
        OsRng.fill_bytes(&mut random);
        let slug: String = random.iter().map(|byte| format!("{byte:02x}")).collect();
        sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,archive_retention_seconds,archive_limit) VALUES($1,'Quote previews','Owned preview fixture',4000,100,50,10,10,3600,10)")
            .bind(&slug).execute(&owner).await.unwrap();
        let result = tokio::spawn(exercise(owner.clone(), public.clone(), slug.clone())).await;
        public.close().await;
        for statement in [
            "DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
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
}
