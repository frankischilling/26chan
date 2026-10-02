#![cfg(feature = "database-tests")]
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use board_staff::{AppState, Config, Limits, auth};
use chrono::Timelike;
use sqlx::PgPool;
use std::{sync::Arc, time::Duration};
use tower::ServiceExt;
use webauthn_rs::prelude::*;

async fn pool(key: &str) -> PgPool {
    PgPool::connect(&std::env::var(key).expect("explicit owned database credential required"))
        .await
        .unwrap()
}

struct Fixture {
    owner: PgPool,
    state: Arc<AppState>,
    public: Router,
    api: Router,
    board: String,
    account: i64,
    token: String,
    csrf: String,
}
impl Fixture {
    async fn new() -> Arc<Self> {
        let owner = pool("MIGRATION_DATABASE_URL").await;
        let board = format!("c{}", &uuid::Uuid::new_v4().simple().to_string()[..9]);
        sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Owned staff posting','Synthetic',16000,100,100,100,10)")
            .bind(&board).execute(&owner).await.unwrap();
        let account: i64 = sqlx::query_scalar(
            "INSERT INTO staff_identity.accounts(role) VALUES('moderator') RETURNING id",
        )
        .fetch_one(&owner)
        .await
        .unwrap();
        let credential = uuid::Uuid::new_v4().as_bytes().to_vec();
        sqlx::query(
            "INSERT INTO staff_identity.credentials(id,account_id,credential) VALUES($1,$2,'{}')",
        )
        .bind(&credential)
        .bind(account)
        .execute(&owner)
        .await
        .unwrap();
        let token = auth::token();
        let csrf = auth::token();
        sqlx::query("INSERT INTO staff_identity.sessions(token_hash,csrf_hash,account_id,credential_id) VALUES($1,$2,$3,$4)")
            .bind(auth::hash(&token)).bind(auth::hash(&csrf)).bind(account).bind(credential).execute(&owner).await.unwrap();
        let origin = Url::parse("http://localhost:3001").unwrap();
        let state = Arc::new(AppState {
            config: Config {
                origin: origin.origin().ascii_serialization(),
                public_origin: "http://127.0.0.1:3000".into(),
                media_origin: "http://127.0.0.1:3002".into(),
                bind: "127.0.0.1:3001".parse().unwrap(),
                production: false,
                auth_database: String::new(),
                staff_database: String::new(),
                idle_timeout: Duration::from_secs(900),
            },
            auth: pool("AUTH_DATABASE_URL").await,
            staff: pool("STAFF_DATABASE_URL").await,
            webauthn: WebauthnBuilder::new("localhost", &origin)
                .unwrap()
                .build()
                .unwrap(),
            limits: Limits::default(),
        });
        let (public, api) = board_public::routers(
            pool("TEST_PUBLIC_DATABASE_URL").await,
            "http://127.0.0.1:3000".into(),
            false,
        );
        Arc::new(Self {
            owner,
            state,
            public,
            api,
            board,
            account,
            token,
            csrf,
        })
    }
    async fn submit(
        &self,
        thread: i64,
        extra: &str,
        csrf: &str,
        origin: &str,
    ) -> (StatusCode, Option<String>) {
        let response=board_staff::router(self.state.clone()).oneshot(Request::post("/post")
            .header("content-type","application/x-www-form-urlencoded").header("origin",origin).header("sec-fetch-site","same-origin")
            .header("cookie",format!("staff={}",self.token))
            .body(Body::from(format!("csrf={csrf}&board={}&thread={thread}&name=Owned+staff&subject=Owned+notice&comment=%3Cscript%3Eharmless%3C%2Fscript%3E{extra}",self.board))).unwrap()).await.unwrap();
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        (
            response.status(),
            response
                .headers()
                .get("location")
                .map(|value| value.to_str().unwrap().into()),
        )
    }
    async fn latest(&self) -> (i64, i64, Option<String>) {
        sqlx::query_as("SELECT id,thread_id,capcode FROM content.posts WHERE board=$1 ORDER BY id DESC LIMIT 1").bind(&self.board).fetch_one(&self.owner).await.unwrap()
    }
    async fn cleanup(&self) {
        let discussion_threads: Vec<i64> = sqlx::query_scalar(
            "SELECT p.id FROM staff_identity.discussion_posts d JOIN content.posts p ON p.id=d.post_id \
             WHERE d.account_id=$1 AND p.board='j' AND p.thread_id=p.id",
        ).bind(self.account).fetch_all(&self.owner).await.unwrap();
        sqlx::query("DELETE FROM content.moderation_audit WHERE board='j' AND account_id=$1")
            .bind(self.account)
            .execute(&self.owner)
            .await
            .unwrap();
        for query in [
            "DELETE FROM content.posts WHERE board='j' AND thread_id=ANY($1)",
            "DELETE FROM content.threads WHERE board='j' AND id=ANY($1)",
        ] {
            sqlx::query(query)
                .bind(&discussion_threads)
                .execute(&self.owner)
                .await
                .unwrap();
        }
        for query in [
            "DELETE FROM content.moderation_audit WHERE board=$1",
            "DELETE FROM content.reports WHERE board=$1",
            "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
            "DELETE FROM content.posts WHERE board=$1",
            "DELETE FROM content.threads WHERE board=$1",
            "DELETE FROM content.boards WHERE slug=$1",
        ] {
            sqlx::query(query)
                .bind(&self.board)
                .execute(&self.owner)
                .await
                .unwrap();
        }
        for query in [
            "DELETE FROM staff_identity.sessions WHERE account_id=$1",
            "DELETE FROM staff_identity.credentials WHERE account_id=$1",
            "DELETE FROM staff_identity.accounts WHERE id=$1",
        ] {
            sqlx::query(query)
                .bind(self.account)
                .execute(&self.owner)
                .await
                .unwrap();
        }
    }
}

async fn denied(pool: &PgPool, query: &'static str) {
    let error = sqlx::query(query)
        .execute(pool)
        .await
        .expect_err("forbidden authority granted");
    assert_eq!(
        error
            .as_database_error()
            .and_then(|error| error.code())
            .as_deref(),
        Some("42501")
    );
    assert_eq!(
        sqlx::query_scalar::<_, i32>("SELECT 1")
            .fetch_one(pool)
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn authenticated_staff_posts_have_scoped_persisted_badges_and_public_forms_have_no_authority()
{
    let fixture = Fixture::new().await;
    let case = fixture.clone();
    let result=tokio::spawn(async move {
        let app=board_staff::router(case.state.clone());
        let response=app.clone().oneshot(Request::get("/post").body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(),StatusCode::UNAUTHORIZED);
        let response=app.clone().oneshot(Request::get("/post").header("cookie",format!("staff={}; staff-csrf={}",case.token,case.csrf)).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(),StatusCode::OK);
        let html=String::from_utf8(to_bytes(response.into_body(),262144).await.unwrap().to_vec()).unwrap();
        assert!(html.contains("Staff posting")); assert!(!html.contains("name=\"highlight\""));
        for (extra,csrf,origin,status) in [
            ("","bad","http://localhost:3001",StatusCode::FORBIDDEN),
            ("",case.csrf.as_str(),"http://127.0.0.1:3000",StatusCode::FORBIDDEN),
            ("&capcode=admin",case.csrf.as_str(),"http://localhost:3001",StatusCode::UNPROCESSABLE_ENTITY),
            ("&highlight=true",case.csrf.as_str(),"http://localhost:3001",StatusCode::UNAUTHORIZED),
        ] { assert_eq!(case.submit(0,extra,csrf,origin).await.0,status); }
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM content.posts WHERE board=$1").bind(&case.board).fetch_one(&case.owner).await.unwrap(),0);
        for extra in ["&capcode=admin", "&role=admin", "&staff=true", "&highlight=true"] {
            let response=case.public.clone().oneshot(Request::post(format!("/{}/post",case.board))
                .header("content-type","application/x-www-form-urlencoded").header("origin","http://127.0.0.1:3000")
                .header("accept","application/json").body(Body::from(format!("resto=0&sub=Owned&com=Public&pwd=owned-public-password{extra}"))).unwrap()).await.unwrap();
            assert_eq!(response.status(),StatusCode::UNPROCESSABLE_ENTITY);
        }
        let response=case.public.clone().oneshot(Request::post(format!("/{}/post",case.board))
            .header("content-type","application/x-www-form-urlencoded").header("origin","http://127.0.0.1:3000")
            .header("accept","application/json").header("x-staff-capcode","admin")
            .header("cookie",format!("staff={}; staff-csrf={}",case.token,case.csrf))
            .body(Body::from("resto=0&sub=Owned&com=Public&pwd=owned-public-password")).unwrap()).await.unwrap();
        assert_eq!(response.status(),StatusCode::OK);
        let ordinary:serde_json::Value=serde_json::from_slice(&to_bytes(response.into_body(),8192).await.unwrap()).unwrap();
        let public_id=ordinary["pid"].as_i64().expect("Healthy public posting must work before forgery denials are qualified");
        assert!(sqlx::query_scalar::<_,Option<String>>("SELECT capcode FROM content.posts WHERE id=$1").bind(public_id).fetch_one(&case.owner).await.unwrap().is_none());
        let session_hash=auth::hash(&case.token); let csrf_hash=auth::hash(&case.csrf);
        let direct=board_store::create_staff_post(&case.state.staff,&case.board,0,
            &board_store::NewPost { name:"Owned direct staff#private-trip-suffix".into(),subject:"Owned".into(),comment:"Synthetic direct authority".into(),deletion_hash:String::new(),sage:false },chrono::Utc::now(),
            board_store::StaffPostAuthority { auth_pool:&case.state.auth,session_hash:&session_hash,csrf_hash:&csrf_hash,ticket_hash:&auth::hash(&auth::token()).try_into().unwrap(),idle_seconds:900,highlight:false }).await.expect("actual staff posting transaction must succeed");
        assert!(direct>0);
        assert_eq!(sqlx::query_scalar::<_,String>("SELECT name FROM content.posts WHERE id=$1").bind(direct).fetch_one(&case.owner).await.unwrap(),"Owned direct staff");
        let (status,location)=case.submit(0,"",&case.csrf,"http://localhost:3001").await;
        assert_eq!(status,StatusCode::SEE_OTHER);
        let (op,thread,capcode)=case.latest().await; assert_eq!(op,thread);assert_eq!(capcode.as_deref(),Some("mod"));
        assert_eq!(location.unwrap(),format!("/post?board={}&thread={op}&posted={op}",case.board));
        sqlx::query("UPDATE content.boards SET user_ids=true,country_flags=true WHERE slug=$1").bind(&case.board).execute(&case.owner).await.unwrap();
        assert_eq!(case.submit(op,"",&case.csrf,"http://localhost:3001").await.0,StatusCode::SEE_OTHER);
        let (reply,_,capcode)=case.latest().await;assert_eq!(capcode.as_deref(),Some("mod"));
        let identity:(Option<String>,Option<String>,Option<String>)=sqlx::query_as("SELECT poster_id,trip,country FROM content.posts WHERE id=$1").bind(reply).fetch_one(&case.owner).await.unwrap();
        assert_eq!(identity,(None,None,None));
        for query in ["SELECT * FROM post_secrets.staff_post_intents LIMIT 1","SELECT * FROM staff_identity.sessions LIMIT 1","UPDATE content.posts SET capcode='admin' WHERE false","INSERT INTO content.posts(id,board,thread_id,name,subject,comment,capcode) SELECT 1,'x',1,'x','','x','admin' WHERE false"] {
            denied(&case.state.staff,query).await;
        }
        denied(&case.state.auth,"UPDATE staff_identity.accounts SET public_capcode='admin' WHERE false").await;
        denied(&case.state.staff,"SELECT staff_identity.issue_post_authority(NULL,NULL,NULL,900,false,1,'x',1,'x','','x',clock_timestamp())").await;
        let public=pool("TEST_PUBLIC_DATABASE_URL").await;
        denied(&public,"SELECT content.consume_staff_post_authority(NULL,1,'x',1,'x','','x',clock_timestamp())").await;
        denied(&public,"SELECT * FROM post_secrets.staff_post_intents LIMIT 1").await;
        let direct=sqlx::query("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,$2,'Unproved','','Harmless')")
            .bind(&case.board).bind(op).execute(&case.state.staff).await.unwrap_err();
        assert_eq!(direct.as_database_error().and_then(|error|error.code()).as_deref(),Some("28000"));
        sqlx::query("UPDATE staff_identity.accounts SET role='admin' WHERE id=$1").bind(case.account).execute(&case.owner).await.unwrap();
        for (label,highlight,saved) in [("admin",false,"admin"),("admin",true,"admin_highlight"),("manager",false,"manager"),("developer",false,"developer"),("founder",false,"founder")] {
            sqlx::query("UPDATE staff_identity.accounts SET public_capcode=$2 WHERE id=$1").bind(case.account).bind(label).execute(&case.owner).await.unwrap();
            assert_eq!(case.submit(op,if highlight {"&highlight=true"} else {""},&case.csrf,"http://localhost:3001").await.0,StatusCode::SEE_OTHER);
            let (id,_,capcode)=case.latest().await;assert_eq!(capcode.as_deref(),Some(saved));
            let response=case.public.clone().oneshot(Request::get(format!("/{}/thread/{op}.json",case.board)).body(Body::empty()).unwrap()).await.unwrap();
            assert_eq!(response.status(),StatusCode::OK);
            let value:serde_json::Value=serde_json::from_slice(&to_bytes(response.into_body(),4_194_304).await.unwrap()).unwrap();
            let post=value["posts"].as_array().unwrap().iter().find(|post|post["no"]==id).unwrap();
            assert_eq!(post["capcode"],saved);assert!(post.get("id").is_none());assert!(post.get("country").is_none());
            assert!(!post["com"].as_str().unwrap().contains("<script>"));
        }
        let (last,_,_)=case.latest().await;
        sqlx::query("UPDATE content.boards SET json_tail_size=1 WHERE slug=$1").bind(&case.board).execute(&case.owner).await.unwrap();
        for router in [&case.public,&case.api] {
            let full=public_json(router,&format!("/{}/thread/{op}.json",case.board)).await;
            assert_eq!(full["posts"][0]["capcode"],"mod");
            assert!(full["posts"].as_array().unwrap().iter().all(|post|post.get("id").is_none() && post.get("trip").is_none() && post.get("country").is_none()));
            let tail=public_json(router,&format!("/{}/thread/{op}-tail.json",case.board)).await;
            assert!(tail["posts"][0].get("capcode").is_none());assert_eq!(tail["posts"][1]["capcode"],"founder");
            let index=public_json(router,&format!("/{}/1.json",case.board)).await;
            let entry=index["threads"].as_array().unwrap().iter().find(|entry|entry["posts"][0]["no"]==op).unwrap();
            assert_eq!(entry["posts"][0]["capcode"],"mod");
            let catalog=public_json(router,&format!("/{}/catalog.json",case.board)).await;
            let entry=catalog[0]["threads"].as_array().unwrap().iter().find(|entry|entry["no"]==op).unwrap();
            assert_eq!(entry["capcode"],"mod");assert_eq!(entry["last_replies"].as_array().unwrap().last().unwrap()["capcode"],"founder");
        }
        let html=public_text(&case.public,&format!("/{}/thread/{op}",case.board)).await;
        for label in ["Mod","Admin","Manager","Developer","Founder"] {assert!(html.contains(&format!(">## {label}</strong>")));}
        assert!(html.contains(" highlightPost"));assert!(!html.contains("foundericon@2x"));assert!(!html.contains("<script>harmless"));
        let update=public_json(&case.public,&format!("/_watch/{}/thread/{op}/posts",case.board)).await;
        assert!(update["posts"].as_array().unwrap().iter().all(|post|post["html"].as_str().unwrap().contains("class=\"identityIcon\"")));
        let preview=public_json(&case.public,&format!("/_watch/{}/post/{last}",case.board)).await;
        assert!(preview["post"]["html"].as_str().unwrap().contains(">## Founder</strong>"));
        for (posted,status) in [(last,StatusCode::OK),(public_id,StatusCode::NOT_FOUND)] {
            let response=app.clone().oneshot(Request::get(format!("/post?board={}&thread={op}&posted={posted}",case.board))
                .header("cookie",format!("staff={}; staff-csrf={}",case.token,case.csrf)).body(Body::empty()).unwrap()).await.unwrap();
            assert_eq!(response.status(),status);
        }
        sqlx::query("INSERT INTO content.reports(board,post_id,reason) VALUES($1,$2,'Synthetic staff notice report')").bind(&case.board).bind(last).execute(&case.owner).await.unwrap();
        let response=app.clone().oneshot(Request::get("/reports").header("cookie",format!("staff={}; staff-csrf={}",case.token,case.csrf)).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(),StatusCode::OK);
        let queue=String::from_utf8(to_bytes(response.into_body(),262144).await.unwrap().to_vec()).unwrap();
        assert!(queue.contains("<strong>## Founder</strong>"));assert!(!queue.contains("<script>"));
        let audit:i64=sqlx::query_scalar("SELECT count(*) FROM content.moderation_audit WHERE board=$1 AND account_id=$2 AND action='staff-post'").bind(&case.board).bind(case.account).fetch_one(&case.owner).await.unwrap();
        assert_eq!(audit,8);
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM post_secrets.staff_post_intents WHERE account_id=$1").bind(case.account).fetch_one(&case.owner).await.unwrap(),0);
        for query in ["UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp()-interval '11 minutes' WHERE token_hash=$1","UPDATE staff_identity.sessions SET expires_at=clock_timestamp()-interval '1 second' WHERE token_hash=$1"] {
            sqlx::query(query).bind(auth::hash(&case.token)).execute(&case.owner).await.unwrap();
            let status=case.submit(op,"",&case.csrf,"http://localhost:3001").await.0;
            assert!(matches!(status,StatusCode::UNAUTHORIZED|StatusCode::FORBIDDEN));
        }
    }).await;
    fixture.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn authenticated_capcoded_staff_bypass_robot9000_without_registering_text() {
    let fixture = Fixture::new().await;
    let case = fixture.clone();
    let outcome=tokio::spawn(async move {
        sqlx::query("UPDATE content.boards SET robot9000=true WHERE slug=$1")
            .bind(&case.board).execute(&case.owner).await.unwrap();
        for _ in 0..2 {
            assert_eq!(case.submit(0,"",&case.csrf,"http://localhost:3001").await.0,StatusCode::SEE_OTHER);
            assert_eq!(case.latest().await.2.as_deref(),Some("mod"));
        }
        let counts:(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM post_secrets.robot9000_texts WHERE board=$1),(SELECT count(*) FROM post_secrets.robot9000_mutes WHERE board=$1)")
            .bind(&case.board).fetch_one(&case.owner).await.unwrap();
        assert_eq!(counts,(0,0));
        sqlx::query("UPDATE staff_identity.accounts SET revoked_at=clock_timestamp() WHERE id=$1")
            .bind(case.account).execute(&case.owner).await.unwrap();
        assert_eq!(case.submit(0,"",&case.csrf,"http://localhost:3001").await.0,StatusCode::UNAUTHORIZED);
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM content.posts WHERE board=$1").bind(&case.board).fetch_one(&case.owner).await.unwrap(),2);
    }).await;
    fixture.cleanup().await;
    outcome.unwrap();
}

#[derive(Clone)]
struct BoundPost {
    ticket: Vec<u8>,
    id: i64,
    thread: i64,
    board: String,
    name: String,
    subject: String,
    comment: String,
    wordfilter_payload: Option<Vec<u8>>,
    wordfilter_search: Option<String>,
    time: chrono::DateTime<chrono::Utc>,
}
impl BoundPost {
    async fn new(case: &Fixture, thread: i64) -> Self {
        Self {
            ticket: auth::hash(&auth::token()),
            id: sqlx::query_scalar("SELECT nextval('content.post_number')")
                .fetch_one(&case.owner)
                .await
                .unwrap(),
            thread,
            board: case.board.clone(),
            name: "Bound staff".into(),
            subject: "Bound subject".into(),
            comment: "Bound synthetic comment".into(),
            wordfilter_payload: None,
            wordfilter_search: None,
            time: chrono::Utc::now().with_nanosecond(0).unwrap(),
        }
    }
    async fn issue(&self, case: &Fixture) -> Result<(), sqlx::Error> {
        sqlx::query(
            "SELECT staff_identity.issue_wordfiltered_post_authority($1,$2,$3,900,false,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
        )
        .bind(&self.ticket)
        .bind(auth::hash(&case.token))
        .bind(auth::hash(&case.csrf))
        .bind(self.id)
        .bind(&self.board)
        .bind(self.thread)
        .bind(&self.name)
        .bind(&self.subject)
        .bind(&self.comment)
        .bind(self.time)
        .bind(self.wordfilter_payload.as_deref())
        .bind(self.wordfilter_search.as_deref())
        .execute(&case.state.auth)
        .await?;
        Ok(())
    }
    async fn insert(&self, case: &Fixture) -> Result<String, sqlx::Error> {
        let mut tx = case.state.staff.begin().await?;
        let ticket = self
            .ticket
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        sqlx::query("SELECT set_config('board.staff_post_ticket',$1,true)")
            .bind(ticket)
            .execute(&mut *tx)
            .await?;
        let payload = self
            .wordfilter_payload
            .as_ref()
            .map_or_else(String::new, |bytes| {
                bytes
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>()
            });
        sqlx::query("SELECT set_config('board.wordfilter_payload',$1,true),set_config('board.wordfilter_search',$2,true)")
            .bind(payload)
            .bind(self.wordfilter_search.as_deref().unwrap_or_default())
            .execute(&mut *tx)
            .await?;
        let result=sqlx::query_scalar("INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at) VALUES($1,$2,$3,$4,$5,$6,$7) RETURNING capcode")
            .bind(self.id).bind(&self.board).bind(self.thread).bind(&self.name).bind(&self.subject)
            .bind(&self.comment).bind(self.time).fetch_one(&mut *tx).await;
        match result {
            Ok(value) => {
                tx.commit().await?;
                Ok(value)
            }
            Err(error) => {
                tx.rollback().await?;
                Err(error)
            }
        }
    }
}
fn sql_code(error: &sqlx::Error) -> Option<String> {
    error
        .as_database_error()
        .and_then(|error| error.code())
        .map(|code| code.into_owned())
}

#[tokio::test]
async fn staff_wordfilters_bind_the_exact_saved_body_without_granting_html_authority() {
    let fixture = Fixture::new().await;
    let case = fixture.clone();
    let outcome=tokio::spawn(async move {
        sqlx::query("UPDATE content.boards SET word_filter_enabled=true,word_filter_profile=0 WHERE slug=$1").bind(&case.board).execute(&case.owner).await.unwrap();
        let session_hash=auth::hash(&case.token); let csrf_hash=auth::hash(&case.csrf);
        let id=board_store::create_staff_post(&case.state.staff,&case.board,0,&board_store::NewPost {
            name:"soy fam CUCK#private suffix".into(),subject:"soy fam CUCK".into(),comment:"soy fam CUCK <script>literal</script>".into(),deletion_hash:String::new(),sage:false,
        },chrono::Utc::now(),
        board_store::StaffPostAuthority { auth_pool:&case.state.auth,session_hash:&session_hash,csrf_hash:&csrf_hash,ticket_hash:&auth::hash(&auth::token()).try_into().unwrap(),idle_seconds:900,highlight:false }).await.unwrap();
        let public=pool("TEST_PUBLIC_DATABASE_URL").await;
        let saved=board_store::find_post(&public,&case.board,id).await.unwrap();
        assert_eq!(saved.name,"soy fam CUCK"); assert_eq!(saved.subject,"soy fam CUCK"); assert_eq!(saved.capcode.as_deref(),Some("mod"));
        assert_eq!(saved.comment,"onions senpai KEK &lt;script&gt;literal&lt;/script&gt;");
        assert!(saved.wordfilter_payload.is_some()); assert!(saved.trip.is_none());
        let response=case.public.clone().oneshot(Request::get(format!("/{}/thread/{id}",case.board)).body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(response.status(),StatusCode::OK);
        let html=String::from_utf8(to_bytes(response.into_body(),1_000_000).await.unwrap().to_vec()).unwrap();
        assert!(html.contains("onions senpai KEK &#60;script&#62;literal&#60;/script&#62;"),"{html}"); assert!(!html.contains("<script>literal")); assert!(!html.contains("private suffix"));
        sqlx::query("UPDATE content.boards SET word_filter_profile=4 WHERE slug=$1").bind(&case.board).execute(&case.owner).await.unwrap();
        let mut typed=board_domain::wordfiltered_comment::prepare("bound & \" text",board_domain::comment_markup::MarkupPolicy::default(),board_domain::wordfilter::Profile::Test,Some(board_domain::wordfilter::LeetRolls::from_choices(0,3).unwrap())).unwrap();
        typed.freeze_format(&case.board);
        let mut bound=BoundPost::new(&case,id).await;
        bound.comment=board_domain::filtered_formatting::source_projection(&board_domain::filtered_formatting::lines(&typed,&case.board));
        bound.wordfilter_payload=Some(typed.encode().unwrap());
        bound.wordfilter_search=Some(board_domain::formatting::plain_text(&board_domain::filtered_formatting::lines(&typed,&case.board)));
        bound.issue(&case).await.unwrap();
        let mut changed=bound.clone(); changed.wordfilter_payload.as_mut().unwrap()[10]^=1;
        assert_eq!(sql_code(&changed.insert(&case).await.unwrap_err()).as_deref(),Some("28000"));
        let mut changed_search=bound.clone(); changed_search.wordfilter_search.as_mut().unwrap().push('x');
        assert_eq!(sql_code(&changed_search.insert(&case).await.unwrap_err()).as_deref(),Some("28000"));
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM post_secrets.staff_post_intents WHERE token_hash=$1").bind(&bound.ticket).fetch_one(&case.owner).await.unwrap(),1);
        assert_eq!(bound.insert(&case).await.unwrap(),"mod");
        let inserted=board_store::find_post(&public,&case.board,bound.id).await.unwrap();
        assert_eq!(inserted.wordfilter_payload,bound.wordfilter_payload);
        assert_eq!(sql_code(&bound.insert(&case).await.unwrap_err()).as_deref(),Some("28000"));
    }).await;
    fixture.cleanup().await;
    outcome.unwrap();
}

#[tokio::test]
async fn staff_authority_binds_every_prepared_field_and_can_be_consumed_only_once() {
    let fixture = Fixture::new().await;
    let case = fixture.clone();
    let result=tokio::spawn(async move {
        assert_eq!(case.submit(0,"",&case.csrf,"http://localhost:3001").await.0,StatusCode::SEE_OTHER);
        let (thread,_,_)=case.latest().await;
        let post=BoundPost::new(&case,thread).await;post.issue(&case).await.unwrap();
        for field in 0..7 {
            let mut bad=post.clone();
            match field {0=>bad.id+=1,1=>bad.board="unowned".into(),2=>bad.thread+=1,
                3=>bad.name.push('x'),4=>bad.subject.push('x'),5=>bad.comment.push('x'),
                6=>bad.time+=chrono::Duration::seconds(1),_=>unreachable!()}
            assert_eq!(sql_code(&bad.insert(&case).await.unwrap_err()).as_deref(),Some("28000"));
        }
        let (first,second)=tokio::join!(post.insert(&case),post.insert(&case));
        assert_eq!(usize::from(first.is_ok())+usize::from(second.is_ok()),1);
        let (success,failure)=if first.is_ok() {(first,second)} else {(second,first)};
        assert_eq!(success.unwrap(),"mod");assert_eq!(sql_code(&failure.unwrap_err()).as_deref(),Some("28000"));
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM content.posts WHERE id=$1").bind(post.id).fetch_one(&case.owner).await.unwrap(),1);
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM content.moderation_audit WHERE target_id=$1 AND action='staff-post'").bind(post.id).fetch_one(&case.owner).await.unwrap(),1);
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM post_secrets.staff_post_intents WHERE token_hash=$1").bind(&post.ticket).fetch_one(&case.owner).await.unwrap(),0);
    }).await;
    fixture.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn authorization_expiring_during_the_actual_session_lock_wait_denies_the_post() {
    let fixture = Fixture::new().await;
    let case = fixture.clone();
    let result=tokio::spawn(async move {
        assert_eq!(case.submit(0,"",&case.csrf,"http://localhost:3001").await.0,StatusCode::SEE_OTHER);
        let (thread,_,_)=case.latest().await;
        let post=BoundPost::new(&case,thread).await;post.issue(&case).await.unwrap();
        let expires:chrono::DateTime<chrono::Utc>=sqlx::query_scalar("UPDATE staff_identity.sessions SET expires_at=clock_timestamp()+interval '1 second' WHERE token_hash=$1 RETURNING expires_at")
            .bind(auth::hash(&case.token)).fetch_one(&case.owner).await.unwrap();
        let mut blocker=case.owner.begin().await.unwrap();
        sqlx::query("SELECT token_hash FROM staff_identity.sessions WHERE token_hash=$1 FOR UPDATE")
            .bind(auth::hash(&case.token)).execute(&mut *blocker).await.unwrap();
        let locker:i32=sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *blocker).await.unwrap();
        let child_case=case.clone();let child_post=post.clone();
        let child=tokio::spawn(async move {child_post.insert(&child_case).await});
        let mut observed=false;
        for _ in 0..150 {
            observed=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE usename='board_staff' AND $1=ANY(pg_blocking_pids(pid)))")
                .bind(locker).fetch_one(&mut *blocker).await.unwrap();
            if observed {break;}tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let mut expired=false;
        for _ in 0..300 {
            expired=sqlx::query_scalar("SELECT clock_timestamp()>=$1").bind(expires).fetch_one(&mut *blocker).await.unwrap();
            if expired {break;}tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let observed_clock:chrono::DateTime<chrono::Utc>=sqlx::query_scalar("SELECT clock_timestamp()").fetch_one(&mut *blocker).await.unwrap();
        blocker.commit().await.unwrap();
        let outcome=child.await.unwrap();
        assert!(observed,"Actual staff consumer must reach the healthy session lock before its deadline");
        assert!(expired,"Database clock must pass the absolute session deadline before release: deadline={expires}, observed={observed_clock}");
        assert_eq!(sql_code(&outcome.unwrap_err()).as_deref(),Some("28000"));
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM content.posts WHERE id=$1").bind(post.id).fetch_one(&case.owner).await.unwrap(),0);
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM content.moderation_audit WHERE target_id=$1 AND action='staff-post'").bind(post.id).fetch_one(&case.owner).await.unwrap(),0);
    }).await;
    fixture.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn concurrent_authority_issuance_enforces_the_account_bound_and_cleans_expired_intents() {
    let fixture = Fixture::new().await;
    let case = fixture.clone();
    let result=tokio::spawn(async move {
        assert_eq!(case.submit(0,"",&case.csrf,"http://localhost:3001").await.0,StatusCode::SEE_OTHER);
        let (thread,_,_)=case.latest().await;
        let mut tasks=tokio::task::JoinSet::new();
        for _ in 0..34 {let case=case.clone();tasks.spawn(async move {
            let post=BoundPost::new(&case,thread).await;post.issue(&case).await
        });}
        let mut accepted=0;let mut denied=0;
        while let Some(result)=tasks.join_next().await {match result.unwrap() {
            Ok(())=>accepted+=1,Err(error)=>{assert_eq!(sql_code(&error).as_deref(),Some("54000"));denied+=1;}
        }}
        assert_eq!((accepted,denied),(32,2));
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM post_secrets.staff_post_intents WHERE account_id=$1").bind(case.account).fetch_one(&case.owner).await.unwrap(),32);
        sqlx::query("UPDATE post_secrets.staff_post_intents SET expires_at=clock_timestamp()-interval '1 second' WHERE account_id=$1").bind(case.account).execute(&case.owner).await.unwrap();
        let healthy=BoundPost::new(&case,thread).await;healthy.issue(&case).await.unwrap();
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM post_secrets.staff_post_intents WHERE account_id=$1").bind(case.account).fetch_one(&case.owner).await.unwrap(),1);
        assert_eq!(healthy.insert(&case).await.unwrap(),"mod");
    }).await;
    fixture.cleanup().await;
    result.unwrap();
}

async fn public_text(router: &Router, path: &str) -> String {
    let response = router
        .clone()
        .oneshot(Request::get(path).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK, "{path}");
    String::from_utf8(
        to_bytes(response.into_body(), 4_194_304)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap()
}
async fn public_json(router: &Router, path: &str) -> serde_json::Value {
    serde_json::from_str(&public_text(router, path).await).unwrap()
}

#[tokio::test]
async fn operator_revocation_can_delete_the_session_while_posting_waits_for_account_authority() {
    revocation_lock_case(true).await;
    revocation_lock_case(false).await;
}
async fn revocation_lock_case(consume: bool) {
    let fixture = Fixture::new().await;
    let case = fixture.clone();
    let result=tokio::spawn(async move {
        assert_eq!(case.submit(0,"",&case.csrf,"http://localhost:3001").await.0,StatusCode::SEE_OTHER);
        let (thread,_,_)=case.latest().await;
        let post=BoundPost::new(&case,thread).await;
        if consume {post.issue(&case).await.unwrap();}
        let mut operator=case.owner.begin().await.unwrap();
        sqlx::query("SELECT id FROM staff_identity.accounts WHERE id=$1 FOR UPDATE").bind(case.account).execute(&mut *operator).await.unwrap();
        let locker:i32=sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *operator).await.unwrap();
        let child_case=case.clone();let child_post=post.clone();
        let child=tokio::spawn(async move {
            if consume {child_post.insert(&child_case).await} else {child_post.issue(&child_case).await.map(|()|"issued".to_owned())}
        });
        let mut observed=false;
        for _ in 0..150 {
            observed=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE usename=$2 AND $1=ANY(pg_blocking_pids(pid)))")
                .bind(locker).bind(if consume {"board_staff"} else {"board_auth"}).fetch_one(&mut *operator).await.unwrap();
            if observed {break;}tokio::time::sleep(Duration::from_millis(10)).await;
        }
        sqlx::query("SET LOCAL lock_timeout='250ms'").execute(&mut *operator).await.unwrap();
        let deletion=sqlx::query("DELETE FROM staff_identity.sessions WHERE account_id=$1").bind(case.account).execute(&mut *operator).await;
        if deletion.is_ok() {operator.commit().await.unwrap();} else {operator.rollback().await.unwrap();}
        let outcome=child.await.unwrap();
        assert!(observed,"Actual consumer must wait for the operator's account lock");
        assert!(deletion.is_ok(),"Revocation must not wait on posting proof or session locks (consume={consume}): {:?}",deletion.err().as_ref().and_then(sql_code));
        assert_eq!(sql_code(&outcome.unwrap_err()).as_deref(),Some("28000"));
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM content.posts WHERE id=$1").bind(post.id).fetch_one(&case.owner).await.unwrap(),0);
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM content.moderation_audit WHERE target_id=$1 AND action='staff-post'").bind(post.id).fetch_one(&case.owner).await.unwrap(),0);
    }).await;
    fixture.cleanup().await;
    result.unwrap();
}

async fn private_request(
    case: &Fixture,
    path: &str,
    form: Option<String>,
) -> axum::response::Response {
    let mut request = Request::builder().uri(path).header(
        "cookie",
        format!("staff={}; staff-csrf={}", case.token, case.csrf),
    );
    let body = if let Some(form) = form {
        request = request
            .method("POST")
            .header("content-type", "application/x-www-form-urlencoded")
            .header("origin", "http://localhost:3001")
            .header("sec-fetch-site", "same-origin");
        Body::from(form)
    } else {
        Body::empty()
    };
    let response = board_staff::router(case.state.clone())
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    assert!(
        response
            .headers()
            .get("access-control-allow-origin")
            .is_none()
    );
    response
}

fn private_form(case: &Fixture, thread: i64, comment: &str) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .append_pair("csrf", &case.csrf)
        .append_pair("mode", "regist")
        .append_pair("resto", &thread.to_string())
        .append_pair("name", "Forged Admin <name>")
        .append_pair("email", "fortune")
        .append_pair("sub", "Owned <script> subject")
        .append_pair("com", comment)
        .finish()
}

async fn private_html(case: &Fixture, path: &str) -> String {
    let response = private_request(case, path, None).await;
    let status = response.status();
    let text = String::from_utf8(
        to_bytes(response.into_body(), 4_194_304)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert_eq!(status, StatusCode::OK, "{text}");
    text
}

#[tokio::test]
async fn private_discussion_forces_anonymous_roles_and_keeps_identity_off_public_routes() {
    let fixture = Fixture::new().await;
    let case = fixture.clone();
    let result = tokio::spawn(async move {
        let username = format!("owned_private_{}",case.account);
        sqlx::query("UPDATE staff_identity.accounts SET role='janitor',username=$2,allow_boards=ARRAY['janitor'] WHERE id=$1")
            .bind(case.account).bind(&username).execute(&case.owner).await.unwrap();
        for path in ["/j/","/j/index.php","/j/0.php","/j/thread/1","/j/res/1.php","/j/post/1"] {
            let response = board_staff::router(case.state.clone()).oneshot(Request::get(path).body(Body::empty()).unwrap()).await.unwrap();
            assert_eq!(response.status(),StatusCode::UNAUTHORIZED,"{path}");
        }
        for path in ["/j/","/j/index.php","/j/0.php"] {
            let text = private_html(&case,path).await;
            assert!(text.contains("Janitor &#38; Moderator Discussion"));
            assert!(text.contains("name=\"csrf\""));
            assert!(text.contains("type=\"hidden\" name=\"name\""));
            assert!(text.contains("type=\"hidden\" name=\"email\""));
            assert!(!text.contains("<label for=\"name\""));
        }
        let single_auth=sqlx::postgres::PgPoolOptions::new().max_connections(1)
            .acquire_timeout(Duration::from_secs(1))
            .connect(&std::env::var("AUTH_DATABASE_URL").unwrap()).await.unwrap();
        let single_state=Arc::new(AppState {
            config:case.state.config.clone(),auth:single_auth.clone(),staff:case.state.staff.clone(),
            webauthn:WebauthnBuilder::new("localhost",&Url::parse("http://localhost:3001").unwrap()).unwrap().build().unwrap(),
            limits:Limits::default(),
        });
        let one_connection=board_staff::router(single_state).oneshot(Request::get("/j/")
            .header("cookie",format!("staff={}; staff-csrf={}",case.token,case.csrf))
            .body(Body::empty()).unwrap()).await.unwrap();
        assert_eq!(one_connection.status(),StatusCode::OK,"A private page must not hold one auth connection while waiting for another");
        drop(one_connection);
        single_auth.close().await;
        let response = private_request(&case,"/j/imgboard.php",Some(private_form(&case,0,"Owned private <script> body"))).await;
        assert_eq!(response.status(),StatusCode::SEE_OTHER);
        let location = response.headers()["location"].to_str().unwrap();
        let thread: i64 = location.strip_prefix("/j/thread/").unwrap().split('#').next().unwrap().parse().unwrap();
        let saved: (String,Option<String>,Option<String>) = sqlx::query_as("SELECT name,capcode,trip FROM content.posts WHERE id=$1")
            .bind(thread).fetch_one(&case.owner).await.unwrap();
        assert_eq!(saved,("Anonymous".into(),None,None));
        let actor:i64 = sqlx::query_scalar("SELECT account_id FROM staff_identity.discussion_posts WHERE post_id=$1")
            .bind(thread).fetch_one(&case.state.auth).await.unwrap();
        assert_eq!(actor,case.account);
        let text = private_html(&case,&format!("/j/thread/{thread}")).await;
        assert!(text.contains("Anonymous ## Janitor"));
        assert!(text.contains("discussionJanitor"));
        assert!(!text.contains("Forged Admin"));
        assert!(!text.contains(&username));
        assert!(!text.contains("<script>"));
        assert!(text.contains("&#60;script&#62;"));
        assert_eq!(case.submit(0,"",&case.csrf,"http://localhost:3001").await.0,StatusCode::FORBIDDEN);

        let thread_path = format!("/j/thread/{thread}");
        for (role,label,class) in [("moderator","Mod","discussionModerator"),("manager","Manager","discussionManager"),("admin","Admin","discussionAdmin")] {
            sqlx::query("UPDATE staff_identity.accounts SET role=$2 WHERE id=$1")
                .bind(case.account).bind(role).execute(&case.owner).await.unwrap();
            let response=private_request(&case,"/j/imgboard.php",Some(private_form(&case,thread,&format!("Reply by source role {role}. >>{thread} >>>/g/123")))).await;
            assert_eq!(response.status(),StatusCode::SEE_OTHER);
            let text=private_html(&case,&thread_path).await;
            assert!(text.contains(&format!("Anonymous ## {label}")));
            assert!(text.contains(class));
            assert!(!text.contains("## Janitor"),"Source labels follow the current author role");
            assert!(!text.contains(&username));
            assert!(text.contains(&format!("href=\"/j/post/{thread}\"")));
            assert!(text.contains("href=\"http://127.0.0.1:3000/g/post/123\""));
        }
        let legacy=private_html(&case,&format!("/j/res/{thread}.php")).await;
        assert!(legacy.contains("Anonymous ## Admin"));
        let quote=private_html(&case,&format!("{thread_path}?quote={thread}")).await;
        assert!(quote.contains(&format!("&#62;&#62;{thread}\n</textarea>")));
        let redirected=private_request(&case,&format!("/j/post/{thread}"),None).await;
        assert_eq!(redirected.status(),StatusCode::SEE_OTHER);
        assert_eq!(redirected.headers()["location"],format!("{thread_path}#p{thread}"));
        for app in [&case.public,&case.api] {
            for path in [format!("/j/thread/{thread}.json"),"/j/1.json".into(),"/j/catalog.json".into(),"/j/index.rss".into()] {
                let response=app.clone().oneshot(Request::get(&path).header("cookie",format!("staff={}",case.token)).body(Body::empty()).unwrap()).await.unwrap();
                assert_eq!(response.status(),StatusCode::NOT_FOUND,"{path}");
            }
        }
        let public=pool("TEST_PUBLIC_DATABASE_URL").await;
        let count:i64=sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE thread_id=$1")
            .bind(thread).fetch_one(&public).await.unwrap();
        assert_eq!(count,0);
        denied(&public,"SELECT * FROM staff_identity.discussion_posts LIMIT 0").await;
        denied(&case.state.staff,"SELECT * FROM staff_identity.discussion_posts LIMIT 0").await;
        denied(&case.state.auth,"INSERT INTO staff_identity.discussion_posts(post_id,account_id) SELECT 1,1 WHERE false").await;
        public.close().await;
        let privacy=sqlx::query("UPDATE content.boards SET staff_only=false WHERE slug='j'").execute(&case.owner).await.unwrap_err();
        assert_eq!(sql_code(&privacy).as_deref(),Some("23514"));

        let before:i64=sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE thread_id=$1").bind(thread).fetch_one(&case.owner).await.unwrap();
        let invalid=private_request(&case,"/j/imgboard.php",Some(private_form(&case,thread,""))).await;
        assert_eq!(invalid.status(),StatusCode::BAD_REQUEST);
        let invalid_text=String::from_utf8(to_bytes(invalid.into_body(),4_194_304).await.unwrap().to_vec()).unwrap();
        assert!(invalid_text.contains("Owned &#60;script&#62; subject"));
        assert!(invalid_text.contains("role=\"alert\""));
        assert!(!invalid_text.contains("<script>"));
        let bad_csrf=private_form(&case,thread,"Rejected CSRF").replace(&case.csrf,"invalid");
        assert_eq!(private_request(&case,"/j/imgboard.php",Some(bad_csrf)).await.status(),StatusCode::FORBIDDEN);
        let forged=board_store::create_staff_post(&case.state.staff,"j",thread,
            &board_store::NewPost{name:"Forged identity".into(),subject:"Owned".into(),comment:"Must fail".into(),deletion_hash:String::new(),sage:false},chrono::Utc::now(),
            board_store::StaffPostAuthority{auth_pool:&case.state.auth,session_hash:&auth::hash(&case.token),csrf_hash:&auth::hash(&case.csrf),ticket_hash:&auth::hash(&auth::token()).try_into().unwrap(),idle_seconds:900,highlight:false}).await;
        assert!(matches!(forged,Err(board_store::StoreError::AuthorizationChanged)));
        sqlx::query("UPDATE staff_identity.accounts SET deny_boards=ARRAY['j'] WHERE id=$1").bind(case.account).execute(&case.owner).await.unwrap();
        let queue=private_html(&case,"/reports").await;
        assert!(!queue.contains("href=\"/j/\""));
        for path in ["/j/","/latest.php","/j/latest.php",thread_path.as_str()] {
            assert_eq!(private_request(&case,path,None).await.status(),StatusCode::FORBIDDEN);
        }
        assert_eq!(private_request(&case,"/j/imgboard.php",Some(private_form(&case,thread,"Denied board"))).await.status(),StatusCode::FORBIDDEN);
        sqlx::query("UPDATE staff_identity.accounts SET deny_boards=ARRAY[]::text[] WHERE id=$1").bind(case.account).execute(&case.owner).await.unwrap();
        sqlx::query("UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp()-interval '11 minutes' WHERE account_id=$1").bind(case.account).execute(&case.owner).await.unwrap();
        assert!(private_html(&case,&thread_path).await.contains("before posting"));
        assert_eq!(private_request(&case,"/j/imgboard.php",Some(private_form(&case,thread,"Stale auth"))).await.status(),StatusCode::FORBIDDEN);
        sqlx::query("UPDATE staff_identity.sessions SET expires_at=clock_timestamp()-interval '1 second' WHERE account_id=$1").bind(case.account).execute(&case.owner).await.unwrap();
        assert_eq!(private_request(&case,&thread_path,None).await.status(),StatusCode::UNAUTHORIZED);
        let after:i64=sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE thread_id=$1").bind(thread).fetch_one(&case.owner).await.unwrap();
        assert_eq!(before,after);
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM content.moderation_audit WHERE board='j' AND account_id=$1")
            .bind(case.account).fetch_one(&case.owner).await.unwrap(),before);
    }).await;
    fixture.cleanup().await;
    result.unwrap();
}
