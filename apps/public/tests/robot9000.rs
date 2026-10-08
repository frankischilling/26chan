#![cfg(feature = "database-tests")]

use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use std::net::SocketAddr;
use tower::ServiceExt;

fn fixture_key() -> std::sync::Arc<board_domain::poster_id::PosterIdKey> {
    use rand_core::RngCore;
    let mut bytes = [0u8; 32];
    rand_core::OsRng.fill_bytes(&mut bytes);
    let encoded: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    std::sync::Arc::new(board_domain::poster_id::PosterIdKey::parse(&encoded).unwrap())
}

async fn submit(
    app: &Router,
    board: &str,
    parent: i64,
    comment: &str,
    peer: u8,
    accept: &str,
) -> axum::response::Response {
    let fields = url::form_urlencoded::Serializer::new(String::new())
        .append_pair("resto", &parent.to_string())
        .append_pair("sub", "Owned Robot9000")
        .append_pair("com", comment)
        .append_pair("pwd", "owned-robot-password")
        .append_pair("email", "bypass_r9k")
        .finish();
    let request = Request::post(format!("/{board}/imgboard.php"))
        .header("origin", "http://127.0.0.1:3000")
        .header("accept", accept)
        .header("content-type", "application/x-www-form-urlencoded")
        .header("x-forwarded-for", "198.51.100.100")
        .header("x-board-client-ip", "198.51.100.101")
        .extension(axum::extract::ConnectInfo(SocketAddr::from((
            [192, 0, 2, peer],
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
async fn real_posting_formats_preserve_robot9000_errors_and_transport_identity() {
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
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,robot9000,posting_reply_seconds,posting_image_seconds,posting_thread_seconds) VALUES($1,'Owned HTTP Robot9000','Synthetic',1000,100,100,100,10,true,0,0,0)")
        .bind(&board).execute(&owner).await.unwrap();
    let (a, p, b) = (owner.clone(), public.clone(), board.clone());
    let outcome=tokio::spawn(async move {
        let key=fixture_key();
        let (app,api)=board_public::routers_with_options(p.clone(),board_public::PublicRouterOptions {
            origin:"http://127.0.0.1:3000".into(),production:false,media:None,
            limits:board_config::PublicRequestLimits::default(),proxy_uid:None,
            tripcode_key:None,poster_id_key:Some(key.clone()),country_database:None,
        });
        let first=json(submit(&app,&b,0,"HTTP original text",1,"application/json").await,StatusCode::OK).await;
        let thread=first["pid"].as_i64().unwrap();
        let duplicate=json(submit(&app,&b,thread,"HTTP original text",2,"application/json").await,StatusCode::OK).await;
        assert_eq!(duplicate,serde_json::json!({"error":"You have been muted for 2 seconds, because your comment was not original."}));
        // Both spoofed headers stay identical; actual socket actors remain separate.
        assert!(json(submit(&app,&b,thread,"unmuted separate socket actor",3,"application/json").await,StatusCode::OK).await["pid"].is_number());
        let actor=key.robot9000_fingerprint(&b,"192.0.2.2".parse().unwrap()).unwrap();
        sqlx::query("UPDATE post_secrets.robot9000_mutes SET mute_until=date_trunc('second',clock_timestamp())+interval '60 seconds',next_expire=date_trunc('second',clock_timestamp())+interval '60 seconds' WHERE board=$1 AND actor=$2")
            .bind(&b).bind(actor.as_slice()).execute(&a).await.unwrap();
        let html=submit(&app,&b,thread,"different text cannot bypass a live mute",2,"text/html").await;
        assert_eq!(html.status(),StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(html.headers()["cache-control"],"no-store");
        assert!(!html.headers().contains_key("set-cookie"));
        let html=String::from_utf8(to_bytes(html.into_body(),8192).await.unwrap().to_vec()).unwrap();
        assert!(html.contains("You&#39;re muted!") || html.contains("You&#x27;re muted!"));
        assert!(html.contains("You cannot post until ") && html.contains(" from now"));
        assert_eq!(json(submit(&app,&b,thread,"café",2,"application/json").await,StatusCode::OK).await["error"],"Non-ASCII text is not allowed.");
        let rows:i64=sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1").bind(&b).fetch_one(&a).await.unwrap();
        assert_eq!(rows,2);
        let response=api.oneshot(Request::get(format!("/{b}/thread/{thread}.json")).body(Body::empty()).unwrap()).await.unwrap();
        let bytes=to_bytes(response.into_body(),65536).await.unwrap();
        let data:serde_json::Value=serde_json::from_slice(&bytes).unwrap();
        assert_eq!(data["posts"].as_array().unwrap().len(),2);
        let text=std::str::from_utf8(&bytes).unwrap();
        assert!(!text.contains("192.0.2."));
        for post in data["posts"].as_array().unwrap() {
            for key in ["actor","digest","mute_until","timeout_power","next_expire"] {
                assert!(post.get(key).is_none(),"Private state field: {key}");
            }
        }
        sqlx::query("UPDATE content.boards SET robot9000_state_limit=1 WHERE slug=$1").bind(&b).execute(&a).await.unwrap();
        let unavailable=json(submit(&app,&b,thread,"capacity must not register this post",4,"application/json").await,StatusCode::SERVICE_UNAVAILABLE).await;
        assert_eq!(unavailable["error"],"Storage is unavailable. Try again later.");
    }).await;
    for query in [
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
