#![cfg(feature = "database-tests")]

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use std::net::SocketAddr;
use tower::ServiceExt;

async fn submit(
    app: &axum::Router,
    board: &str,
    parent: i64,
    comment: &str,
    accept: &str,
) -> axum::response::Response {
    let fields = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("resto", &parent.to_string())
        .append_pair("name", "Alice#password")
        .append_pair("com", comment)
        .append_pair("email", "sage")
        .finish();
    let request = Request::post(format!("/{board}/post"))
        .header("origin", "http://127.0.0.1:3000")
        .header("accept", accept)
        .header("content-type", "application/x-www-form-urlencoded")
        // Spoofed network headers cannot select the rule's private actor.
        .header("x-forwarded-for", "198.51.100.100")
        .extension(axum::extract::ConnectInfo(SocketAddr::from((
            [192, 0, 2, 71],
            45000,
        ))))
        .body(Body::from(fields))
        .unwrap();
    app.clone().oneshot(request).await.unwrap()
}

async fn json(response: axum::response::Response, status: StatusCode) -> serde_json::Value {
    assert_eq!(response.status(), status);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert!(
        response.headers()["vary"]
            .to_str()
            .unwrap()
            .contains("Accept")
    );
    serde_json::from_slice(&to_bytes(response.into_body(), 8192).await.unwrap()).unwrap()
}

#[tokio::test]
async fn real_forms_preserve_rule_errors_quiet_success_and_fail_closed_storage() {
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
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Owned HTTP admission','Synthetic',1000,100,100,100,10)")
        .bind(&board).execute(&owner).await.unwrap();
    let (a, p, b) = (owner.clone(), public.clone(), board.clone());
    let outcome = tokio::spawn(async move {
        let (app,_) = board_public::routers_with_options(p,board_public::PublicRouterOptions {
            origin:"http://127.0.0.1:3000".into(),production:false,media:None,
            limits:board_config::PublicRequestLimits::default(),proxy_uid:None,
            tripcode_key:None,poster_id_key:None,country_database:None,
        });
        let first = json(submit(&app,&b,0,"ordinary thread","application/json").await,StatusCode::OK).await;
        let thread = first["pid"].as_i64().unwrap();
        json(submit(&app,&b,thread,"ordinary reply","application/json").await,StatusCode::OK).await;
        let rule:i64 = sqlx::query_scalar("INSERT INTO admission.rules(board,pattern) VALUES($1,'paper') RETURNING id")
            .bind(&b).fetch_one(&a).await.unwrap();
        let rejected = submit(&app,&b,thread,"paper","application/json").await;
        assert!(!rejected.headers().contains_key("set-cookie"));
        assert_eq!(json(rejected,StatusCode::OK).await,serde_json::json!({"error":"Error: Our system thinks your post is spam. Please reformat and try again."}));
        let html = submit(&app,&b,thread,"paper","text/html").await;
        assert_eq!(html.status(),StatusCode::UNPROCESSABLE_ENTITY);
        let html = String::from_utf8(to_bytes(html.into_body(),8192).await.unwrap().to_vec()).unwrap();
        assert!(html.contains("Please reformat and try again."));
        assert!(!html.contains("filter ID") && !html.contains("192.0.2.71"));
        sqlx::query("UPDATE admission.rules SET quiet=true WHERE id=$1").bind(rule).execute(&a).await.unwrap();
        let highest:i64 = sqlx::query_scalar("SELECT max(id) FROM content.posts WHERE board=$1")
            .bind(&b).fetch_one(&a).await.unwrap();
        let quiet = submit(&app,&b,thread,"paper","application/json").await;
        let cookies:Vec<_> = quiet.headers().get_all("set-cookie").iter().map(|cookie|cookie.to_str().unwrap().to_owned()).collect();
        assert!(cookies.iter().any(|cookie|cookie.starts_with("4chan_name=Alice;")));
        assert!(cookies.iter().any(|cookie|cookie.starts_with("board-anon=")));
        assert!(cookies.iter().all(|cookie|!cookie.contains("password")));
        assert_eq!(json(quiet,StatusCode::OK).await,serde_json::json!({"tid":thread,"pid":highest+1}));
        let quiet = submit(&app,&b,0,"paper","text/html").await;
        assert_eq!(quiet.status(),StatusCode::SEE_OTHER);
        assert_eq!(quiet.headers()["location"],format!("/{b}/thread/{thread}#p{thread}"));
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM content.posts WHERE board=$1")
            .bind(&b).fetch_one(&a).await.unwrap(),2);
        let peer:String = sqlx::query_scalar("SELECT host(peer) FROM admission.hits WHERE rule_id=$1")
            .bind(rule).fetch_one(&a).await.unwrap();
        assert_eq!(peer,"192.0.2.71");
        sqlx::query("UPDATE admission.rules SET quiet=false,regex=true,pattern='/incomplete' WHERE id=$1")
            .bind(rule).execute(&a).await.unwrap();
        assert_eq!(json(submit(&app,&b,0,"ordinary after broken policy","application/json").await,StatusCode::SERVICE_UNAVAILABLE).await,
            serde_json::json!({"error":"Storage is unavailable. Try again later."}));
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM content.posts WHERE board=$1")
            .bind(&b).fetch_one(&a).await.unwrap(),2);
    }).await;
    for query in [
        "DELETE FROM admission.rules WHERE board=$1",
        "DELETE FROM admission.hits WHERE board=$1",
        "DELETE FROM admission.logs WHERE board=$1",
        "DELETE FROM admission.bans WHERE board=$1",
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
    outcome.unwrap();
}
