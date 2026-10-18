#![cfg(feature = "database-tests")]

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use board_staff::{AppState, Config, Limits, auth, router};
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use tower::ServiceExt;
use webauthn_rs::prelude::*;

#[tokio::test]
async fn janitor_request_http_preserves_creation_review_and_statistics() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let auth_pool = PgPool::connect(&std::env::var("AUTH_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let staff_pool = PgPool::connect(&std::env::var("STAFF_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let origin = Url::parse("http://localhost:3001").unwrap();
    let state = Arc::new(AppState {
        config: Config {
            asn_database: None,
            browser_hint_key: None,
            media: None,
            proxy: None,
            poster_id_key: None,
            country_database: None,
            origin: "http://localhost:3001".into(),
            public_origin: "http://localhost:3000".into(),
            media_origin: "http://127.0.0.1:3002".into(),
            bind: "127.0.0.1:3001".parse().unwrap(),
            production: false,
            auth_database: String::new(),
            staff_database: String::new(),
            idle_timeout: Duration::from_secs(600),
            tripcode_key: None,
        },
        auth: auth_pool.clone(),
        staff: staff_pool.clone(),
        webauthn: WebauthnBuilder::new("localhost", &origin)
            .unwrap()
            .build()
            .unwrap(),
        limits: Limits::default(),
    });
    auth::check_identity(&auth_pool, "board_auth")
        .await
        .unwrap();
    auth::check_identity(&staff_pool, "board_staff")
        .await
        .unwrap();
    let token = auth::token();
    let csrf = auth::token();
    let credential = uuid::Uuid::new_v4().as_bytes().to_vec();
    let actor = uuid::Uuid::new_v4().simple().to_string().repeat(2);
    let mut tx = owner.begin().await.unwrap();
    let op: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
        .fetch_one(&mut *tx)
        .await
        .unwrap();
    let board = format!("jq{op:x}");
    let posts: Vec<i64> =
        sqlx::query_scalar("SELECT nextval('content.post_number') FROM generate_series(1,7)")
            .fetch_all(&mut *tx)
            .await
            .unwrap();
    let template = 10_000_000 + op;
    sqlx::query("SELECT set_config('board.posting_actor',$1,true)")
        .bind(&actor)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Owned HTTP request','Synthetic',2000,100,100,100,10)").bind(&board).execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO content.threads(id,board) VALUES($1,$2)")
        .bind(op)
        .bind(&board)
        .execute(&mut *tx)
        .await
        .unwrap();
    for post in std::iter::once(&op).chain(posts.iter()) {
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$3,'Owned','<script>Owned subject</script>','Owned HTTP request body')").bind(post).bind(&board).bind(op).execute(&mut *tx).await.unwrap();
    }
    let account:i64=sqlx::query_scalar("INSERT INTO staff_identity.accounts(role,allow_boards) VALUES('janitor',ARRAY[$1]) RETURNING id").bind(&board).fetch_one(&mut *tx).await.unwrap();
    sqlx::query(
        "INSERT INTO staff_identity.credentials(id,account_id,credential) VALUES($1,$2,'{}')",
    )
    .bind(&credential)
    .bind(account)
    .execute(&mut *tx)
    .await
    .unwrap();
    sqlx::query("INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id) VALUES($1,$2,$3,$4)").bind(auth::hash(&token)).bind(auth::hash(&csrf)).bind(account).bind(&credential).execute(&mut *tx).await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *tx)
        .await
        .unwrap();
    for (id, warning, seconds) in [(template, false, 86400_i64), (template + 1, true, 0)] {
        sqlx::query("INSERT INTO restrictions.templates(id,name,minimum_role,global,warning,seconds,public_reason,private_reason,request_save_policy,request_blacklist_policy) VALUES($1,'Owned request template','janitor',false,$2,$3,'Owned reason','','none','none')").bind(id).bind(warning).bind(seconds).execute(&mut *tx).await.unwrap();
    }
    tx.commit().await.unwrap();
    let outcome=tokio::spawn({
        let owner=owner.clone(); let board=board.clone(); let posts=posts.clone();
        async move {
            let app=router(state);
            let cookie=format!("staff={token}; staff-csrf={csrf}");
            let path=format!("/ban-request?board={board}&target={}",posts[0]);
            let get=|path:&str,cookies:&str| Request::get(path).header("cookie",cookies).body(Body::empty()).unwrap();
            let post=|body:String,origin:&str| Request::post("/ban-request").header("cookie",&cookie).header("origin",origin).header("sec-fetch-site","same-origin").header("content-type","application/x-www-form-urlencoded").body(Body::from(body)).unwrap();
            assert_eq!(app.clone().oneshot(get(&path,"")).await.unwrap().status(),StatusCode::UNAUTHORIZED);
            let response=app.clone().oneshot(get(&path,&cookie)).await.unwrap();
            assert_eq!(response.status(),StatusCode::OK);
            assert_eq!(response.headers()["cache-control"],"private, no-store");
            let html=String::from_utf8(to_bytes(response.into_body(),1_048_576).await.unwrap().to_vec()).unwrap();
            assert!(html.contains("Owned request template"));
            assert!(!html.contains("<script>Owned subject</script>"));
            assert!(!html.contains(&token));
            assert!(!html.contains(&actor));
            let normal=format!("board={board}&target={}&template={template}&csrf={csrf}",posts[0]);
            assert_eq!(app.clone().oneshot(post(normal.clone(),"https://other.invalid")).await.unwrap().status(),StatusCode::FORBIDDEN);
            assert_eq!(app.clone().oneshot(post(format!("{normal}&unlock=1"),"http://localhost:3001")).await.unwrap().status(),StatusCode::UNPROCESSABLE_ENTITY);
            assert_eq!(app.clone().oneshot(post(format!("{normal}&warn_req=1"),"http://localhost:3001")).await.unwrap().status(),StatusCode::FORBIDDEN);
            assert_eq!(app.clone().oneshot(post(normal.replace(&csrf,"bad"),"http://localhost:3001")).await.unwrap().status(),StatusCode::FORBIDDEN);
            let asynchronous=format!("board={board}&target={}&by_tpl={template}&warn_req=1&csrf={csrf}",posts[0]);
            let response=app.clone().oneshot(post(asynchronous,"http://localhost:3001")).await.unwrap();
            assert_eq!(response.status(),StatusCode::OK);
            assert!(String::from_utf8(to_bytes(response.into_body(),65536).await.unwrap().to_vec()).unwrap().contains("waiting for moderator review"));
            assert_eq!(app.clone().oneshot(post(normal,"http://localhost:3001")).await.unwrap().status(),StatusCode::NOT_FOUND);
            let warning=format!("board={board}&target={}&template={}&csrf={csrf}",posts[1],template+1);
            assert_eq!(app.clone().oneshot(post(warning,"http://localhost:3001")).await.unwrap().status(),StatusCode::OK);
            let response=app.clone().oneshot(get(&format!("/ban-request?board={board}&target={}",posts[2]),&cookie)).await.unwrap();
            assert_eq!(response.status(),StatusCode::OK);
            assert!(String::from_utf8(to_bytes(response.into_body(),1_048_576).await.unwrap().to_vec()).unwrap().contains("2 pending requests"));
            let mut inspect=owner.begin().await.unwrap();
            sqlx::query("SET LOCAL ROLE board_restriction_owner").execute(&mut *inspect).await.unwrap();
            let flags:Vec<bool>=sqlx::query_scalar("SELECT warning FROM restrictions.requests WHERE account_id=$1 ORDER BY post_id").bind(account).fetch_all(&mut *inspect).await.unwrap();
            assert_eq!(flags,vec![false,true]);
            let request_ids:Vec<i64>=sqlx::query_scalar("SELECT id FROM restrictions.requests WHERE account_id=$1 ORDER BY post_id").bind(account).fetch_all(&mut *inspect).await.unwrap();
            inspect.commit().await.unwrap();
            assert_eq!(app.clone().oneshot(get("/ban-requests",&cookie)).await.unwrap().status(),StatusCode::FORBIDDEN);
            let review=|id:i64,decision:&str| Request::post("/ban-requests/review").header("cookie",&cookie)
                .header("origin","http://localhost:3001").header("sec-fetch-site","same-origin")
                .header("content-type","application/x-www-form-urlencoded").header("accept","application/json")
                .body(Body::from(format!("id={id}&decision={decision}&csrf={csrf}"))).unwrap();
            assert_eq!(app.clone().oneshot(review(request_ids[0],"accept")).await.unwrap().status(),StatusCode::FORBIDDEN);
            sqlx::query("UPDATE staff_identity.accounts SET role='moderator' WHERE id=$1").bind(account).execute(&owner).await.unwrap();
            let response=app.clone().oneshot(get("/ban-requests",&cookie)).await.unwrap();
            assert_eq!(response.status(),StatusCode::OK);
            let html=String::from_utf8(to_bytes(response.into_body(),4*1024*1024).await.unwrap().to_vec()).unwrap();
            assert!(html.contains("Owned HTTP request body"));
            assert!(html.contains("name=\"referrer\" content=\"same-origin\""));
            assert!(html.contains(&format!("No.{}",posts[0])));
            let warning_marker=format!("id=\"ban-request-{}\"",request_ids[1]);
            let warning_row=html.split(&warning_marker).nth(1).unwrap().split("</tr>").next().unwrap();
            assert!(warning_row.contains("name=\"ban_length\" type=\"number\" min=\"-1\" max=\"1024\" value=\"0\""));
            assert!(!html.contains("<script>Owned subject</script>"));
            assert!(!html.contains(&actor));
            assert_eq!(app.clone().oneshot(get("/ban-requests?board=outside",&cookie)).await.unwrap().status(),StatusCode::FORBIDDEN);
            for origin in [None, Some("null"), Some("https://outside.invalid")] {
                let mut denied=review(request_ids[0],"accept");
                denied.headers_mut().remove("origin");
                if let Some(origin)=origin { denied.headers_mut().insert("origin",origin.parse().unwrap()); }
                assert_eq!(app.clone().oneshot(denied).await.unwrap().status(),StatusCode::FORBIDDEN);
            }
            let response=app.clone().oneshot(review(request_ids[0],"accept")).await.unwrap();
            assert_eq!(response.status(),StatusCode::OK);
            let accepted:serde_json::Value=serde_json::from_slice(&to_bytes(response.into_body(),4096).await.unwrap()).unwrap();
            assert_eq!(accepted["changed"],true);
            assert_eq!(accepted["cleared"],1);
            assert!(accepted["ban_id"].as_i64().is_some_and(|id|id>0));
            assert_eq!(app.clone().oneshot(review(request_ids[0],"accept")).await.unwrap().status(),StatusCode::NOT_FOUND);
            for changed in [true,false] {
                let response=app.clone().oneshot(review(request_ids[1],"deny")).await.unwrap();
                assert_eq!(response.status(),StatusCode::OK);
                let denied:serde_json::Value=serde_json::from_slice(&to_bytes(response.into_body(),4096).await.unwrap()).unwrap();
                assert_eq!(denied["changed"],changed);
                assert_eq!(denied["cleared"],u32::from(changed));
                assert!(denied["ban_id"].is_null());
            }
            let response=app.clone().oneshot(get("/ban-requests/statistics",&cookie)).await.unwrap();
            assert_eq!(response.status(),StatusCode::OK);
            let html=String::from_utf8(to_bytes(response.into_body(),1_048_576).await.unwrap().to_vec()).unwrap();
            assert!(html.contains("My requests: 1 approved, 1 denied, 0 pending."));
            assert!(html.contains("Reviews I performed in retained history: 1 approved, 1 denied."));
            assert_eq!(app.clone().oneshot(get("/ban-requests/statistics?account_id=1",&cookie)).await.unwrap().status(),StatusCode::BAD_REQUEST);
            if std::env::var("RUN_JANITOR_BROWSER").as_deref()==Ok("1") {
                let listener=tokio::net::TcpListener::bind("127.0.0.1:3001").await.unwrap();
                let served=app.clone();
                let server=tokio::spawn(async move { axum::serve(listener,served).await.unwrap(); });
                let mut command=tokio::process::Command::new("node");
                command.env_clear()
                    .env("PATH",std::env::var_os("PATH").unwrap_or_default())
                    .env("HOME",std::env::var_os("HOME").unwrap_or_default())
                    .env("STAFF_REQUEST_ORIGIN","http://localhost:3001")
                    .env("STAFF_REQUEST_COOKIE",&cookie)
                    .env("STAFF_REQUEST_TARGETS",serde_json::json!({"board":board,"posts":posts[3..7],"template":template}).to_string())
                    .current_dir(concat!(env!("CARGO_MANIFEST_DIR"),"/../.."))
                    .arg("tests/browser/janitor-requests.mjs").kill_on_drop(true);
                if let Some(path)=std::env::var_os("PLAYWRIGHT_BROWSERS_PATH") { command.env("PLAYWRIGHT_BROWSERS_PATH",path); }
                let result=tokio::time::timeout(Duration::from_secs(120),command.output()).await;
                server.abort(); let _=server.await;
                let output=result.expect("janitor browser deadline").unwrap();
                println!("{}",String::from_utf8_lossy(&output.stdout));
                assert!(output.status.success(),"Janitor browser failed: {}",String::from_utf8_lossy(&output.stderr));
            }
            sqlx::query("UPDATE staff_identity.sessions SET expires_at=clock_timestamp()-interval '1 second' WHERE account_id=$1").bind(account).execute(&owner).await.unwrap();
            let expired=format!("board={board}&target={}&template={template}&csrf={csrf}",posts[2]);
            assert_eq!(app.clone().oneshot(post(expired,"http://localhost:3001")).await.unwrap().status(),StatusCode::UNAUTHORIZED);
            sqlx::query("UPDATE staff_identity.sessions SET expires_at=clock_timestamp()+interval '1 hour' WHERE account_id=$1").bind(account).execute(&owner).await.unwrap();
            sqlx::query("UPDATE staff_identity.accounts SET revoked_at=clock_timestamp() WHERE id=$1").bind(account).execute(&owner).await.unwrap();
            assert_eq!(app.clone().oneshot(get(&format!("/ban-request?board={board}&target={}",posts[2]),&cookie)).await.unwrap().status(),StatusCode::UNAUTHORIZED);
            let untouched:bool=sqlx::query_scalar("SELECT NOT deleted AND comment='Owned HTTP request body' FROM content.posts WHERE id=$1").bind(posts[2]).fetch_one(&owner).await.unwrap();
            assert!(untouched);
        }
    }).await;
    let mut cleanup = owner.begin().await.unwrap();
    sqlx::query("SET LOCAL ROLE board_restriction_owner")
        .execute(&mut *cleanup)
        .await
        .unwrap();
    sqlx::query("DELETE FROM restrictions.request_reviews WHERE account_id=$1 OR reviewer_id=$1")
        .bind(account)
        .execute(&mut *cleanup)
        .await
        .unwrap();
    sqlx::query("DELETE FROM restrictions.request_totals WHERE account_id=$1")
        .bind(account)
        .execute(&mut *cleanup)
        .await
        .unwrap();
    sqlx::query("DELETE FROM restrictions.audit WHERE ban_id IN(SELECT id FROM restrictions.bans WHERE account_id=$1)").bind(account).execute(&mut *cleanup).await.unwrap();
    sqlx::query("WITH gone AS (DELETE FROM restrictions.bans WHERE account_id=$1 RETURNING charged_bytes) UPDATE restrictions.capacity SET used_bytes=used_bytes-(SELECT coalesce(sum(charged_bytes),0) FROM gone) WHERE singleton").bind(account).execute(&mut *cleanup).await.unwrap();
    sqlx::query("WITH gone AS (DELETE FROM restrictions.requests WHERE account_id=$1 RETURNING charged_bytes) UPDATE restrictions.request_capacity SET used_bytes=used_bytes-(SELECT coalesce(sum(charged_bytes),0) FROM gone) WHERE singleton").bind(account).execute(&mut *cleanup).await.unwrap();
    sqlx::query("DELETE FROM restrictions.templates WHERE id IN($1,$2)")
        .bind(template)
        .bind(template + 1)
        .execute(&mut *cleanup)
        .await
        .unwrap();
    sqlx::query("RESET ROLE")
        .execute(&mut *cleanup)
        .await
        .unwrap();
    sqlx::query("DELETE FROM staff_identity.sessions WHERE account_id=$1")
        .bind(account)
        .execute(&mut *cleanup)
        .await
        .unwrap();
    sqlx::query("DELETE FROM staff_identity.credentials WHERE account_id=$1")
        .bind(account)
        .execute(&mut *cleanup)
        .await
        .unwrap();
    sqlx::query("DELETE FROM staff_identity.accounts WHERE id=$1")
        .bind(account)
        .execute(&mut *cleanup)
        .await
        .unwrap();
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(&board)
        .execute(&mut *cleanup)
        .await
        .unwrap();
    sqlx::query("SELECT id FROM content.threads WHERE board=$1 ORDER BY id FOR UPDATE")
        .bind(&board)
        .execute(&mut *cleanup)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.posts WHERE board=$1")
        .bind(&board)
        .execute(&mut *cleanup)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.threads WHERE board=$1")
        .bind(&board)
        .execute(&mut *cleanup)
        .await
        .unwrap();
    sqlx::query("DELETE FROM content.boards WHERE slug=$1")
        .bind(&board)
        .execute(&mut *cleanup)
        .await
        .unwrap();
    cleanup.commit().await.unwrap();
    outcome.unwrap();
}
