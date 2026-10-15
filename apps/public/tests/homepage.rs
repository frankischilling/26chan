#![cfg(feature = "database-tests")]
#[path = "support/posting.rs"]
mod posting;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use board_store::homepage;
use sqlx::{Connection, PgConnection, PgPool};
use tower::ServiceExt;
const ORIGIN: &str = "http://127.0.0.1:3000";
async fn request(app: &Router, method: Method, path: &str) -> (StatusCode, String, String) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("origin", ORIGIN)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let csp = response
        .headers()
        .get("content-security-policy")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_owned();
    assert!(response.headers().get("set-cookie").is_none());
    let body = String::from_utf8(
        to_bytes(response.into_body(), 2_000_000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    (status, csp, body)
}

#[tokio::test]
async fn homepage_routes_render_live_safe_cards_and_stale_statistics() {
    let owner_url = std::env::var("MIGRATION_DATABASE_URL").unwrap();
    let owner = PgPool::connect(&owner_url).await.unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    assert!(
        sqlx::query_scalar::<_, bool>(
            "SELECT NOT EXISTS(SELECT 1 FROM site_private.frontpage_state)"
        )
        .fetch_one(&owner)
        .await
        .unwrap(),
        "requires fresh disposable homepage cache"
    );
    let slug = format!(
        "hh{:06x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
            & 0xffffff
    );
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Owned homepage routes','',1000,100,100,100,10)").bind(&slug).execute(&owner).await.unwrap();
    let result=tokio::spawn({let owner=owner.clone();let public=public.clone();let slug=slug.clone(); async move {
        let (app,api)=posting::routers(public.clone(),&slug,ORIGIN.into(),false);
        let (status,csp,empty)=request(&app,Method::GET,"/").await;
        assert_eq!(status,StatusCode::OK);assert!(csp.contains("script-src 'none'"));
        assert!(empty.contains("Statistics are not available yet."));
        assert_eq!(request(&app,Method::GET,"/stats").await.0,StatusCode::OK);
        assert!(request(&app,Method::GET,"/stats").await.2.contains("No board samples are available yet."));
        assert!(empty.contains("No eligible posts are available."));
        assert_eq!(request(&app,Method::HEAD,"/").await.2,"");
        assert_eq!(request(&app,Method::POST,"/").await.0,StatusCode::METHOD_NOT_ALLOWED);
        assert_ne!(request(&api,Method::GET,"/").await.0,StatusCode::OK);
        let id:i64=sqlx::query_scalar("INSERT INTO content.threads(board,bumped_at) VALUES($1,clock_timestamp()) RETURNING id").bind(&slug).fetch_one(&owner).await.unwrap();
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at) VALUES($1,$2,$1,'Anonymous','Owned promoted <title>','owned-home-route <script>literal</script>',clock_timestamp()-interval '12 minutes')").bind(id).bind(&slug).execute(&owner).await.unwrap();
        sqlx::query("INSERT INTO content.posts(board,thread_id,name,subject,comment,created_at) SELECT $1,$2,'Anonymous','','Owned route reply',clock_timestamp()-interval '11 minutes' FROM generate_series(1,15)").bind(&slug).bind(id).execute(&owner).await.unwrap();
        let mut writer=PgConnection::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap()).await.unwrap();
        homepage::refresh(&mut writer).await.unwrap();
        let html=request(&app,Method::GET,"/").await.2;
        assert!(html.contains("Owned promoted &#60;title&#62;"));
        assert!(!html.contains("<script>literal</script>"));
        assert!(html.contains(&format!("/{slug}/thread/{id}#p{id}")));
        assert!(request(&app,Method::GET,"/stats").await.2.contains(&format!("/stats?board={slug}")));
        assert_eq!(request(&app,Method::GET,&format!("/stats?board={slug}")).await.0,StatusCode::OK);
        for path in ["/stats?board=", "/stats?board=g&board=b", "/stats?other=g"] { assert_eq!(request(&app,Method::GET,path).await.0,StatusCode::BAD_REQUEST); }
        assert!(html.contains("15 replies"));assert!(html.contains("Recent posting sessions"));
        assert!(!html.contains("more than 15 minutes old"));
        sqlx::query("UPDATE site_private.frontpage_state SET generated_at=clock_timestamp()-interval '16 minutes'").execute(&owner).await.unwrap();
        assert!(request(&app,Method::GET,"/").await.2.contains("more than 15 minutes old"));
        sqlx::query("UPDATE content.boards SET staff_only=true WHERE slug=$1").bind(&slug).execute(&owner).await.unwrap();
        let html=request(&app,Method::GET,"/").await.2;
        assert!(!html.contains("Owned promoted"));
        assert_eq!(request(&app,Method::GET,&format!("/stats?board={slug}")).await.0,StatusCode::NOT_FOUND);assert!(!html.contains(&format!("href=\"/{slug}/\"")));
        assert!(html.contains("Statistics are not available yet."));
        sqlx::query("UPDATE content.boards SET staff_only=false WHERE slug=$1").bind(&slug).execute(&owner).await.unwrap();
        sqlx::query("UPDATE content.posts SET deleted=true WHERE id=$1").bind(id).execute(&owner).await.unwrap();
        assert!(!request(&app,Method::GET,"/").await.2.contains("Owned promoted"));
    }}).await;
    sqlx::raw_sql("DELETE FROM site_private.frontpage_cards; DELETE FROM site_private.frontpage_state; DELETE FROM site_private.board_statistics;").execute(&owner).await.unwrap();
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
    sqlx::query("DELETE FROM content.boards WHERE slug=$1 AND title='Owned homepage routes'")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    result.unwrap();
}
