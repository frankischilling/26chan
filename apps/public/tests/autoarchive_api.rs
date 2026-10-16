#![cfg(feature = "database-tests")]
#[path = "support/posting.rs"]
mod posting;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use board_store::NewPost;
use serde_json::Value;
use sqlx::PgPool;
use tower::ServiceExt;

async fn response(app: &Router, path: &str) -> axum::response::Response {
    app.clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap()
}
async fn json(app: &Router, path: &str) -> Value {
    let r = response(app, path).await;
    assert_eq!(r.status(), StatusCode::OK, "{path}");
    serde_json::from_slice(&to_bytes(r.into_body(), 4 * 1024 * 1024).await.unwrap()).unwrap()
}
fn post() -> NewPost {
    NewPost {
        name: "Anonymous".into(),
        subject: "Owned archive".into(),
        comment: "Synthetic reply".into(),
        deletion_hash: "owned-hash".into(),
        sage: true,
    }
}

async fn exercise(owner: &PgPool, public: &PgPool, slug: &str) {
    let op = posting::create_post(public, slug, 0, &post())
        .await
        .unwrap();
    sqlx::query("INSERT INTO content.posts(board,thread_id,name,subject,comment) SELECT $1,$2,'Anonymous','','Owned historical reply' FROM generate_series(1,1499)").bind(slug).bind(op).execute(owner).await.unwrap();
    sqlx::query("UPDATE content.threads SET reply_count=1499 WHERE board=$1 AND id=$2")
        .bind(slug)
        .bind(op)
        .execute(owner)
        .await
        .unwrap();
    let (web, api) = board_public::routers_with_options(
        public.clone(),
        board_public::PublicRouterOptions {
            origin: "http://127.0.0.1:3000".into(),
            production: false,
            media: None,
            limits: board_config::PublicRequestLimits::default(),
            proxy_uid: None,
            poster_id_key: Some(posting::key(slug)),
            tripcode_key: None,
            country_database: None,
        },
    );
    let path = format!("/{slug}/thread/{op}.json");
    let old = response(&web, &path).await;
    assert_eq!(old.status(), StatusCode::OK);
    let etag = old.headers()["etag"].to_str().unwrap().to_owned();
    assert!(
        json(&web, &path).await["posts"][0]
            .get("archived")
            .is_none()
    );
    let mut request = Request::post(format!("/{slug}/post"))
        .header("origin", "http://127.0.0.1:3000")
        .header("accept", "application/json")
        .header("content-type", "application/x-www-form-urlencoded")
        .body(Body::from(format!(
            "resto={op}&com=Final+anonymous+reply&pwd=&email=sage"
        )))
        .unwrap();
    request.extensions_mut().insert(axum::extract::ConnectInfo(
        "192.0.2.201:12345".parse::<std::net::SocketAddr>().unwrap(),
    ));
    let accepted = web.clone().oneshot(request).await.unwrap();
    assert_eq!(accepted.status(), StatusCode::OK);
    assert!(
        accepted
            .headers()
            .get_all("set-cookie")
            .iter()
            .any(|value| value.to_str().unwrap().starts_with("board-anon="))
    );
    let value: Value =
        serde_json::from_slice(&to_bytes(accepted.into_body(), 65536).await.unwrap()).unwrap();
    let last = value["pid"].as_i64().unwrap();
    for app in [&web, &api] {
        let thread = json(app, &path).await;
        let posts = thread["posts"].as_array().unwrap();
        assert_eq!(posts.len(), 1501);
        assert_eq!(posts.last().unwrap()["no"], last);
        assert_eq!(posts[0]["replies"], 1500);
        assert_eq!(posts[0]["archived"], 1);
        assert_eq!(posts[0]["closed"], 1);
        assert!(posts[0]["archived_on"].as_i64().unwrap() > 0);
        let tail = json(app, &format!("/{slug}/thread/{op}-tail.json")).await;
        assert_eq!(tail["posts"].as_array().unwrap().len(), 51);
        assert_eq!(tail["posts"][0]["replies"], 1500);
        assert_eq!(
            json(app, &format!("/{slug}/archive.json")).await,
            serde_json::json!([op])
        );
        let changed = app
            .clone()
            .oneshot(
                Request::get(&path)
                    .header("if-none-match", &etag)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(changed.status(), StatusCode::OK);
        assert_ne!(changed.headers()["etag"].to_str().unwrap(), etag);
    }
    let native = json(&web, &format!("/_watch/{slug}/thread/{op}/posts")).await;
    assert_eq!(native["posts"].as_array().unwrap().len(), 1501);
    assert_eq!(native["archived"], true);
    assert_eq!(native["closed"], false); // Native transport keeps the staff flag separate.
    assert_eq!(native["replies"], 1500);
    let html = response(&web, &format!("/{slug}/thread/{op}")).await;
    assert_eq!(html.status(), StatusCode::OK);
    let text = String::from_utf8(
        to_bytes(html.into_body(), 8 * 1024 * 1024)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(text.contains(&format!("id=\"pc{last}\"")));
    assert!(!text.contains("id=\"postForm\""));
}

#[tokio::test]
async fn threshold_is_visible_on_html_json_tail_and_native_routes() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let slug: String =
        sqlx::query_scalar("SELECT 'ac'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(captcha_enabled,slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,json_tail_size,archive_retention_seconds,autoarchive_cap,posting_reply_seconds,posting_image_seconds,posting_thread_seconds,text_only) VALUES(false,$1,'Owned autoarchive API','Synthetic fixture',2000,1000,1000,5,5,50,3600,1500,0,0,0,true)").bind(&slug).execute(&owner).await.unwrap();
    let (o, p, s) = (owner.clone(), public.clone(), slug.clone());
    let result = tokio::spawn(async move { exercise(&o, &p, &s).await }).await;
    let tokens: Vec<Vec<u8>> = sqlx::query_scalar("SELECT a.token_hash FROM post_secrets.anonymous_posts a JOIN content.posts p ON p.id=a.post_id WHERE p.board=$1")
        .bind(&slug).fetch_all(&owner).await.unwrap();
    posting::cleanup_posting(&owner, &slug).await;
    for statement in [
        "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
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
    for token in tokens {
        sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
            .bind(token)
            .execute(&owner)
            .await
            .unwrap();
    }
    public.close().await;
    owner.close().await;
    result.unwrap();
}
