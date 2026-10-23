#![cfg(feature = "database-tests")]
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use board_store::BoardSelection;
use tower::ServiceExt;

#[tokio::test]
async fn upload_old_marking_uses_whole_board_ids_and_thread_sticky_state() {
    let owner = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = sqlx::PgPool::connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT 'so'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,captcha_enabled) VALUES($1,'Owned old marking','Synthetic',1000,100,100,100,30,false)").bind(&board).execute(&owner).await.unwrap();
    let saved = board.clone();
    let admin = owner.clone();
    let result=tokio::spawn(async move {
        let (web,_)=board_public::routers_with_media(public.clone(),"http://127.0.0.1:3000".into(),false,None);
        let mut ids=Vec::new();
        for (additional,marked) in [(25,false),(1,true),(10,true)] {
            // Historical text rows predate the operator's upload-mode switch.
            sqlx::query("UPDATE content.boards SET upload_board=false WHERE slug=$1").bind(&board).execute(&admin).await.unwrap();
            let added:Vec<i64>=sqlx::query_scalar("WITH created AS (INSERT INTO content.threads(board) SELECT $1 FROM generate_series(1,$2) RETURNING id,board) INSERT INTO content.posts(id,board,thread_id,name,subject,comment) SELECT id,board,id,'Owned','4|Historical movie','Owned historical text' FROM created RETURNING id").bind(&board).bind(additional).fetch_all(&admin).await.unwrap();
            ids.extend(added);ids.sort_unstable();
            sqlx::query("UPDATE content.boards SET upload_board=true WHERE slug=$1").bind(&board).execute(&admin).await.unwrap();
            let page=board_store::source_board_page_snapshot(&public,&board,BoardSelection::Page(1)).await.unwrap();
            assert_eq!(page.snapshot.upload_board_old_ids,if marked {ids[..5].to_vec()}else{vec![]});
            let thread=board_store::thread_page_snapshot(&public,&board,ids[0]).await.unwrap();
            assert_eq!(thread.snapshot.upload_board_old_ids,page.snapshot.upload_board_old_ids);
            let response=web.clone().oneshot(Request::get(format!("/{board}/thread/{}",ids[0])).body(Body::empty()).unwrap()).await.unwrap();
            assert_eq!(response.status(),StatusCode::OK);
            let html=String::from_utf8(to_bytes(response.into_body(),1024*1024).await.unwrap().to_vec()).unwrap();
            assert_eq!(html.contains("Marked for deletion (old)."),marked);
            if ids.len()>30 {
                assert_eq!(page.snapshot.threads.len(),30);
                assert!(!page.snapshot.threads.iter().any(|view|view.thread.id==ids[0]),"Oldest IDs must not be replaced by oldest visible rows");
            }
        }
        sqlx::query("UPDATE content.threads SET sticky=true WHERE id=$1").bind(ids[0]).execute(&admin).await.unwrap();
        let response=web.oneshot(Request::get(format!("/{board}/thread/{}",ids[0])).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(),StatusCode::OK);
        let html=String::from_utf8(to_bytes(response.into_body(),1024*1024).await.unwrap().to_vec()).unwrap();
        assert!(!html.contains("Marked for deletion (old)."));
    }).await;
    for query in [
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(query)
            .bind(&saved)
            .execute(&owner)
            .await
            .unwrap();
    }
    result.unwrap();
}
