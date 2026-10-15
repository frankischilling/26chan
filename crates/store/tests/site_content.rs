#![cfg(feature = "database-tests")]
use board_store::site_content::{self, Error};
use sqlx::{Connection, PgConnection, PgPool};

fn input(value: serde_json::Value) -> site_content::Input {
    site_content::read_input(serde_json::to_vec(&value).unwrap().as_slice()).unwrap()
}

#[tokio::test]
async fn operator_content_is_revision_fenced_public_only_when_published_and_role_isolated() {
    let owner_url = std::env::var("MIGRATION_DATABASE_URL").unwrap();
    let public = PgPool::connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut owner = PgConnection::connect(&owner_url).await.unwrap();
    // This test must never replace preexisting operator content.
    assert!(
        sqlx::query_scalar::<_, bool>(site_content::READINESS_SQL)
            .fetch_one(&public)
            .await
            .unwrap()
    );
    for drift in [
        "GRANT USAGE ON SCHEMA site_private TO board_public",
        "GRANT SELECT(body) ON site_private.pages TO board_staff",
        "GRANT SELECT ON content.published_news TO board_public WITH GRANT OPTION",
        "GRANT UPDATE ON content.published_site_pages TO board_public",
        "ALTER VIEW content.published_news SET (security_barrier=false)",
        "CREATE OR REPLACE VIEW content.published_news WITH(security_barrier=true) AS SELECT id,published_at,title,body,author FROM site_private.news WHERE published",
        "ALTER TABLE site_private.news DROP CONSTRAINT news_body_check",
    ] {
        let mut tx = owner.begin().await.unwrap();
        sqlx::query(drift).execute(&mut *tx).await.unwrap();
        assert!(
            !sqlx::query_scalar::<_, bool>(site_content::READINESS_SQL)
                .fetch_one(&mut *tx)
                .await
                .unwrap(),
            "{drift}"
        );
        tx.rollback().await.unwrap();
    }
    assert!(
        sqlx::query_scalar::<_, bool>(site_content::READINESS_SQL)
            .fetch_one(&public)
            .await
            .unwrap()
    );
    let empty: bool = sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM site_private.pages) AND NOT EXISTS(SELECT 1 FROM site_private.news)")
        .fetch_one(&mut owner).await.unwrap();
    assert!(empty, "requires fresh disposable site-content tables");
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos()
        & 0xffffff;
    let board = format!("sc{nonce:06x}");
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Owned site board','',1000,100,100,100,10)")
        .bind(&board).execute(&mut owner).await.unwrap();
    let result = tokio::spawn({
        let owner_url = owner_url.clone(); let public = public.clone(); let board = board.clone();
        async move {
            let mut owner = PgConnection::connect(&owner_url).await.unwrap();
            let page = |key: &str, expected| input(serde_json::json!({"kind":"page","version":1,"key":key,"expected_revision":expected,"title":"Owned site content","body":"owned-source-site-content <script>text</script>"}));
            assert_eq!(site_content::page_revision(&mut owner,"faq").await.unwrap(),0);
            assert_eq!(site_content::publish(&mut owner,&page("faq",0)).await.unwrap(),1);
            assert!(matches!(site_content::publish(&mut owner,&page("faq",0)).await,Err(Error::Revision)));
            assert_eq!(site_content::site_page(&public,"faq").await.unwrap().unwrap().revision,1);
            assert_eq!(site_content::publish(&mut owner,&page("faq",1)).await.unwrap(),2);
            assert!(matches!(site_content::retract_page(&mut owner,"faq",1).await,Err(Error::Revision)));
            site_content::retract_page(&mut owner,"faq",2).await.unwrap();
            assert!(site_content::site_page(&public,"faq").await.unwrap().is_none());
            assert_eq!(site_content::page_revision(&mut owner,"faq").await.unwrap(),3);
            assert_eq!(site_content::publish(&mut owner,&page("faq",3)).await.unwrap(),4);
            let key=format!("rules/{board}");
            site_content::publish(&mut owner,&page(&key,0)).await.unwrap();
            assert_eq!(site_content::site_page(&public,&key).await.unwrap().unwrap().board.as_deref(),Some(board.as_str()));
            sqlx::query("UPDATE content.boards SET staff_only=true WHERE slug=$1").bind(&board).execute(&mut owner).await.unwrap();
            assert!(site_content::site_page(&public,&key).await.unwrap().is_none());
            assert!(site_content::board_rules(&public).await.unwrap().is_empty());
            let now=chrono::Utc::now().timestamp();
            let news=|time|input(serde_json::json!({"kind":"news","version":1,"published_at":time,"title":"Owned news","author":"Owned editor","body":"owned-source-site-content announcement"}));
            assert_eq!(site_content::publish(&mut owner,&news(now-10)).await.unwrap(),1);
            assert!(matches!(site_content::publish(&mut owner,&news(now-10)).await,Err(Error::NewsOrder)));
            assert_eq!(site_content::publish(&mut owner,&news(now-5)).await.unwrap(),2);
            assert_eq!(site_content::publish(&mut owner,&news(now+3600)).await.unwrap(),3);
            assert_eq!(site_content::news_page(&public,None,25).await.unwrap().iter().map(|n|n.id).collect::<Vec<_>>(),[2,1]);
            assert_eq!(site_content::news_page(&public,Some(2),1).await.unwrap()[0].id,1);
            assert_eq!(site_content::news_item(&public,2).await.unwrap().unwrap().author,"Owned editor");
            assert!(site_content::news_item(&public,3).await.unwrap().is_none());
            site_content::retract_news(&mut owner,2).await.unwrap();
            assert!(site_content::news_item(&public,2).await.unwrap().is_none());
            assert_eq!(site_content::news_page(&public,None,25).await.unwrap().iter().map(|n|n.id).collect::<Vec<_>>(),[1]);
            for statement in ["SELECT * FROM site_private.pages", "SELECT * FROM site_private.news", "UPDATE content.published_news SET title='denied'", "DELETE FROM content.published_site_pages"] {
                let error=sqlx::query(statement).execute(&public).await.unwrap_err();
                assert_eq!(error.as_database_error().unwrap().code().as_deref(),Some("42501"));
            }
            let mut public_connection=PgConnection::connect(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap()).await.unwrap();
            assert!(matches!(site_content::publish(&mut public_connection,&page("faq",4)).await,Err(Error::Role)));
            // The migrator already has SET permission for this narrow owner role.
            // Switching away must not retain site-maintenance authority.
            sqlx::query("SET ROLE board_attachment_owner").execute(&mut owner).await.unwrap();
            assert!(matches!(site_content::retract_news(&mut owner,1).await,Err(Error::Role)));
            sqlx::query("RESET ROLE").execute(&mut owner).await.unwrap();
            assert_eq!(site_content::page_revision(&mut owner,"faq").await.unwrap(),4);
        }
    }).await;
    sqlx::query("DELETE FROM site_private.pages WHERE (key='faq' OR board=$1) AND body LIKE 'owned-source-site-content %'").bind(&board).execute(&mut owner).await.unwrap();
    sqlx::query("DELETE FROM site_private.news WHERE id IN (1,2,3) AND body='owned-source-site-content announcement'").execute(&mut owner).await.unwrap();
    sqlx::query("DELETE FROM content.boards WHERE slug=$1 AND title='Owned site board'")
        .bind(&board)
        .execute(&mut owner)
        .await
        .unwrap();
    result.unwrap();
}
