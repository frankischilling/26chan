#![cfg(feature = "database-tests")]
#[path = "support/posting.rs"]
mod posting;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use board_store::site_content;
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
fn input(value: serde_json::Value) -> site_content::Input {
    site_content::read_input(serde_json::to_vec(&value).unwrap().as_slice()).unwrap()
}
#[tokio::test]
async fn local_information_news_updates_pagination_and_private_rules_use_real_public_routes() {
    let owner_url = std::env::var("MIGRATION_DATABASE_URL").unwrap();
    let owner = PgPool::connect(&owner_url).await.unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let empty:bool=sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM site_private.pages) AND NOT EXISTS(SELECT 1 FROM site_private.news)").fetch_one(&owner).await.unwrap();
    assert!(empty, "requires fresh disposable site-content tables");
    let slug = format!(
        "si{:06x}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .subsec_nanos()
            & 0xffffff
    );
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Owned site routes','',1000,100,100,100,10)").bind(&slug).execute(&owner).await.unwrap();
    let result=tokio::spawn({let owner=owner.clone();let public=public.clone();let slug=slug.clone();async move {
        let (app,api)=posting::routers(public.clone(),&slug,ORIGIN.into(),false);
        for path in ["/about","/faq","/rules","/contact","/feedback","/legal","/press","/advertise","/support","/japanese","/blog","/news"] {
            let (status,csp,_)=request(&app,Method::GET,path).await;
            assert_eq!(status,StatusCode::OK,"{path}");assert!(csp.contains("script-src 'none'"));
            assert_eq!(request(&app,Method::HEAD,path).await.2,"");
            assert_eq!(request(&app,Method::POST,path).await.0,StatusCode::METHOD_NOT_ALLOWED);
        }
        assert_eq!(request(&app,Method::GET,"/news/").await.0,StatusCode::OK);
        assert!(!request(&app,Method::GET,"/news/").await.2.contains("class=\"siteHome\""),"the original news board keeps its trailing-slash route");
        assert_ne!(request(&api,Method::GET,"/faq").await.0,StatusCode::OK);
        assert!(request(&app,Method::GET,"/news").await.2.contains("No news has been published."));
        let mut connection=PgConnection::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap()).await.unwrap();
        let page=|key:&str,revision|input(serde_json::json!({"kind":"page","version":1,"key":key,"expected_revision":revision,"title":"Owned route <title>","body":"owned-site-route <script>plain text</script>"}));
        site_content::publish(&mut connection,&page("faq",0)).await.unwrap();
        let html=request(&app,Method::GET,"/faq").await.2;
        assert!(html.contains("Owned route &#60;title&#62;"));assert!(!html.contains("<script>plain"));
        assert!(html.contains("id=\"what4chan\""));
        site_content::publish(&mut connection,&page("announcement",0)).await.unwrap();
        for path in [format!("/{slug}/"),format!("/{slug}/catalog")] {
            let (status,_,html)=request(&app,Method::GET,&path).await;
            assert_eq!(status,StatusCode::OK);
            assert!(html.contains("id=\"globalMessage\""));
            assert!(html.contains("id=\"globalToggle\""));
            assert!(html.contains("owned-site-route &#60;script&#62;plain text&#60;/script&#62;"));
            assert!(!html.contains("<script>plain text</script>"));
        }
        let id:i64=sqlx::query_scalar("INSERT INTO content.threads(board) VALUES($1) RETURNING id").bind(&slug).fetch_one(&owner).await.unwrap();
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','Owned announcement route','Owned announcement fixture')").bind(id).bind(&slug).execute(&owner).await.unwrap();
        let thread_path=format!("/{slug}/thread/{id}");
        assert!(request(&app,Method::GET,&thread_path).await.2.contains("id=\"globalMessage\""));
        sqlx::query("UPDATE content.threads SET closed=true WHERE board=$1 AND id=$2").bind(&slug).bind(id).execute(&owner).await.unwrap();
        assert!(!request(&app,Method::GET,&thread_path).await.2.contains("id=\"globalMessage\""));
        sqlx::query("UPDATE content.boards SET upload_board=true WHERE slug=$1").bind(&slug).execute(&owner).await.unwrap();
        assert!(request(&app,Method::GET,&thread_path).await.2.contains("id=\"globalMessage\""));
        sqlx::query("UPDATE content.boards SET upload_board=false WHERE slug=$1").bind(&slug).execute(&owner).await.unwrap();
        sqlx::query("UPDATE content.threads SET closed=false,archived_at=now(),archive_expires_at=now()+interval '1 hour' WHERE board=$1 AND id=$2").bind(&slug).bind(id).execute(&owner).await.unwrap();
        assert!(!request(&app,Method::GET,&thread_path).await.2.contains("id=\"globalMessage\""));
        site_content::retract_page(&mut connection,"announcement",1).await.unwrap();
        assert!(!request(&app,Method::GET,&format!("/{slug}/")).await.2.contains("id=\"globalMessage\""));
        let key=format!("rules/{slug}");site_content::publish(&mut connection,&page(&key,0)).await.unwrap();
        assert!(request(&app,Method::GET,"/rules").await.2.contains(&format!("id=\"{slug}\"")));
        sqlx::query("UPDATE content.boards SET staff_only=true WHERE slug=$1").bind(&slug).execute(&owner).await.unwrap();
        assert!(!request(&app,Method::GET,"/rules").await.2.contains(&format!("id=\"{slug}\"")));
        let date=chrono::Utc::now().timestamp()-100;
        for index in 0..27 {
            let news=input(serde_json::json!({"kind":"news","version":1,"published_at":date+index,"title":format!("Owned route news {index}"),"author":"Owned editor","body":"owned-site-route <script>news</script>"}));
            assert_eq!(site_content::publish(&mut connection,&news).await.unwrap(),index+1);
        }
        let html=request(&app,Method::GET,"/news").await.2;
        assert!(html.contains("/news?before=3"));assert!(!html.contains("<script>news"));
        assert!(html.contains("Owned editor"));assert!(!html.contains("Owned route news 0</span>"));
        let html=request(&app,Method::GET,"/news?before=3").await.2;
        assert!(html.contains("Owned route news 0"));assert!(!html.contains("Owned route news 26"));
        let html=request(&app,Method::GET,"/").await.2;
        assert!(html.contains("Owned route news 26"));assert!(!html.contains("Owned route news 23"));
        assert!(!html.contains(&format!("href=\"/{slug}/\"")));
        assert_eq!(request(&app,Method::GET,"/news?item=27").await.0,StatusCode::OK);
        site_content::retract_news(&mut connection,27).await.unwrap();
        assert_eq!(request(&app,Method::GET,"/news?item=27").await.0,StatusCode::NOT_FOUND);
        assert!(!request(&app,Method::GET,"/").await.2.contains("Owned route news 26"));
        site_content::retract_page(&mut connection,"faq",1).await.unwrap();
        assert!(request(&app,Method::GET,"/faq").await.2.contains("How do I post?"));
        for path in ["/news?before=0","/news?item=01","/news?item=10001","/news?item=1&item=2"] {
            assert_eq!(request(&app,Method::GET,path).await.0,StatusCode::BAD_REQUEST,"{path}");
        }
    }}).await;
    sqlx::query("DELETE FROM site_private.pages WHERE (key IN ('faq','announcement') OR board=$1) AND body='owned-site-route <script>plain text</script>'").bind(&slug).execute(&owner).await.unwrap();
    sqlx::query("DELETE FROM site_private.news WHERE id BETWEEN 1 AND 27 AND body='owned-site-route <script>news</script>'").execute(&owner).await.unwrap();
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
    sqlx::query("DELETE FROM content.boards WHERE slug=$1 AND title='Owned site routes'")
        .bind(&slug)
        .execute(&owner)
        .await
        .unwrap();
    result.unwrap();
}
