#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    response::Response,
};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use sqlx::PgPool;
use tower::ServiceExt;

const ORIGIN: &str = "http://127.0.0.1:3000";

async fn request(app: &Router, path: &str, method: &str, etag: Option<&str>) -> Response {
    let mut req = Request::builder()
        .uri(path)
        .method(method)
        .header("origin", ORIGIN);
    if let Some(etag) = etag {
        req = req.header("if-none-match", etag);
    }
    app.clone()
        .oneshot(req.body(Body::empty()).unwrap())
        .await
        .unwrap()
}

async fn body(response: Response) -> String {
    String::from_utf8(
        response
            .into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap()
}

async fn contract(owner: PgPool, public: PgPool, slug: String) {
    let (web, api) = board_public::routers(public.clone(), ORIGIN.into(), false);
    let archive = format!("/{slug}/archive.json");
    for app in [&web, &api] {
        let empty = request(app, &archive, "GET", None).await;
        assert_eq!(empty.status(), StatusCode::OK, "enabled empty archive");
        assert_eq!(body(empty).await, "[]");
        assert_eq!(
            request(app, "/unknownarc/archive.json", "GET", None)
                .await
                .status(),
            404
        );
    }
    let new_post = board_store::NewPost {
        name: "Anonymous".into(),
        subject: "<script>archive</script>".into(),
        comment: "Synthetic archive post".into(),
        deletion_hash: "synthetic-unused-hash".into(),
        sage: false,
    };
    let archived = board_store::create_post(&public, &slug, 0, &new_post)
        .await
        .unwrap();
    let reply = board_store::create_post(&public, &slug, archived, &new_post)
        .await
        .unwrap();
    sqlx::query("UPDATE content.threads SET archived_at='2026-01-01T00:00:00Z',archive_expires_at=now()+interval '1 hour',closed=false WHERE id=$1").bind(archived).execute(&owner).await.unwrap();
    let active = board_store::create_post(&public, &slug, 0, &new_post)
        .await
        .unwrap();
    let mut archive_etag = String::new();
    let mut thread_etag = String::new();
    for app in [&web, &api] {
        let response = request(app, &archive, "GET", None).await;
        assert_eq!(response.status(), 200);
        archive_etag = response.headers()["etag"].to_str().unwrap().to_string();
        assert_eq!(
            serde_json::from_str::<Value>(&body(response).await).unwrap(),
            json!([archived])
        );
        let cached = request(app, &archive, "GET", Some(&archive_etag)).await;
        assert_eq!(cached.status(), 304);
        assert!(body(cached).await.is_empty());
        let head = request(app, &archive, "HEAD", None).await;
        assert_eq!(head.status(), 200);
        assert_eq!(head.headers()["etag"], archive_etag);
        assert!(body(head).await.is_empty());
        let response = request(app, &format!("/{slug}/thread/{archived}.json"), "GET", None).await;
        assert_eq!(response.status(), 200);
        thread_etag = response.headers()["etag"].to_str().unwrap().to_string();
        let value: Value = serde_json::from_str(&body(response).await).unwrap();
        assert_eq!(value["posts"][0]["archived"], 1);
        assert_eq!(value["posts"][0]["archived_on"], 1767225600_i64);
        assert_eq!(value["posts"][0]["closed"], 1);
        assert_eq!(value["posts"][1]["no"], reply);
        for field in ["archived", "archived_on", "closed"] {
            assert!(value["posts"][1].get(field).is_none());
        }
        let active_response =
            request(app, &format!("/{slug}/thread/{active}.json"), "GET", None).await;
        let value: Value = serde_json::from_str(&body(active_response).await).unwrap();
        for field in ["archived", "archived_on", "closed"] {
            assert!(value["posts"][0].get(field).is_none());
        }
        let boards: Value =
            serde_json::from_str(&body(request(app, "/boards.json", "GET", None).await).await)
                .unwrap();
        assert_eq!(
            boards["boards"]
                .as_array()
                .unwrap()
                .iter()
                .find(|b| b["board"] == slug)
                .unwrap()["is_archived"],
            1
        );
    }
    let cors = request(&api, &archive, "GET", Some(&archive_etag)).await;
    assert_eq!(cors.headers()["access-control-allow-origin"], ORIGIN);
    assert_eq!(
        cors.headers()["access-control-expose-headers"],
        "ETag, Last-Modified"
    );
    let preflight = api
        .clone()
        .oneshot(
            Request::builder()
                .uri(&archive)
                .method("OPTIONS")
                .header("origin", ORIGIN)
                .header("access-control-request-method", "GET")
                .header("access-control-request-headers", "If-None-Match")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(preflight.status(), 204);
    assert_eq!(preflight.headers()["access-control-allow-origin"], ORIGIN);
    let html = body(request(&web, &format!("/{slug}/archive"), "GET", None).await).await;
    assert!(html.contains(&format!("/{slug}/thread/{archived}")));
    assert!(html.contains("&#60;script&#62;archive&#60;/script&#62;"));
    assert!(!html.contains("<script>"));
    let html = body(request(&web, &format!("/{slug}/thread/{archived}"), "GET", None).await).await;
    assert!(html.contains("archived and read-only"));
    assert!(!html.contains("id=\"postForm\""));
    assert!(html.contains(&format!("action=\"/{slug}/delete\"")));
    assert!(html.contains(&format!("action=\"/{slug}/report\"")));
    for suffix in ["", "catalog", &format!("thread/{active}")] {
        let html = body(request(&web, &format!("/{slug}/{suffix}"), "GET", None).await).await;
        assert!(html.contains(&format!("href=\"/{slug}/archive\"")));
    }
    let denied = web
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/{slug}/post"))
                .method("POST")
                .header("origin", ORIGIN)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(Body::from(format!(
                    "resto={archived}&com=Rejected+archive+reply&password=synthetic-password"
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(denied.status(), 409);
    for query in [
        "UPDATE content.threads SET deleted=true WHERE id=$1",
        "UPDATE content.threads SET deleted=false,archive_expires_at=now()-interval '1 second' WHERE id=$1",
    ] {
        sqlx::query(query)
            .bind(archived)
            .execute(&owner)
            .await
            .unwrap();
        for app in [&web, &api] {
            let response = request(app, &archive, "GET", Some(&archive_etag)).await;
            assert_eq!(response.status(), 200);
            assert_eq!(body(response).await, "[]");
            assert_eq!(
                request(
                    app,
                    &format!("/{slug}/thread/{archived}.json"),
                    "GET",
                    Some(&thread_etag)
                )
                .await
                .status(),
                404
            );
        }
        assert_eq!(
            request(&web, &format!("/{slug}/thread/{archived}"), "GET", None)
                .await
                .status(),
            404
        );
    }
    sqlx::query(
        "UPDATE content.threads SET archive_expires_at=now()+interval '1 hour' WHERE id=$1",
    )
    .bind(archived)
    .execute(&owner)
    .await
    .unwrap();
    sqlx::query("UPDATE content.boards SET archive_retention_seconds=0 WHERE slug=$1")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    for app in [&web, &api] {
        assert_eq!(
            request(app, &archive, "GET", Some(&archive_etag))
                .await
                .status(),
            404
        );
        let boards: Value =
            serde_json::from_str(&body(request(app, "/boards.json", "GET", None).await).await)
                .unwrap();
        assert!(
            boards["boards"]
                .as_array()
                .unwrap()
                .iter()
                .find(|b| b["board"] == slug)
                .unwrap()
                .get("is_archived")
                .is_none()
        );
    }
    assert_eq!(
        request(&web, &format!("/{slug}/archive"), "GET", None)
            .await
            .status(),
        404
    );
    let html = body(request(&web, &format!("/{slug}/"), "GET", None).await).await;
    assert!(!html.contains(&format!("href=\"/{slug}/archive\"")));
}

#[tokio::test]
async fn archive_routes_preserve_visibility_cache_fields_and_read_only_html() {
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
    let slug = format!("p{seed:x}");
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,archive_retention_seconds) VALUES ($1,'Archive fixture','Synthetic owned data',100,20,10,10,10,3600)").bind(&slug).execute(&owner).await.unwrap();
    let result = tokio::spawn(contract(owner.clone(), public.clone(), slug.clone())).await;
    for query in [
        "DELETE FROM content.reports WHERE board=$1",
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
