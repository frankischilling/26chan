#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::Body,
    http::{HeaderMap, Request, StatusCode},
};
use http_body_util::BodyExt;
use rand_core::{OsRng, RngCore};

use tower::ServiceExt;

async fn response(
    app: &Router,
    method: &str,
    path: &str,
    etag: Option<&str>,
) -> (StatusCode, HeaderMap, Vec<u8>) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("origin", "http://127.0.0.1:3000");
    if let Some(etag) = etag {
        request = request.header("if-none-match", etag);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::empty()).unwrap())
        .await
        .unwrap();
    let (parts, body) = response.into_parts();
    (
        parts.status,
        parts.headers,
        body.collect().await.unwrap().to_bytes().to_vec(),
    )
}

async fn page(app: &Router, path: &str) -> (serde_json::Value, String) {
    let (status, headers, bytes) = response(app, "GET", path, None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(headers["content-type"], "application/json");
    assert!(!headers.contains_key("last-modified"));
    assert!(!headers.contains_key("set-cookie"));
    assert!(bytes.len() <= 4_194_304);
    (
        serde_json::from_slice(&bytes).unwrap(),
        headers["etag"].to_str().unwrap().to_owned(),
    )
}

#[tokio::test]
async fn native_pages_keep_coherent_source_previews_exact_ids_and_private_authority() {
    let owner = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut random = [0; 4];
    OsRng.fill_bytes(&mut random);
    let token = u32::from_le_bytes(random);
    let board = format!("np{token:08x}");
    let base = (1i64 << 55) + i64::from(token) * 100;
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit,archive_retention_seconds,archive_limit,comment_spoiler_cleanup) VALUES($1,'Owned <img> directory title','Owned page snapshots',4000,1000,300,20,1,7,3600,10,true)")
        .bind(&board).execute(&owner).await.unwrap();
    let fixture = board.clone();
    let admin = owner.clone();
    let result = tokio::spawn(async move {
        for (offset, sticky, deleted, archived) in [(0,false,false,false),(10,true,false,false),(20,false,false,false),(30,true,true,false),(40,false,false,true)] {
            let id = base + offset;
            sqlx::query("INSERT INTO content.threads(id,board,bumped_at,sticky,deleted,archived_at,archive_expires_at) VALUES($1,$2,to_timestamp(1800000000+$3),$4,$5,CASE WHEN $6 THEN clock_timestamp() END,CASE WHEN $6 THEN clock_timestamp()+interval '1 hour' END)")
                .bind(id).bind(&fixture).bind(offset as f64).bind(sticky).bind(deleted).bind(archived).execute(&admin).await.unwrap();
            sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','Owned <script>subject</script>','[spoiler]<img src=x onerror=bad()>[/spoiler]')")
                .bind(id).bind(&fixture).execute(&admin).await.unwrap();
        }
        for offset in 1..=6 {
            sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment,deleted) VALUES($1,$2,$3,'Anonymous','','Owned earlier reply',$4)")
                .bind(base+10+offset).bind(&fixture).bind(base+10).bind(offset==5).execute(&admin).await.unwrap();
        }
        let (app, api) = board_public::routers(public.clone(), "http://127.0.0.1:3000".into(), false);
        let first_path = format!("/_watch/{fixture}/page/0");
        let (first, initial_tag) = page(&app, &first_path).await;
        assert_eq!(first.as_object().unwrap().len(), 6);
        assert_eq!(first["version"], 2); assert_eq!(first["replies_shown"], 5);
        assert_eq!(first["page"], 0); assert_eq!(first["next_page"], 1);
        let first_thread = &first["threads"][0];
        assert_eq!(first_thread["thread"], (base+10).to_string());
        assert_eq!(first_thread["sticky"], true);
        assert_eq!(first_thread["replies"], 5); assert_eq!(first_thread["images"], 0); assert_eq!(first_thread["omitted"], 4);
        assert_eq!(first_thread["posts"].as_array().unwrap().iter().map(|post| post["no"].as_str().unwrap()).collect::<Vec<_>>(),
            [base+10,base+16].map(|id| id.to_string()));
        let html = first_thread["posts"][0]["html"].as_str().unwrap();
        assert!(html.contains("<s>")); assert!(!html.contains("<img src=x")); assert!(!html.contains("<script>subject"));
        assert!(html.contains(&format!("action=\"/{fixture}/delete\"")));
        assert!(html.contains(&format!("action=\"/{fixture}/report\"")));
        assert!(!html.contains("password_hash"));
        let head = response(&app, "HEAD", &first_path, None).await;
        assert_eq!(head.0, StatusCode::OK); assert_eq!(head.1["etag"], initial_tag); assert!(head.2.is_empty());
        let cached = response(&app, "GET", &first_path, Some(&initial_tag)).await;
        assert_eq!(cached.0, StatusCode::NOT_MODIFIED); assert!(cached.2.is_empty());
        assert_eq!(response(&api, "GET", &first_path, None).await.0, StatusCode::NOT_FOUND);
        assert_eq!(response(&app, "POST", &first_path, None).await.0, StatusCode::METHOD_NOT_ALLOWED);
        let (second, _) = page(&app, &format!("/_watch/{fixture}/page/1")).await;
        assert_eq!(second["threads"][0]["thread"], (base+20).to_string()); assert_eq!(second["next_page"], 2);
        let (third, _) = page(&app, &format!("/_watch/{fixture}/page/2")).await;
        assert_eq!(third["threads"][0]["thread"], base.to_string()); assert!(third["next_page"].is_null());
        let (empty, _) = page(&app, &format!("/_watch/{fixture}/page/3")).await;
        assert_eq!(empty["threads"], serde_json::json!([]));
        assert!(empty["next_page"].is_null());
        assert_eq!(response(&app, "GET", &format!("/_watch/{fixture}/page/20"), None).await.0, StatusCode::NOT_FOUND);
        let preview_html = response(&app, "GET", &format!("/{fixture}/0"), None).await.2;
        assert!(std::str::from_utf8(&preview_html).unwrap().contains(html));
        let (json_page, _) = page(&app, &format!("/{fixture}/1.json")).await;
        assert_eq!(json_page["threads"][0]["posts"].as_array().unwrap().len(), 2,
            "Public JSON and native HTML share the source sticky-one preview policy");

        let mut tx = admin.begin().await.unwrap();
        sqlx::query("UPDATE content.threads SET sticky=false WHERE id=$1").bind(base+10).execute(&mut *tx).await.unwrap();
        sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1").bind(base+16).execute(&mut *tx).await.unwrap();
        assert_eq!(page(&app, &first_path).await.0, first, "Uncommitted changes cannot leak into a snapshot");
        tx.commit().await.unwrap();
        let changed = response(&app, "GET", &first_path, Some(&initial_tag)).await;
        assert_eq!(changed.0, StatusCode::OK); assert_ne!(changed.1["etag"], initial_tag);
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&changed.2).unwrap()["threads"][0]["thread"], (base+20).to_string());
        let (changed_tail, _) = page(&app, &format!("/_watch/{fixture}/page/1")).await;
        assert_eq!(changed_tail["threads"][0]["replies"], 4);
        assert_eq!(changed_tail["threads"][0]["omitted"], 0);

        let limits = board_config::PublicRequestLimits::from_lookup(|name| {
            (name == "PUBLIC_MAX_RESPONSE_BYTES").then(|| "4096".to_owned())
        }).unwrap();
        let (limited, _) = board_public::routers_with_limits(public, "http://127.0.0.1:3000".into(), false, None, limits);
        sqlx::query("UPDATE content.posts SET comment=repeat('x',16000) WHERE id=$1").bind(base+20).execute(&admin).await.unwrap();
        let blocked = response(&limited, "GET", &first_path, None).await;
        assert_eq!(blocked.0, StatusCode::SERVICE_UNAVAILABLE);
        assert!(!String::from_utf8_lossy(&blocked.2).contains("\"threads\""));

        let (directory, directory_tag) = page(&app, "/_watch/boards").await;
        assert_eq!(directory.as_object().unwrap().len(), 2);
        let entry = directory["boards"].as_array().unwrap().iter().find(|item| item["board"] == fixture).unwrap();
        assert_eq!(entry, &serde_json::json!({"board":fixture,"title":"Owned <img> directory title"}));
        assert!(directory["boards"].as_array().unwrap().len() <= 100);
        assert_eq!(response(&app, "GET", "/_watch/boards", Some(&directory_tag)).await.0, StatusCode::NOT_MODIFIED);
        assert_eq!(response(&api, "GET", "/_watch/boards", None).await.0, StatusCode::NOT_FOUND);
    }).await;
    for statement in [
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(statement)
            .bind(&board)
            .execute(&owner)
            .await
            .unwrap();
    }
    owner.close().await;
    result.unwrap();
}
