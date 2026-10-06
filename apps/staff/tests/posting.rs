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

#[tokio::test]
async fn ordinary_staff_authority_binds_all_metadata_body_rank_and_one_use() {
    let fixture = ordinary_fixture().await;
    let case = fixture.clone();
    let result=tokio::spawn(async move {
        sqlx::query("UPDATE content.boards SET op_markup=false WHERE slug=$1").bind(&case.board).execute(&case.owner).await.unwrap();
        for role in ["janitor","moderator","manager","admin"] {
            sqlx::query("UPDATE staff_identity.accounts SET role=$2 WHERE id=$1").bind(case.account).bind(role).execute(&case.owner).await.unwrap();
            let proof=OrdinaryProof::issue(&case,role!="janitor").await;
            assert_eq!(proof.context.as_object().unwrap().len(),14);
            for key in proof.context.as_object().unwrap().keys() {
                let mut forged=proof.context.clone();forged[key]=serde_json::json!(format!("{}forged",forged[key].as_str().unwrap()));
                proof.denied(&case,&forged,"Owned ordinary proof body").await;
            }
            proof.denied(&case,&proof.context,"Forged ordinary proof body").await;
            let mut tx=case.state.staff.begin().await.unwrap();proof.context(&mut tx,&proof.context).await;
            proof.insert(&mut tx,&case.board,"Owned ordinary proof body").await.unwrap();tx.commit().await.unwrap();
            let saved:(Option<String>,String,String,String)=sqlx::query_as("SELECT p.capcode,p.poster_id,d.password_hash,o.peer::text FROM content.posts p JOIN post_secrets.deletion d ON d.post_id=p.id JOIN post_secrets.op_peers o ON o.thread_id=p.id WHERE p.id=$1")
                .bind(proof.id).fetch_one(&case.owner).await.unwrap();
            assert_eq!(saved.0,None);assert_eq!(saved.1,proof.context["poster_id"]);assert_eq!(saved.2,"owned-derived-hash");assert_eq!(saved.3,"81.2.69.142/32");
            let mut tx=case.state.staff.begin().await.unwrap();proof.context(&mut tx,&proof.context).await;
            let error=proof.insert(&mut tx,&case.board,"Owned ordinary proof body").await.unwrap_err();
            assert_eq!(error.as_database_error().and_then(|error|error.code()).as_deref(),Some("28000"));tx.rollback().await.unwrap();
            let intents:i64=sqlx::query_scalar("SELECT count(*) FROM post_secrets.staff_post_intents WHERE account_id=$1").bind(case.account).fetch_one(&case.owner).await.unwrap();assert_eq!(intents,0);
        }
    }).await;
    fixture.cleanup().await;
    result.unwrap();
}

struct OrdinaryProof {
    id: i64,
    stamp: chrono::DateTime<chrono::Utc>,
    ticket: Vec<u8>,
    context: serde_json::Value,
}

#[tokio::test]
async fn ordinary_staff_consumer_rechecks_expiry_after_account_and_intent_lock_waits() {
    let fixture = ordinary_fixture().await;
    let case = fixture.clone();
    let result=tokio::spawn(async move {
        sqlx::query("UPDATE content.boards SET op_markup=false WHERE slug=$1").bind(&case.board).execute(&case.owner).await.unwrap();
        for kind in ["account","intent","session","recent"] {
            sqlx::query("UPDATE staff_identity.sessions SET expires_at=clock_timestamp()+interval '1 day',authenticated_at=clock_timestamp(),last_activity_at=clock_timestamp() WHERE account_id=$1")
                .bind(case.account).execute(&case.owner).await.unwrap();
            let proof=OrdinaryProof::issue(&case,true).await;
            let mut tx=case.state.staff.begin().await.unwrap();proof.context(&mut tx,&proof.context).await;
            let pid:i32=sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *tx).await.unwrap();
            match kind {
                "session"=>{sqlx::query("UPDATE staff_identity.sessions SET expires_at=clock_timestamp()+interval '1 second' WHERE account_id=$1").bind(case.account).execute(&case.owner).await.unwrap();}
                "recent"=>{sqlx::query("UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp()-interval '10 minutes'+interval '1 second' WHERE account_id=$1").bind(case.account).execute(&case.owner).await.unwrap();}
                _=>{sqlx::query("UPDATE post_secrets.staff_post_intents SET expires_at=clock_timestamp()+interval '1 second' WHERE token_hash=$1").bind(&proof.ticket).execute(&case.owner).await.unwrap();}
            }
            let deadline:chrono::DateTime<chrono::Utc>=sqlx::query_scalar("SELECT CASE $2 WHEN 'session' THEN s.expires_at WHEN 'recent' THEN s.authenticated_at+interval '10 minutes' ELSE i.expires_at END FROM post_secrets.staff_post_intents i JOIN staff_identity.sessions s ON s.token_hash=i.session_hash WHERE i.token_hash=$1")
                .bind(&proof.ticket).bind(kind).fetch_one(&case.owner).await.unwrap();
            let mut blocker=case.owner.begin().await.unwrap();
            let blocker_pid:i32=sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *blocker).await.unwrap();
            if kind=="account" {
                sqlx::query("SELECT id FROM staff_identity.accounts WHERE id=$1 FOR UPDATE").bind(case.account).fetch_one(&mut *blocker).await.unwrap();
            } else {
                sqlx::query("SELECT token_hash FROM post_secrets.staff_post_intents WHERE token_hash=$1 FOR UPDATE").bind(&proof.ticket).fetch_one(&mut *blocker).await.unwrap();
            }
            let child_board=case.board.clone();
            let insert=tokio::spawn(async move {
                let error=proof.insert(&mut tx,&child_board,"Owned ordinary proof body").await.unwrap_err();
                let code=error.as_database_error().and_then(|error|error.code()).map(|code|code.into_owned());
                tx.rollback().await.unwrap();code
            });
            tokio::time::timeout(Duration::from_secs(2),async {
                loop {
                    let waiting:bool=sqlx::query_scalar("SELECT $2=ANY(pg_blocking_pids($1))")
                        .bind(pid).bind(blocker_pid).fetch_one(&case.owner).await.unwrap();
                    if waiting {break;}tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }).await.unwrap();
            tokio::time::timeout(Duration::from_secs(3),async {
                loop {
                    let expired:bool=sqlx::query_scalar("SELECT clock_timestamp()>=$1").bind(deadline).fetch_one(&case.owner).await.unwrap();
                    if expired {break;}tokio::time::sleep(Duration::from_millis(10)).await;
                }
            }).await.unwrap();
            blocker.commit().await.unwrap();
            assert_eq!(tokio::time::timeout(Duration::from_secs(2),insert).await.unwrap().unwrap().as_deref(),Some("28000"));
            let rows:(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM content.posts WHERE board=$1),(SELECT count(*) FROM content.moderation_audit WHERE board=$1)").bind(&case.board).fetch_one(&case.owner).await.unwrap();assert_eq!(rows,(0,0));
        }
    }).await;
    fixture.cleanup().await;
    result.unwrap();
}

impl OrdinaryProof {
    async fn issue(case: &Fixture, authorized: bool) -> Self {
        let id: i64 = sqlx::query_scalar("SELECT nextval('content.post_number')")
            .fetch_one(&case.owner)
            .await
            .unwrap();
        let stamp = chrono::Utc::now().with_nanosecond(0).unwrap();
        sqlx::query(
            "INSERT INTO content.threads(id,board,created_at,modified_at) VALUES($1,$2,$3,$3)",
        )
        .bind(id)
        .bind(&case.board)
        .bind(stamp)
        .execute(&case.owner)
        .await
        .unwrap();
        let key = case.state.config.poster_id_key.as_deref().unwrap();
        let peer = "81.2.69.142".parse().unwrap();
        let count = key.count_context(&case.board, id, peer).unwrap();
        let context = serde_json::json!({"poster_id":key.label(&case.board,id,peer).unwrap(),"poster_fingerprint":count.fingerprint,"poster_epoch":count.epoch,
            "post_sage":"false","country":"GB","country_name":"United Kingdom","flag":"","source_op_reply":"false",
            "dice_result":"","fortune_text":"","fortune_color":"","peer":"81.2.69.142","deletion_hash":"owned-derived-hash","op_password_proof":""});
        let ticket = auth::hash(&auth::token());
        let limit:i32=sqlx::query_scalar("SELECT CASE WHEN $2 THEN max_authorized_comment_chars ELSE max_comment_chars END FROM content.boards WHERE slug=$1").bind(&case.board).bind(authorized).fetch_one(&case.owner).await.unwrap();
        sqlx::query("SELECT staff_identity.issue_ordinary_post_authority($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19)")
            .bind(&ticket).bind(auth::hash(&case.token)).bind(auth::hash(&case.csrf)).bind(900i32).bind(id).bind(&case.board).bind(id)
            .bind("Named ordinary staff").bind("Owned subject").bind("Owned ordinary proof body").bind(stamp).bind(authorized).bind(limit)
            .bind(None::<Vec<u8>>).bind(None::<String>).bind("bypass_r9k").bind(None::<String>).bind(true).bind(&context)
            .execute(&case.state.auth).await.unwrap();
        Self {
            id,
            stamp,
            ticket,
            context,
        }
    }
    async fn context(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        context: &serde_json::Value,
    ) {
        let ticket = self
            .ticket
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        sqlx::query("SELECT set_config('board.staff_post_ticket',$1,true),set_config('board.staff_post_options','bypass_r9k',true),set_config('board.post_trip','',true),set_config('board.wordfilter_payload','',true),set_config('board.wordfilter_search','',true)")
            .bind(ticket).execute(&mut **tx).await.unwrap();
        for (key, value) in context.as_object().unwrap() {
            sqlx::query("SELECT set_config($1,$2,true)")
                .bind(format!("board.{key}"))
                .bind(value.as_str().unwrap())
                .execute(&mut **tx)
                .await
                .unwrap();
        }
    }
    async fn insert(
        &self,
        tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
        board: &str,
        body: &str,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment,created_at) VALUES($1,$2,$1,'Named ordinary staff','Owned subject',$3,$4)")
            .bind(self.id).bind(board).bind(body).bind(self.stamp).execute(&mut **tx).await?;
        Ok(())
    }
    async fn denied(&self, case: &Fixture, context: &serde_json::Value, body: &str) {
        let mut tx = case.state.staff.begin().await.unwrap();
        self.context(&mut tx, context).await;
        let error = self.insert(&mut tx, &case.board, body).await.unwrap_err();
        assert_eq!(
            error
                .as_database_error()
                .and_then(|error| error.code())
                .as_deref(),
            Some("28000")
        );
        tx.rollback().await.unwrap();
        let remaining: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM post_secrets.staff_post_intents WHERE token_hash=$1",
        )
        .bind(&self.ticket)
        .fetch_one(&case.owner)
        .await
        .unwrap();
        assert_eq!(remaining, 1);
    }
}

#[tokio::test]
async fn ordinary_staff_private_tables_and_owner_helpers_remain_inaccessible() {
    for role in [
        "TEST_PUBLIC_DATABASE_URL",
        "STAFF_DATABASE_URL",
        "AUTH_DATABASE_URL",
    ] {
        let runtime = pool(role).await;
        for query in [
            "SELECT * FROM post_secrets.staff_post_intents LIMIT 0",
            "SELECT * FROM post_secrets.poster_contexts LIMIT 0",
            "SELECT * FROM post_secrets.robot9000_texts LIMIT 0",
            "SELECT content.staff_ordinary_policy('g')",
            "SELECT content.check_staff_op_context('g',1,2,'{}')",
            "SELECT content.consume_badged_post_authority(NULL,1,'g',1,'','','',clock_timestamp())",
        ] {
            let error = sqlx::query(query).execute(&runtime).await.unwrap_err();
            assert_eq!(
                error
                    .as_database_error()
                    .and_then(|error| error.code())
                    .as_deref(),
                Some("42501")
            );
        }
        if role != "TEST_PUBLIC_DATABASE_URL" {
            for query in [
                "SELECT * FROM post_secrets.op_peers LIMIT 0",
                "SELECT * FROM post_secrets.op_replies LIMIT 0",
                "SELECT * FROM post_secrets.deletion LIMIT 0",
            ] {
                let error = sqlx::query(query).execute(&runtime).await.unwrap_err();
                assert_eq!(
                    error
                        .as_database_error()
                        .and_then(|error| error.code())
                        .as_deref(),
                    Some("42501")
                );
            }
        }
        if role != "STAFF_DATABASE_URL" {
            for query in [
                "SELECT content.staff_ordinary_context()",
                "SELECT content.staff_op_context('g',1,'192.0.2.1')",
                "SELECT content.staff_op_deletion_hash('g',1)",
            ] {
                let error = sqlx::query(query).execute(&runtime).await.unwrap_err();
                assert_eq!(
                    error
                        .as_database_error()
                        .and_then(|error| error.code())
                        .as_deref(),
                    Some("42501")
                );
            }
        }
    }
    let owner = pool("MIGRATION_DATABASE_URL").await;
    let ordinary:bool=sqlx::query_scalar("SELECT NOT rolcanlogin AND NOT rolsuper AND NOT rolcreatedb AND NOT rolcreaterole AND NOT rolbypassrls FROM pg_roles WHERE rolname='board_staff_post_owner'")
        .fetch_one(&owner).await.unwrap();
    assert!(ordinary);
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn ordinary_staff_verified_unix_peer_drives_persisted_identity() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let _serial = ORDINARY_TESTS.lock().await;
    let fixture = ordinary_fixture().await;
    let case = fixture.clone();
    let result=tokio::spawn(async move {
        let (peer,_)=tokio::net::UnixStream::pair().unwrap();let uid=peer.peer_cred().unwrap().uid();
        for expected in [uid,uid.wrapping_add(1)] {
            let directory=tempfile::tempdir().unwrap();let socket=directory.path().join("staff.sock");
            let mut config=case.state.config.clone();
            config.proxy=board_config::PublicProxy::from_values(Some(socket.to_str().unwrap()),Some(&expected.to_string()),false).unwrap();
            let listener=board_http::transport::HttpListener::bind(config.bind,config.proxy.as_ref()).await.unwrap();
            let state=Arc::new(AppState {config,auth:case.state.auth.clone(),staff:case.state.staff.clone(),webauthn:case.state.webauthn.clone(),limits:Limits::default()});
            let (stop,stopped)=tokio::sync::watch::channel(false);
            let server=tokio::spawn(listener.serve(board_staff::router(state),stopped,
                board_http::transport::ConnectionBudget::new(board_config::PublicRequestLimits::default())));
            for (headers,status) in [("X-Board-Client-IP: ::ffff:81.2.69.142\r\nX-Forwarded-For: 2001:218::\r\n",303),("",400),
                ("X-Board-Client-IP: 81.2.69.142\r\nX-Board-Client-IP: 2001:218::\r\n",400)] {
                let before:i64=sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1").bind(&case.board).fetch_one(&case.owner).await.unwrap();
                let form=ordinary_form(&case,0,"","","Owned actual staff Unix peer");
                let request=format!("POST /post HTTP/1.1\r\nHost: localhost\r\nOrigin: http://localhost:3001\r\nSec-Fetch-Site: same-origin\r\nCookie: staff={}\r\nContent-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{form}",case.token,form.len());
                let mut client=tokio::net::UnixStream::connect(&socket).await.unwrap();client.write_all(request.as_bytes()).await.unwrap();
                let mut response=String::new();tokio::time::timeout(Duration::from_secs(5),client.read_to_string(&mut response)).await.unwrap().unwrap();
                let expected_status=if expected==uid {status} else {403};
                assert!(response.starts_with(&format!("HTTP/1.1 {expected_status}")),"Unexpected ordinary staff listener status.");
                let after:i64=sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1").bind(&case.board).fetch_one(&case.owner).await.unwrap();
                assert_eq!(after,before+if expected_status==303 {1} else {0});
            }
            stop.send(true).unwrap();tokio::time::timeout(Duration::from_secs(2),server).await.unwrap().unwrap().unwrap();assert!(!socket.exists());
        }
        let (id,_,capcode)=case.latest().await;assert_eq!(capcode,None);
        let saved:(String,String,String)=sqlx::query_as("SELECT p.poster_id,p.country,o.peer::text FROM content.posts p JOIN post_secrets.op_peers o ON o.thread_id=p.id WHERE p.id=$1")
            .bind(id).fetch_one(&case.owner).await.unwrap();
        assert_eq!(saved.0,case.state.config.poster_id_key.as_deref().unwrap().label(&case.board,id,"81.2.69.142".parse().unwrap()).unwrap());
        assert_eq!(saved.1,"GB");assert_eq!(saved.2,"81.2.69.142/32");
    }).await;
    fixture.cleanup().await;
    result.unwrap();
}

static ORDINARY_TESTS: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn ordinary_fixture() -> Arc<Fixture> {
    let mut case = Arc::try_unwrap(Fixture::new().await).ok().unwrap();
    let state = Arc::get_mut(&mut case.state).unwrap();
    state.config.poster_id_key = Some(Arc::new(
        board_domain::poster_id::PosterIdKey::parse(&"22".repeat(32)).unwrap(),
    ));
    state.config.country_database = Some(Arc::new(
        board_domain::country::CountryDatabase::from_bytes(
            include_bytes!("../../../crates/domain/tests/fixtures/GeoIP2-Country-Test.mmdb")
                .to_vec(),
        )
        .unwrap(),
    ));
    sqlx::query("UPDATE content.boards SET text_only=true,user_ids=true,country_flags=true,board_flags=ARRAY['AC'],op_markup=true WHERE slug=$1")
        .bind(&case.board).execute(&case.owner).await.unwrap();
    Arc::new(case)
}

fn ordinary_form(case: &Fixture, parent: i64, options: &str, flag: &str, comment: &str) -> String {
    url::form_urlencoded::Serializer::new(
        staff_form(case, parent, "Owned#password", "Owned", comment) + "&",
    )
    .append_pair("badge", "none")
    .append_pair("options", options)
    .append_pair("flag", flag)
    .append_pair("password", "owned-ordinary-password")
    .finish()
}

async fn ordinary_request(
    case: &Fixture,
    form: String,
    peer: Option<&str>,
) -> axum::response::Response {
    let mut request = Request::post("/post")
        .header("content-type", "application/x-www-form-urlencoded")
        .header("origin", &case.state.config.origin)
        .header("sec-fetch-site", "same-origin")
        .header("cookie", format!("staff={}", case.token))
        .body(Body::from(form))
        .unwrap();
    if let Some(peer) = peer {
        request
            .extensions_mut()
            .insert(axum::extract::ConnectInfo(std::net::SocketAddr::new(
                peer.parse().unwrap(),
                54321,
            )));
    }
    board_staff::router(case.state.clone())
        .oneshot(request)
        .await
        .unwrap()
}

#[tokio::test]
async fn ordinary_staff_ranks_persist_public_identity_flags_op_membership_and_deletion() {
    let _serial = ORDINARY_TESTS.lock().await;
    let fixture = ordinary_fixture().await;
    let case = fixture.clone();
    let result=tokio::spawn(async move {
        for role in ["janitor","moderator","manager","admin"] {
            sqlx::query("UPDATE staff_identity.accounts SET role=$2,flags='{}' WHERE id=$1")
                .bind(case.account).bind(role).execute(&case.owner).await.unwrap();
            let html=private_html(&case,"/post").await;
            assert!(html.contains("value=\"none\"") && html.contains("name=\"options\"") && html.contains("name=\"password\""));
            assert_eq!(ordinary_request(&case,ordinary_form(&case,0,"","","Owned ordinary OP [op]markup[/op]"),Some("81.2.69.142")).await.status(),StatusCode::SEE_OTHER);
            let (op,_,capcode)=case.latest().await;
            assert_eq!(capcode,None);
            let key=case.state.config.poster_id_key.as_deref().unwrap();
            let label=key.label(&case.board,op,"81.2.69.142".parse().unwrap()).unwrap();
            let row:serde_json::Value=sqlx::query_scalar("SELECT jsonb_build_object('name',name,'trip',trip,'id',poster_id,'json_id',json_op_poster_id,'country',country,'country_name',country_name,'flag',board_flag,'format',comment_format,'authorized',staff_authorized_limits) FROM content.posts WHERE board=$1 AND id=$2")
                .bind(&case.board).bind(op).fetch_one(&case.owner).await.unwrap();
            assert_eq!(row["name"],"Owned");assert_eq!(row["trip"],"!ozOtJW9BFA");
            assert_eq!(row["id"],label);assert_eq!(row["json_id"],label);
            assert_eq!(row["country"],"GB");assert_eq!(row["country_name"],"United Kingdom");assert!(row["flag"].is_null());
            assert!(row["format"].as_i64().unwrap()&16!=0);assert_eq!(row["authorized"],role!="janitor");
            let api=public_json(&case.api,&format!("/{}/thread/{op}.json",case.board)).await;
            assert_eq!(api["posts"][0]["id"],label);assert_eq!(api["posts"][0]["country"],"GB");assert!(api["posts"][0].get("capcode").is_none());
            let confirmation=private_html(&case,&format!("/post?board={}&thread={op}&posted={op}",case.board)).await;
            assert!(confirmation.contains(&format!("Post {op} created.")));
            assert_eq!(ordinary_request(&case,ordinary_form(&case,op,"SaGe","","Owned same peer sage reply"),Some("::ffff:81.2.69.142")).await.status(),StatusCode::SEE_OTHER);
            let (sage,_,_)=case.latest().await;
            let saved:String=sqlx::query_scalar("SELECT poster_id FROM content.posts WHERE id=$1").bind(sage).fetch_one(&case.owner).await.unwrap();
            assert_eq!(saved,"Heaven");
            let op_reply:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM post_secrets.op_replies WHERE post_id=$1 AND thread_id=$2)").bind(sage).bind(op).fetch_one(&case.owner).await.unwrap();
            assert!(op_reply);
            assert_eq!(ordinary_request(&case,ordinary_form(&case,op,"","AC","Owned other peer board flag reply"),Some("2001:218::")).await.status(),StatusCode::SEE_OTHER);
            let (flagged,_,_)=case.latest().await;
            let row:(Option<String>,Option<String>,Option<String>)=sqlx::query_as("SELECT country,board_flag,poster_id FROM content.posts WHERE id=$1").bind(flagged).fetch_one(&case.owner).await.unwrap();
            assert_eq!(row.0,None);assert_eq!(row.1.as_deref(),Some("AC"));assert_ne!(row.2.as_deref(),Some(label.as_str()));
            let count:Option<i32>=sqlx::query_scalar("SELECT content.unique_posters($1,$2)").bind(&case.board).bind(op).fetch_one(&case.public_pool().await).await.unwrap();
            assert_eq!(count,Some(2));
            let hash=board_store::deletion_hash(&case.public_pool().await,&case.board,flagged).await.unwrap();
            use argon2::PasswordVerifier;
            assert!(argon2::Argon2::default().verify_password(b"owned-ordinary-password",&argon2::PasswordHash::new(&hash).unwrap()).is_ok());
            use sha2::Digest;
            // This test owns the post; keep the source board's public deletion policy.
            sqlx::query("UPDATE content.posts SET created_at=clock_timestamp()-interval '601 seconds' WHERE id=$1")
                .bind(flagged).execute(&case.owner).await.unwrap();
            board_store::delete_with_password_proof(&case.public_pool().await,&case.board,flagged,sha2::Sha256::digest(hash.as_bytes()).into(),false).await.unwrap();
            let counts:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM post_secrets.staff_post_intents WHERE account_id=$1),(SELECT count(*) FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$2 AND thread_id=$3)),(SELECT count(*) FROM content.moderation_audit WHERE account_id=$1 AND board=$2 AND action='staff-post' AND target_id IN(SELECT id FROM content.posts WHERE thread_id=$3))")
                .bind(case.account).bind(&case.board).bind(op).fetch_one(&case.owner).await.unwrap();
            assert_eq!(counts,(0,3,3));
        }
    }).await;
    fixture.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn ordinary_staff_flags_use_the_selected_board_dictionary_and_locked_policy() {
    let _serial = ORDINARY_TESTS.lock().await;
    let fixture = ordinary_fixture().await;
    let case = fixture.clone();
    let result = tokio::spawn(async move {
        let reference: serde_json::Value = serde_json::from_str(include_str!(
            "../../public/tests/fixtures/board-flags.json"
        )).unwrap();
        for (kind, codes, rejected) in [
            ("pol", vec!["AN", "CM"], "TWI"),
            ("mlp", vec!["AN", "TWI"], "CM"),
            ("lgbt", vec!["NB", "TRN"], "AN"),
            ("test", vec!["FL1", "FL2"], "NB"),
        ] {
            sqlx::query("UPDATE content.boards SET board_flag_type=$2,board_flags=$3 WHERE slug=$1")
                .bind(&case.board).bind(kind).bind(&codes).execute(&case.owner).await.unwrap();
            let html = private_html(&case, &format!("/post?board={}", case.board)).await;
            let menu = html.split("<select id=\"staff-flag\"").nth(1).unwrap()
                .split("</select>").next().unwrap();
            let expected = codes.iter().map(|code| format!("<option value=\"{code}\">{}</option>",
                reference["tables"][kind]["selector"][*code].as_str().unwrap())).collect::<String>();
            assert_eq!(menu, format!(" name=\"flag\"><option value=\"\">None</option>{expected}"));
            assert!(html.contains(&format!("data-flag-type=\"{kind}\" data-flags=\"{}\"", codes.join(" "))));

            let before: i64 = sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1")
                .bind(&case.board).fetch_one(&case.owner).await.unwrap();
            assert_eq!(ordinary_request(&case, ordinary_form(&case, 0, "", rejected,
                "Owned cross-type flag rejection"), Some("81.2.69.142")).await.status(), StatusCode::BAD_REQUEST);
            let after: i64 = sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1")
                .bind(&case.board).fetch_one(&case.owner).await.unwrap();
            assert_eq!(after, before);

            let code = codes[0];
            assert_eq!(ordinary_request(&case, ordinary_form(&case, 0, "", code,
                &format!("Owned {kind} flag post")), Some("81.2.69.142")).await.status(), StatusCode::SEE_OTHER);
            let (post, _, capcode) = case.latest().await;
            assert_eq!(capcode, None);
            let saved: (String, String, String) = sqlx::query_as(
                "SELECT board_flag_type,board_flag,flag_name FROM content.posts WHERE board=$1 AND id=$2")
                .bind(&case.board).bind(post).fetch_one(&case.owner).await.unwrap();
            let label = reference["tables"][kind]["display"][code].as_str().unwrap();
            assert_eq!(saved, (kind.to_owned(), code.to_owned(), label.to_owned()));
            let json = public_json(&case.api, &format!("/{}/thread/{post}.json", case.board)).await;
            assert_eq!(json["posts"][0]["board_flag"], code);
            assert_eq!(json["posts"][0]["flag_name"], label);
            assert!(json["posts"][0].get("board_flag_type").is_none());
        }
    }).await;
    fixture.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn ordinary_staff_content_admission_rejects_quietly_and_records_autosage() {
    let _serial = ORDINARY_TESTS.lock().await;
    let fixture = ordinary_fixture().await;
    let case = fixture.clone();
    let result=tokio::spawn(async move {
        let rule:i64=sqlx::query_scalar("INSERT INTO admission.rules(board,pattern) VALUES($1,'ownedordinaryfilter') RETURNING id")
            .bind(&case.board).fetch_one(&case.owner).await.unwrap();
        let response=ordinary_request(&case,ordinary_form(&case,0,"","","ownedordinaryfilter"),Some("192.0.2.117")).await;
        assert_eq!(response.status(),StatusCode::BAD_REQUEST);
        let body=to_bytes(response.into_body(),4096).await.unwrap();assert!(!body.is_empty());
        let counts:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM content.posts WHERE board=$1),(SELECT count(*) FROM admission.logs WHERE board=$1),(SELECT count(*) FROM post_secrets.staff_post_intents WHERE account_id=$2)")
            .bind(&case.board).bind(case.account).fetch_one(&case.owner).await.unwrap();assert_eq!(counts,(0,0,0));
        sqlx::query("UPDATE admission.rules SET log=true WHERE id=$1").bind(rule).execute(&case.owner).await.unwrap();
        assert_eq!(ordinary_request(&case,ordinary_form(&case,0,"","","ownedordinaryfilter"),Some("192.0.2.121")).await.status(),StatusCode::SEE_OTHER);
        let logs:i64=sqlx::query_scalar("SELECT count(*) FROM admission.logs WHERE board=$1").bind(&case.board).fetch_one(&case.owner).await.unwrap();assert_eq!(logs,1);
        sqlx::query("UPDATE admission.rules SET log=false,quiet=true WHERE id=$1").bind(rule).execute(&case.owner).await.unwrap();
        assert_eq!(ordinary_request(&case,ordinary_form(&case,0,"","","ownedordinaryfilter"),Some("192.0.2.118")).await.status(),StatusCode::SEE_OTHER);
        let posts:i64=sqlx::query_scalar("SELECT count(*) FROM content.posts WHERE board=$1").bind(&case.board).fetch_one(&case.owner).await.unwrap();assert_eq!(posts,1);
        sqlx::query("UPDATE admission.rules SET quiet=false,autosage=true WHERE id=$1").bind(rule).execute(&case.owner).await.unwrap();
        assert_eq!(ordinary_request(&case,ordinary_form(&case,0,"","","ownedordinaryfilter"),Some("192.0.2.119")).await.status(),StatusCode::SEE_OTHER);
        let (op,_,_)=case.latest().await;
        let autosage:bool=sqlx::query_scalar("SELECT permasage FROM content.threads WHERE id=$1").bind(op).fetch_one(&case.owner).await.unwrap();assert!(autosage);
    }).await;
    for query in [
        "DELETE FROM admission.rules WHERE board=$1",
        "DELETE FROM admission.logs WHERE board=$1",
        "DELETE FROM admission.hits WHERE board=$1",
        "DELETE FROM admission.bans WHERE board=$1",
    ] {
        sqlx::query(query)
            .bind(&fixture.board)
            .execute(&fixture.owner)
            .await
            .unwrap();
    }
    fixture.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn ordinary_staff_robot9000_rejections_roll_back_posts_and_discard_proofs() {
    let _serial = ORDINARY_TESTS.lock().await;
    let fixture = ordinary_fixture().await;
    let case = fixture.clone();
    let result=tokio::spawn(async move {
        sqlx::query("UPDATE content.boards SET robot9000=true WHERE slug=$1").bind(&case.board).execute(&case.owner).await.unwrap();
        let comment="Owned ordinary Robot9000 sufficiently distinct original sentence";
        assert_eq!(ordinary_request(&case,ordinary_form(&case,0,"","",comment),Some("192.0.2.120")).await.status(),StatusCode::SEE_OTHER);
        let (op,_,_)=case.latest().await;
        let before=posting_snapshot(&case,&case.board,op).await;
        assert_eq!(ordinary_request(&case,ordinary_form(&case,op,"","",comment),Some("192.0.2.120")).await.status(),StatusCode::BAD_REQUEST);
        assert_eq!(posting_snapshot(&case,&case.board,op).await,before);
        assert_eq!(ordinary_request(&case,ordinary_form(&case,op,"bypass_r9ksage","",comment),Some("192.0.2.120")).await.status(),StatusCode::SEE_OTHER);
        let before=posting_snapshot(&case,&case.board,op).await;
        assert_eq!(ordinary_request(&case,ordinary_form(&case,op,"bypass_r9k sage","",comment),Some("192.0.2.120")).await.status(),StatusCode::BAD_REQUEST);
        assert_eq!(posting_snapshot(&case,&case.board,op).await,before);
        assert_eq!(ordinary_request(&case,ordinary_form(&case,op,"","AC","Owned invalid transport"),None).await.status(),StatusCode::SERVICE_UNAVAILABLE);
        for (query,expected) in [
            ("SELECT count(*) FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",2),
            ("SELECT count(*) FROM post_secrets.poster_contexts WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",2),
            ("SELECT count(*) FROM post_secrets.op_replies WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",1),
        ] {
            let count:i64=sqlx::query_scalar(query).bind(&case.board).fetch_one(&case.owner).await.unwrap();
            assert_eq!(count,expected);
        }
    }).await;
    for query in [
        "DELETE FROM post_secrets.robot9000_texts WHERE board=$1",
        "DELETE FROM post_secrets.robot9000_mutes WHERE board=$1",
    ] {
        sqlx::query(query)
            .bind(&fixture.board)
            .execute(&fixture.owner)
            .await
            .unwrap();
    }
    fixture.cleanup().await;
    result.unwrap();
}

fn source_staff_id(capcode: &str, enabled: bool) -> Option<String> {
    let reference: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/staff-poster-ids.json")).unwrap();
    let cases = reference["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 14);
    cases
        .iter()
        .find(|case| case["capcode"] == capcode && case["display_ids"] == enabled)
        .expect("the source fixture covers each supported badge and display switch")["expected"]
        .as_str()
        .map(str::to_owned)
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
    async fn public_pool(&self) -> PgPool {
        pool("TEST_PUBLIC_DATABASE_URL").await
    }
    async fn new() -> Arc<Self> {
        let owner = pool("MIGRATION_DATABASE_URL").await;
        let board = format!("c{}", &uuid::Uuid::new_v4().simple().to_string()[..9]);
        sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES($1,'Owned staff posting','Synthetic',16000,100,100,100,10)")
            .bind(&board).execute(&owner).await.unwrap();
        let account: i64 = sqlx::query_scalar(
            "INSERT INTO staff_identity.accounts(role,flags) VALUES('moderator',ARRAY['capcode','capcodename','developer']) RETURNING id",
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
                proxy: None,
                poster_id_key: None,
                country_database: None,
                origin: origin.origin().ascii_serialization(),
                public_origin: "http://127.0.0.1:3000".into(),
                media_origin: "http://127.0.0.1:3002".into(),
                bind: "127.0.0.1:3001".parse().unwrap(),
                production: false,
                auth_database: String::new(),
                staff_database: String::new(),
                idle_timeout: Duration::from_secs(900),
                tripcode_key: Some(Arc::new(
                    board_domain::identity::SecureKey::parse(&"11".repeat(32)).unwrap(),
                )),
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
            board_store::StaffPostAuthority { auth_pool:&case.state.auth,session_hash:&session_hash,csrf_hash:&csrf_hash,ticket_hash:&auth::hash(&auth::token()).try_into().unwrap(),idle_seconds:900,highlight:false,authorized_limits:true,identity:None }).await.expect("actual staff posting transaction must succeed");
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
        assert_eq!(identity,(source_staff_id("mod",true),None,None));
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
            assert_eq!(post["capcode"],saved);assert_eq!(post["id"].as_str(),source_staff_id(saved,true).as_deref());assert!(post.get("country").is_none());
            assert!(!post["com"].as_str().unwrap().contains("<script>"));
        }
        let (last,_,_)=case.latest().await;
        sqlx::query("UPDATE content.boards SET json_tail_size=1 WHERE slug=$1").bind(&case.board).execute(&case.owner).await.unwrap();
        for router in [&case.public,&case.api] {
            let full=public_json(router,&format!("/{}/thread/{op}.json",case.board)).await;
            assert_eq!(full["posts"][0]["capcode"],"mod");
            for post in full["posts"].as_array().unwrap() {
                let expected=if post["no"]==op {None} else {source_staff_id(post["capcode"].as_str().unwrap(),true)};
                assert_eq!(post["id"].as_str(),expected.as_deref());
                assert!(post.get("trip").is_none() && post.get("country").is_none());
            }
            let tail=public_json(router,&format!("/{}/thread/{op}-tail.json",case.board)).await;
            assert!(tail["posts"][0].get("capcode").is_none());assert_eq!(tail["posts"][1]["capcode"],"founder");
            assert!(tail["posts"][0].get("id").is_none());assert_eq!(tail["posts"][1]["id"],source_staff_id("founder",true).unwrap());
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
async fn static_staff_ids_match_source_and_survive_policy_changes_without_network_material() {
    let fixture = Fixture::new().await;
    let case = fixture.clone();
    let result = tokio::spawn(async move {
        sqlx::query("UPDATE staff_identity.accounts SET role='admin' WHERE id=$1")
            .bind(case.account).execute(&case.owner).await.unwrap();
        let reference: serde_json::Value =
            serde_json::from_str(include_str!("fixtures/staff-poster-ids.json")).unwrap();
        let mut thread = 0;
        let mut saved: Vec<(i64,Option<String>)> = Vec::new();
        for row in reference["cases"].as_array().unwrap() {
            let badge = row["capcode"].as_str().unwrap();
            let enabled = row["display_ids"].as_bool().unwrap();
            if badge == "none" {
                assert!(row["expected"].is_null());
                continue;
            }
            sqlx::query("UPDATE content.boards SET user_ids=$2 WHERE slug=$1")
                .bind(&case.board).bind(enabled).execute(&case.owner).await.unwrap();
            assert_eq!(case.submit(thread,&format!("&badge={badge}"),&case.csrf,"http://localhost:3001").await.0,StatusCode::SEE_OTHER);
            let (id,parent,capcode) = case.latest().await;
            if thread == 0 { thread = parent; }
            assert_eq!(parent,thread);
            assert_eq!(capcode.as_deref(),Some(badge));
            let label: Option<String> = sqlx::query_scalar("SELECT poster_id FROM content.posts WHERE id=$1")
                .bind(id).fetch_one(&case.owner).await.unwrap();
            assert_eq!(label,source_staff_id(badge,enabled));
            saved.push((id,label));
        }
        let contexts: i64 = sqlx::query_scalar("SELECT count(*) FROM post_secrets.poster_contexts WHERE thread_id=$1")
            .bind(thread).fetch_one(&case.owner).await.unwrap();
        assert_eq!(contexts,0);
        for enabled in [false,true] {
            sqlx::query("UPDATE content.boards SET user_ids=$2,json_tail_size=2 WHERE slug=$1")
                .bind(&case.board).bind(enabled).execute(&case.owner).await.unwrap();
            for router in [&case.public,&case.api] {
                let full = public_json(router,&format!("/{}/thread/{thread}.json",case.board)).await;
                for (id,label) in &saved {
                    let post = full["posts"].as_array().unwrap().iter().find(|post| post["no"]==*id).unwrap();
                    assert_eq!(post["id"].as_str(),label.as_deref());
                    assert!(post.get("country").is_none() && post.get("board_flag").is_none());
                }
                assert!(full["posts"][0].get("unique_ips").is_none());
                let tail = public_json(router,&format!("/{}/thread/{thread}-tail.json",case.board)).await;
                assert!(tail["posts"][0].get("id").is_none());
                assert_eq!(tail["posts"][1]["id"],"Developer");
                assert_eq!(tail["posts"][2]["id"],"Founder");
                let index = public_json(router,&format!("/{}/1.json",case.board)).await;
                assert_eq!(index["threads"][0]["posts"].as_array().unwrap().last().unwrap()["id"],"Founder");
                let catalog = public_json(router,&format!("/{}/catalog.json",case.board)).await;
                assert_eq!(catalog[0]["threads"][0]["last_replies"].as_array().unwrap().last().unwrap()["id"],"Founder");
            }
            let html = public_text(&case.public,&format!("/{}/thread/{thread}",case.board)).await;
            assert!(!html.contains("posteruid"));
            assert!(html.contains("## Mod") && html.contains("## Founder"));
        }
        let public = pool("TEST_PUBLIC_DATABASE_URL").await;
        let mut forged = public.begin().await.unwrap();
        sqlx::query("SELECT set_config('board.poster_id','Admin',true),set_config('board.staff_is_admin','true',true),set_config('board.staff_post_ticket',repeat('0',64),true)")
            .execute(&mut *forged).await.unwrap();
        let error = sqlx::query("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,$2,'Owned forged label','','Owned forged static staff ID')")
            .bind(&case.board).bind(thread).execute(&mut *forged).await.unwrap_err();
        assert_eq!(error.as_database_error().and_then(|error| error.code()).as_deref(),Some("23514"));
        forged.rollback().await.unwrap();
        public.close().await;
        let mismatched = sqlx::query("UPDATE content.posts SET poster_id='Admin' WHERE id=$1")
            .bind(saved[0].0).execute(&case.owner).await.unwrap_err();
        assert_eq!(mismatched.as_database_error().and_then(|error| error.code()).as_deref(),Some("23514"));
        assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM post_secrets.staff_post_intents WHERE account_id=$1")
            .bind(case.account).fetch_one(&case.owner).await.unwrap(),0);
        sqlx::query("UPDATE content.boards SET user_ids=false,archive_retention_seconds=3600,archive_limit=10 WHERE slug=$1")
            .bind(&case.board).execute(&case.owner).await.unwrap();
        assert_eq!(case.submit(thread,"&badge=founder",&case.csrf,"http://localhost:3001").await.0,StatusCode::SEE_OTHER);
        let (last,_,_) = case.latest().await;
        assert!(sqlx::query_scalar::<_,Option<String>>("SELECT poster_id FROM content.posts WHERE id=$1")
            .bind(last).fetch_one(&case.owner).await.unwrap().is_none());
        sqlx::query("UPDATE content.threads SET archived_at=clock_timestamp(),archive_expires_at=clock_timestamp()+interval '1 hour' WHERE id=$1")
            .bind(thread).execute(&case.owner).await.unwrap();
        for router in [&case.public,&case.api] {
            let archived = public_json(router,&format!("/{}/thread/{thread}.json",case.board)).await;
            assert!(archived["posts"].as_array().unwrap().iter().all(|post| post.get("id").is_none()));
            assert_eq!(archived["posts"][0]["archived"],1);
        }
        let persisted: Vec<(i64,Option<String>)> = sqlx::query_as("SELECT id,poster_id FROM content.posts WHERE board=$1 AND id<>$2 ORDER BY id")
            .bind(&case.board).bind(last).fetch_all(&case.owner).await.unwrap();
        assert_eq!(persisted,saved);
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
    prepared_trip: Option<String>,
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
            prepared_trip: None,
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
    async fn issue_limited(
        &self,
        case: &Fixture,
        authorized: Option<bool>,
        limit: Option<i32>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "SELECT staff_identity.issue_limited_post_authority($1,$2,$3,900,false,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)",
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
        .bind(authorized)
        .bind(limit)
        .bind(self.wordfilter_payload.as_deref())
        .bind(self.wordfilter_search.as_deref())
        .execute(&case.state.auth)
        .await?;
        Ok(())
    }
    async fn issue_source(
        &self,
        case: &Fixture,
        options: &str,
        name_allowed: Option<bool>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("SELECT staff_identity.issue_source_post_authority($1,$2,$3,900,false,$4,$5,$6,$7,$8,$9,$10,true,10000,$11,$12,$13,$14,$15)")
            .bind(&self.ticket).bind(auth::hash(&case.token)).bind(auth::hash(&case.csrf))
            .bind(self.id).bind(&self.board).bind(self.thread).bind(&self.name).bind(&self.subject)
            .bind(&self.comment).bind(self.time).bind(self.wordfilter_payload.as_deref())
            .bind(self.wordfilter_search.as_deref()).bind(options).bind(self.prepared_trip.as_deref())
            .bind(name_allowed).execute(&case.state.auth).await?;
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
        sqlx::query("SELECT set_config('board.post_trip',$1,true)")
            .bind(self.prepared_trip.as_deref().unwrap_or_default())
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
        board_store::StaffPostAuthority { auth_pool:&case.state.auth,session_hash:&session_hash,csrf_hash:&csrf_hash,ticket_hash:&auth::hash(&auth::token()).try_into().unwrap(),idle_seconds:900,highlight:false,authorized_limits:true,identity:None }).await.unwrap();
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

fn staff_form(case: &Fixture, thread: i64, name: &str, subject: &str, comment: &str) -> String {
    url::form_urlencoded::Serializer::new(String::new())
        .append_pair("csrf", &case.csrf)
        .append_pair("board", &case.board)
        .append_pair("thread", &thread.to_string())
        .append_pair("name", name)
        .append_pair("subject", subject)
        .append_pair("comment", comment)
        .finish()
}

async fn posting_snapshot(case: &Fixture, board: &str, thread: i64) -> serde_json::Value {
    sqlx::query_scalar(
        "SELECT jsonb_build_object(\
            'posts',(SELECT count(*) FROM content.posts WHERE board=$1 AND thread_id=$2),\
            'audit',(SELECT count(*) FROM content.moderation_audit WHERE board=$1 AND account_id=$3),\
            'clock',(SELECT modified_at FROM content.threads WHERE board=$1 AND id=$2),\
            'intents',(SELECT count(*) FROM post_secrets.staff_post_intents WHERE account_id=$3))",
    )
    .bind(board)
    .bind(thread)
    .bind(case.account)
    .fetch_one(&case.owner)
    .await
    .unwrap()
}

#[tokio::test]
async fn public_staff_names_match_source_preparation_and_keep_only_prepared_hashes() {
    let fixture = Fixture::new().await;
    let case = fixture.clone();
    let result = tokio::spawn(async move {
        assert_eq!(
            case.submit(0, "", &case.csrf, "http://localhost:3001")
                .await
                .0,
            StatusCode::SEE_OTHER
        );
        let (thread, _, _) = case.latest().await;
        let source: serde_json::Value = serde_json::from_str(include_str!(
            "../../../crates/domain/tests/fixtures/staff-name.json"
        ))
        .unwrap();
        let group = source["groups"]
            .as_array()
            .unwrap()
            .iter()
            .find(|group| {
                group["board"] == "g"
                    && group["code"] == false
                    && group["sjis"] == false
                    && group["strip"] == false
            })
            .unwrap();
        let mut saved = Vec::new();
        for reference in group["cases"].as_array().unwrap() {
            let before = posting_snapshot(&case, &case.board, thread).await;
            let response = private_request(
                &case,
                "/post",
                Some(staff_form(
                    &case,
                    thread,
                    reference["input"].as_str().unwrap(),
                    "Owned subject",
                    "Owned source identity case",
                )),
            )
            .await;
            if reference["outcome"] == "too_long" {
                assert_eq!(
                    response.status(),
                    StatusCode::BAD_REQUEST,
                    "{}",
                    reference["input"]
                );
                assert_eq!(posting_snapshot(&case, &case.board, thread).await, before);
            } else {
                assert_eq!(
                    response.status(),
                    StatusCode::SEE_OTHER,
                    "{}",
                    reference["input"]
                );
                let (id, _, badge) = case.latest().await;
                assert_eq!(badge.as_deref(), Some("mod"));
                let identity: (String, Option<String>) =
                    sqlx::query_as("SELECT name,trip FROM content.posts WHERE id=$1")
                        .bind(id)
                        .fetch_one(&case.owner)
                        .await
                        .unwrap();
                assert_eq!(identity.0, reference["name"].as_str().unwrap());
                assert_eq!(identity.1.as_deref(), reference["modern_trip"].as_str());
                saved.push((
                    id,
                    reference["name_html"].as_str().unwrap().to_owned(),
                    identity.1,
                ));
            }
        }
        assert_eq!(saved.len(), 69);
        for router in [&case.public, &case.api] {
            let json = public_json(router, &format!("/{}/thread/{thread}.json", case.board)).await;
            for (id, name, trip) in &saved {
                let row = json["posts"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|row| row["no"] == *id)
                    .unwrap();
                if name.is_empty() && trip.is_some() {
                    assert!(row.get("name").is_none());
                } else {
                    assert_eq!(row["name"], *name);
                }
                assert_eq!(row["trip"].as_str(), trip.as_deref());
                assert_eq!(row["capcode"], "mod");
                assert!(row.get("id").is_none());
            }
        }
        let html = public_text(&case.public, &format!("/{}/thread/{thread}", case.board)).await;
        let updates = public_json(
            &case.public,
            &format!("/_watch/{}/thread/{thread}/posts", case.board),
        )
        .await;
        for (id, _, trip) in saved.iter().filter(|(_, _, trip)| trip.is_some()) {
            let trip = trip.as_deref().unwrap();
            let marker = format!("<span class=\"postertrip\">{trip}</span>");
            assert!(html.contains(&marker));
            let no = id.to_string();
            let row = updates["posts"]
                .as_array()
                .unwrap()
                .iter()
                .find(|row| row["no"].as_str() == Some(no.as_str()))
                .unwrap();
            assert!(row["html"].as_str().unwrap().contains(&marker));
            let preview =
                public_json(&case.public, &format!("/_watch/{}/post/{id}", case.board)).await;
            assert!(preview["post"]["html"].as_str().unwrap().contains(&marker));
        }
        assert!(!html.contains("#password"));
    })
    .await;
    fixture.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn postgres_source_badge_and_name_helpers_match_all_reference_cases() {
    let fixture = Fixture::new().await;
    let case = fixture.clone();
    let result=tokio::spawn(async move {
        let reference:serde_json::Value=serde_json::from_str(include_str!("fixtures/staff-capcodes.json")).unwrap();
        let cases=reference["cases"].as_array().unwrap(); assert_eq!(cases.len(),1024);
        let mut tx=case.owner.begin().await.unwrap();
        sqlx::query("SET LOCAL ROLE board_staff_post_owner").execute(&mut *tx).await.unwrap();
        for row in cases {
            let values=|field:&str|row[field].as_array().unwrap().iter().map(|value|value.as_str().unwrap().to_owned()).collect::<Vec<_>>();
            let (badge,named):(String,bool)=sqlx::query_as("SELECT staff_identity.source_public_capcode($1,$2,$3,$4,$5),staff_identity.source_capcode_name_allowed($1,$2,$3,$4)")
                .bind(row["role"].as_str().unwrap()).bind(values("flags")).bind(values("allow_boards"))
                .bind(values("deny_boards")).bind(row["choice"].as_str().unwrap()).fetch_one(&mut *tx).await.unwrap();
            assert_eq!(badge,row["outcome"].as_str().unwrap(),"{row}");
            let name=if row["choice"].as_str().unwrap().starts_with("capcode_") && !named {"Anonymous"} else {"Owned finished name"};
            assert_eq!(name,row["name"].as_str().unwrap(),"{row}");
        }
        tx.rollback().await.unwrap();
        for role in ["board_public","board_staff","board_auth","board_media","board_media_read","board_media_intake","board_monitor"] {
            for helper in ["staff_identity.source_public_capcode(text,text[],text[],text[],text)",
                "staff_identity.source_capcode_name_allowed(text,text[],text[],text[])","content.staff_display_name_size(text,text)"] {
                assert!(!sqlx::query_scalar::<_,bool>("SELECT has_function_privilege($1,$2,'EXECUTE')").bind(role).bind(helper).fetch_one(&case.owner).await.unwrap());
            }
        }
    }).await;
    fixture.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn source_staff_proofs_bind_hashes_and_recheck_flags_without_granting_legacy_trip_authority()
{
    let fixture = Fixture::new().await;
    let case = fixture.clone();
    let result=tokio::spawn(async move {
        assert_eq!(case.submit(0,"",&case.csrf,"http://localhost:3001").await.0,StatusCode::SEE_OTHER);
        let (thread,_,_)=case.latest().await;
        let mut post=BoundPost::new(&case,thread).await;
        let before=posting_snapshot(&case,&case.board,thread).await;
        for hash in ["private-password","!short","!!ABCDEFGHIJKx","!abcdefghij "] {
            post.prepared_trip=Some(hash.into());
            assert_eq!(sql_code(&post.issue_source(&case,"capcode_mod",Some(true)).await.unwrap_err()).as_deref(),Some("28000"));
            assert_eq!(posting_snapshot(&case,&case.board,thread).await,before);
        }
        post.prepared_trip=Some("!ozOtJW9BFA".into());
        for named in [None,Some(false)] {
            assert_eq!(sql_code(&post.issue_source(&case,"capcode_mod",named).await.unwrap_err()).as_deref(),Some("28000"));
            assert_eq!(posting_snapshot(&case,&case.board,thread).await,before);
        }
        for options in ["capcode_unknown","capcode_admin","capcode_founder","capcode_admin_hl",""] {
            assert_eq!(sql_code(&post.issue_source(&case,options,Some(true)).await.unwrap_err()).as_deref(),Some("28000"));
            assert_eq!(posting_snapshot(&case,&case.board,thread).await,before);
        }
        post.issue_source(&case,"capcode_mod",Some(true)).await.unwrap();
        let identity:(String,Option<String>,bool)=sqlx::query_as("SELECT source_options,prepared_trip,source_name_allowed FROM post_secrets.staff_post_intents WHERE token_hash=$1")
            .bind(&post.ticket).fetch_one(&case.owner).await.unwrap();
        assert_eq!(identity,("capcode_mod".into(),post.prepared_trip.clone(),true));
        let pending=posting_snapshot(&case,&case.board,thread).await;
        let mut forged=post.clone(); forged.prepared_trip=Some("!0123456789".into());
        assert_eq!(sql_code(&forged.insert(&case).await.unwrap_err()).as_deref(),Some("28000"));
        assert_eq!(posting_snapshot(&case,&case.board,thread).await,pending);
        for flags in [vec!["capcode"],vec!["capcodename"]] {
            sqlx::query("UPDATE staff_identity.accounts SET flags=$2 WHERE id=$1").bind(case.account).bind(flags).execute(&case.owner).await.unwrap();
            assert_eq!(sql_code(&post.insert(&case).await.unwrap_err()).as_deref(),Some("28000"));
            assert_eq!(posting_snapshot(&case,&case.board,thread).await,pending);
        }
        sqlx::query("UPDATE staff_identity.accounts SET flags=ARRAY['capcode','capcodename'] WHERE id=$1").bind(case.account).execute(&case.owner).await.unwrap();
        assert_eq!(post.insert(&case).await.unwrap(),"mod");
        assert_eq!(sqlx::query_scalar::<_,Option<String>>("SELECT trip FROM content.posts WHERE id=$1").bind(post.id).fetch_one(&case.owner).await.unwrap(),post.prepared_trip);
        assert_eq!(sql_code(&post.insert(&case).await.unwrap_err()).as_deref(),Some("28000"));
        let mut legacy=BoundPost::new(&case,thread).await; legacy.issue(&case).await.unwrap();
        legacy.prepared_trip=Some("!0123456789".into());
        assert_eq!(legacy.insert(&case).await.unwrap(),"mod");
        assert!(sqlx::query_scalar::<_,Option<String>>("SELECT trip FROM content.posts WHERE id=$1").bind(legacy.id).fetch_one(&case.owner).await.unwrap().is_none());
        let legacy=BoundPost::new(&case,thread).await; legacy.issue_limited(&case,Some(true),Some(10000)).await.unwrap();
        sqlx::query("UPDATE staff_identity.accounts SET flags=ARRAY['capcode'] WHERE id=$1").bind(case.account).execute(&case.owner).await.unwrap();
        let pending=posting_snapshot(&case,&case.board,thread).await;
        assert_eq!(sql_code(&legacy.insert(&case).await.unwrap_err()).as_deref(),Some("28000"));
        assert_eq!(posting_snapshot(&case,&case.board,thread).await,pending);
    }).await;
    fixture.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn source_badge_permissions_mask_names_after_validation_and_preserve_admin_board_exceptions()
{
    let fixture = Fixture::new().await;
    let case = fixture.clone();
    let result=tokio::spawn(async move {
        assert_eq!(case.submit(0,"",&case.csrf,"http://localhost:3001").await.0,StatusCode::SEE_OTHER);
        let (thread,_,_)=case.latest().await;
        let choices=private_html(&case,"/post").await;
        assert!(choices.contains("value=\"mod\"") && choices.contains("value=\"developer\""));
        assert!(!choices.contains("value=\"founder\"") && !choices.contains("value=\"manager\""));
        sqlx::query("UPDATE staff_identity.accounts SET flags=ARRAY['capcode'] WHERE id=$1").bind(case.account).execute(&case.owner).await.unwrap();
        let form=staff_form(&case,thread,"Owned#password","Owned","Owned masked identity");
        assert_eq!(private_request(&case,"/post",Some(form.clone())).await.status(),StatusCode::SEE_OTHER);
        let (id,_,_)=case.latest().await;
        let identity:(String,Option<String>)=sqlx::query_as("SELECT name,trip FROM content.posts WHERE id=$1").bind(id).fetch_one(&case.owner).await.unwrap();
        assert_eq!(identity,("Anonymous".into(),None));
        let before=posting_snapshot(&case,&case.board,thread).await;
        assert_eq!(private_request(&case,"/post",Some(staff_form(&case,thread,&"&".repeat(52),"","Owned oversized masked name"))).await.status(),StatusCode::BAD_REQUEST);
        assert_eq!(posting_snapshot(&case,&case.board,thread).await,before);
        sqlx::query("UPDATE staff_identity.accounts SET flags=ARRAY['developer'] WHERE id=$1").bind(case.account).execute(&case.owner).await.unwrap();
        assert_eq!(private_request(&case,"/post",Some(form.clone()+"&badge=developer")).await.status(),StatusCode::SEE_OTHER);
        assert_eq!(case.latest().await.2.as_deref(),Some("developer"));
        let before=posting_snapshot(&case,&case.board,thread).await;
        assert_eq!(private_request(&case,"/post",Some(form.clone()+"&badge=mod")).await.status(),StatusCode::UNAUTHORIZED);
        assert_eq!(posting_snapshot(&case,&case.board,thread).await,before);
        sqlx::query("UPDATE staff_identity.accounts SET flags=ARRAY['capcode','capcodename','developer'],allow_boards=ARRAY[$2] WHERE id=$1")
            .bind(case.account).bind(&case.board).execute(&case.owner).await.unwrap();
        assert_eq!(private_request(&case,"/post",Some(form.clone()+"&badge=developer")).await.status(),StatusCode::UNAUTHORIZED);
        assert_eq!(private_request(&case,"/post",Some(form.clone()+"&badge=mod")).await.status(),StatusCode::UNAUTHORIZED);
        assert_eq!(posting_snapshot(&case,&case.board,thread).await,before);
        sqlx::query("UPDATE staff_identity.accounts SET role='manager',flags='{}',allow_boards=ARRAY['all'] WHERE id=$1")
            .bind(case.account).execute(&case.owner).await.unwrap();
        for badge in ["manager","mod"] {
            assert_eq!(private_request(&case,"/post",Some(form.clone()+"&badge="+badge)).await.status(),StatusCode::SEE_OTHER);
            assert_eq!(case.latest().await.2.as_deref(),Some(badge));
        }
        sqlx::query("UPDATE staff_identity.accounts SET role='admin',flags='{}',allow_boards=ARRAY[$2],deny_boards=ARRAY['noboard'] WHERE id=$1")
            .bind(case.account).bind(&case.board).execute(&case.owner).await.unwrap();
        sqlx::query("UPDATE content.boards SET forced_anon=true WHERE slug=$1").bind(&case.board).execute(&case.owner).await.unwrap();
        for badge in ["mod","manager","admin","admin_highlight","founder"] {
            let input=staff_form(&case,thread,"Owned#password","Owned discarded subject","Owned administrator exception")+"&badge="+badge;
            assert_eq!(private_request(&case,"/post",Some(input)).await.status(),StatusCode::SEE_OTHER);
            let (id,_,label)=case.latest().await;
            assert_eq!(label.as_deref(),Some(badge));
            let identity:(String,Option<String>,String)=sqlx::query_as("SELECT name,trip,subject FROM content.posts WHERE id=$1").bind(id).fetch_one(&case.owner).await.unwrap();
            assert_eq!(identity,("Owned".into(),Some("!ozOtJW9BFA".into()),String::new()));
            let html=public_text(&case.public,&format!("/{}/thread/{thread}",case.board)).await;
            assert!(html.contains("<span class=\"name\">Owned</span> <span class=\"postertrip\">!ozOtJW9BFA</span>"));
            let catalog=public_text(&case.public,&format!("/{}/catalog",case.board)).await;
            let marker=format!("data-reply-id=\"{id}\"");
            let last=catalog.split_once(&marker).unwrap().1.split("</div>").next().unwrap();
            let visible=matches!(badge,"admin"|"admin_highlight");
            assert_eq!(last.contains("<span class=\"post-author\">Owned</span>"),visible);
            assert_eq!(last.contains("!ozOtJW9BFA"),visible);
            assert_eq!(last.contains("<span class=\"post-author\">Anonymous</span>"),!visible);
            assert_eq!(private_request(&case,"/post",Some(staff_form(&case,0,"Owned#password","","Owned catalog administrator")+"&badge="+badge)).await.status(),StatusCode::SEE_OTHER);
            let (op,_,_)=case.latest().await;
            for text_only in [false,true] {
                sqlx::query("UPDATE content.boards SET text_only=$2 WHERE slug=$1").bind(&case.board).bind(text_only).execute(&case.owner).await.unwrap();
                let catalog=public_text(&case.public,&format!("/{}/catalog",case.board)).await;
                let marker=format!("id=\"thread-{op}\"");
                let card=catalog.split_once(&marker).unwrap().1.split("id=\"thread-").next().unwrap();
                assert_eq!(card.contains("<span class=\"post-author\">Owned</span>"),visible);
                assert_eq!(card.contains("data-filter-name=\"Owned\""),visible);
                assert_eq!(card.contains("data-filter-trip=\"!ozOtJW9BFA\""),visible);
                assert_eq!(card.contains("data-filter-name=\"Anonymous\""),!visible);
            }
            sqlx::query("UPDATE content.boards SET text_only=false WHERE slug=$1").bind(&case.board).execute(&case.owner).await.unwrap();
        }
        sqlx::query("UPDATE content.boards SET strip_tripcode=true WHERE slug=$1").bind(&case.board).execute(&case.owner).await.unwrap();
        assert_eq!(private_request(&case,"/post",Some(form+"&badge=founder")).await.status(),StatusCode::SEE_OTHER);
        let (id,_,_)=case.latest().await;
        let identity:(String,Option<String>,String)=sqlx::query_as("SELECT name,trip,subject FROM content.posts WHERE id=$1").bind(id).fetch_one(&case.owner).await.unwrap();
        assert_eq!(identity,("Owned".into(),None,String::new()));
        let public=pool("TEST_PUBLIC_DATABASE_URL").await;
        let mut tx=public.begin().await.unwrap();
        sqlx::query("SELECT set_config('board.staff_is_admin','true',true),set_config('board.post_trip','!ozOtJW9BFA',true)").execute(&mut *tx).await.unwrap();
        let identity:(String,Option<String>,String,Option<String>)=sqlx::query_as("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES(nextval('content.post_number'),$1,$2,'Owned forged admin','Owned forged subject','Owned ordinary forged settings') RETURNING name,trip,subject,capcode")
            .bind(&case.board).bind(thread).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(identity,("Anonymous".into(),None,String::new(),None));
        tx.rollback().await.unwrap(); public.close().await;
    }).await;
    fixture.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn actual_staff_writer_rechecks_source_flags_after_the_board_lock_wait() {
    for flags in [vec!["capcode"], vec!["capcodename"]] {
        let fixture = Fixture::new().await;
        let case = fixture.clone();
        let result=tokio::spawn(async move {
            assert_eq!(case.submit(0,"",&case.csrf,"http://localhost:3001").await.0,StatusCode::SEE_OTHER);
            let (thread,_,_)=case.latest().await;
            let before=posting_snapshot(&case,&case.board,thread).await;
            let mut operator=case.owner.begin().await.unwrap();
            sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE").bind(&case.board).execute(&mut *operator).await.unwrap();
            let locker:i32=sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *operator).await.unwrap();
            let child_case=case.clone();
            let child=tokio::spawn(async move {
                private_request(&child_case,"/post",Some(staff_form(&child_case,thread,"Owned#password","Owned","Owned flag race"))).await
            });
            let mut observed=false;
            for _ in 0..150 {
                observed=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE usename='board_staff' AND $1=ANY(pg_blocking_pids(pid)))")
                    .bind(locker).fetch_one(&mut *operator).await.unwrap();
                if observed {break;} tokio::time::sleep(Duration::from_millis(10)).await;
            }
            sqlx::query("UPDATE staff_identity.accounts SET flags=$2 WHERE id=$1").bind(case.account).bind(flags).execute(&mut *operator).await.unwrap();
            operator.commit().await.unwrap();
            let response=child.await.unwrap();
            assert!(observed,"The actual source-identity writer must reach the board lock");
            assert_eq!(response.status(),StatusCode::UNAUTHORIZED);
            assert_eq!(posting_snapshot(&case,&case.board,thread).await,before);
            sqlx::query("UPDATE staff_identity.accounts SET flags=ARRAY['capcode','capcodename'] WHERE id=$1").bind(case.account).execute(&case.owner).await.unwrap();
            assert_eq!(private_request(&case,"/post",Some(staff_form(&case,thread,"Owned#password","Owned","Owned flag race"))).await.status(),StatusCode::SEE_OTHER);
        }).await;
        fixture.cleanup().await;
        result.unwrap();
    }
}

#[tokio::test]
async fn source_staff_ranks_select_raw_budgets_before_cleanup_and_accept_large_encoded_forms() {
    let fixture = Fixture::new().await;
    let case = fixture.clone();
    let result = tokio::spawn(async move {
        let source: serde_json::Value = serde_json::from_str(include_str!(
            "../../../crates/domain/tests/fixtures/authorized-post.json"
        )).unwrap();
        for board in source["boards"].as_array().unwrap() {
            let actual: i32 = sqlx::query_scalar(
                "SELECT max_authorized_comment_chars FROM content.boards WHERE slug=$1"
            ).bind(board["slug"].as_str().unwrap()).fetch_one(&case.owner).await.unwrap();
            assert_eq!(i64::from(actual), board["max_authorized_comment_chars"].as_i64().unwrap());
        }
        let unit = "\u{20000}";
        for (role, field_bytes, comment_chars, authorized) in [
            ("janitor",100,2000,false), ("moderator",255,50000,true),
            ("manager",255,50000,true), ("admin",255,50000,true),
        ] {
            sqlx::query("UPDATE staff_identity.accounts SET role=$2 WHERE id=$1")
                .bind(case.account).bind(role).execute(&case.owner).await.unwrap();
            let html = private_html(&case,"/j/").await;
            assert!(html.contains(&format!("<input name=\"sub\" id=\"sub\" value=\"\" maxlength=\"{field_bytes}\"")),"{role}");
            assert!(html.contains(&format!("maxlength=\"{}\"",comment_chars*2)),"{role}");
            let subject = "s".repeat(field_bytes);
            let body = unit.repeat(comment_chars);
            let form = private_form(&case,0,&body).replace("Owned+%3Cscript%3E+subject", &subject);
            if authorized { assert!(form.len()>600_000, "Must reach the real route's larger body limit"); }
            let response = private_request(&case,"/j/imgboard.php",Some(form)).await;
            assert_eq!(response.status(),StatusCode::SEE_OTHER,"{role}");
            let thread:i64 = response.headers()["location"].to_str().unwrap()
                .strip_prefix("/j/thread/").unwrap().split('#').next().unwrap().parse().unwrap();
            let saved:(String,String,bool,Vec<u8>,Option<String>) = sqlx::query_as(
                "SELECT name,subject,staff_authorized_limits,wordfilter_payload,capcode FROM content.posts WHERE id=$1"
            ).bind(thread).fetch_one(&case.owner).await.unwrap();
            assert_eq!(saved.0,"Anonymous"); assert_eq!(saved.1,subject); assert_eq!(saved.2,authorized);
            assert_eq!(&saved.3[..4],if authorized {b"WF02"} else {b"WF01"}); assert!(saved.4.is_none());
            let prepared=board_domain::wordfiltered_comment::PreparedComment::decode(&saved.3).unwrap();
            assert_eq!(board_domain::formatting::plain_text(&board_domain::filtered_formatting::lines(&prepared,"j")),body);
            let before = posting_snapshot(&case,"j",thread).await;
            for form in [
                private_form(&case,thread,&unit.repeat(comment_chars+1)),
                private_form(&case,thread,"Rejected subject").replace("Owned+%3Cscript%3E+subject",&"s".repeat(field_bytes+1)),
                private_form(&case,thread,"Rejected name").replace("Forged+Admin+%3Cname%3E",&"n".repeat(field_bytes+1)),
                private_form(&case,thread,"Rejected email").replace("fortune",&"e".repeat(field_bytes+1)),
                private_form(&case,thread,&"😀".repeat(comment_chars)),
            ] {
                assert_eq!(private_request(&case,"/j/imgboard.php",Some(form)).await.status(),StatusCode::BAD_REQUEST,"{role}");
                assert_eq!(posting_snapshot(&case,"j",thread).await,before,"Rejection must not change posts, audit, clock or proofs: {role}");
            }
            let repeated = "same\n".repeat(8);
            let over_lines = (0..=71).map(|line|format!("line{line}")).collect::<Vec<_>>().join("\n");
            for comment in [&repeated,&over_lines] {
                let status=private_request(&case,"/j/imgboard.php",Some(private_form(&case,thread,comment))).await.status();
                assert_eq!(status,if authorized {StatusCode::SEE_OTHER} else {StatusCode::BAD_REQUEST},"{role}");
            }
            for app in [&case.public,&case.api] {
                assert_eq!(app.clone().oneshot(Request::get(format!("/j/thread/{thread}.json")).body(Body::empty()).unwrap()).await.unwrap().status(),StatusCode::NOT_FOUND);
            }
        }
        // Larger posting forms do not enlarge authentication JSON endpoints.
        let response=board_staff::router(case.state.clone()).oneshot(Request::post("/login/start")
            .header("content-type","application/json")
            .body(Body::from(format!("{{\"username\":\"{}\"}}","a".repeat(262144)))).unwrap()).await.unwrap();
        assert_eq!(response.status(),StatusCode::PAYLOAD_TOO_LARGE);
    }).await;
    fixture.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn public_staff_limits_keep_finished_name_bounds_and_safe_large_wordfilter_views() {
    let fixture = Fixture::new().await;
    let case = fixture.clone();
    let result = tokio::spawn(async move {
        sqlx::query("UPDATE content.boards SET max_comment_chars=2000,comment_max_lines=100,comment_code_spacing=true,word_filter_enabled=true WHERE slug=$1")
            .bind(&case.board).execute(&case.owner).await.unwrap();
        let subject=format!("A{}B","\t".repeat(253));
        let comment="\u{20000}".repeat(10000);
        let response=private_request(&case,"/post",Some(staff_form(&case,0,&"n".repeat(255),&subject,&comment))).await;
        assert_eq!(response.status(),StatusCode::SEE_OTHER);
        let (thread,_,badge)=case.latest().await; assert_eq!(badge.as_deref(),Some("mod"));
        let public=pool("TEST_PUBLIC_DATABASE_URL").await;
        let saved=board_store::find_post(&public,&case.board,thread).await.unwrap();
        assert_eq!(saved.name,"n".repeat(255));
        assert_eq!(saved.subject,format!("A{}B"," ".repeat(1012)));
        assert_eq!(saved.wordfilter_payload.as_ref().unwrap().get(..4),Some(b"WF02".as_slice()));
        for router in [&case.public,&case.api] {
            let json=public_json(router,&format!("/{}/thread/{thread}.json",case.board)).await;
            let expected=format!("{}{}",format!("{}<wbr>","\u{20000}".repeat(35)).repeat(10000/35),"\u{20000}".repeat(10000%35));
            assert_eq!(json["posts"][0]["com"],expected);
            assert_eq!(json["posts"][0]["name"],"n".repeat(255)); assert_eq!(json["posts"][0]["capcode"],"mod");
        }
        let search_params=url::form_urlencoded::Serializer::new(String::new())
            .append_pair("q","\u{20000}").append_pair("b",&case.board).finish();
        let search=public_json(&case.public,&format!("/search/api?{search_params}")).await;
        assert_eq!(search["threads"][0]["thread"],thread.to_string());
        let search_html=search["threads"][0]["posts"][0]["html"].as_str().unwrap();
        assert!(search_html.contains('\u{20000}')); assert!(search_html.len()<20000);
        let before=posting_snapshot(&case,&case.board,thread).await;
        for (name,subject,comment) in [
            ("n".repeat(256),"Owned".into(),"Rejected name".into()),
            ("&".repeat(52),"Owned".into(),"Rejected escaped finished name".into()),
            ("Owned".into(),"s".repeat(256),"Rejected subject".into()),
            ("Owned".into(),"Owned".into(),"x".repeat(10001)),
        ] {
            assert_eq!(private_request(&case,"/post",Some(staff_form(&case,thread,&name,&subject,&comment))).await.status(),StatusCode::BAD_REQUEST);
            assert_eq!(posting_snapshot(&case,&case.board,thread).await,before);
        }
        // This body exceeds the old prepared/comment/frame limits after tab expansion.
        let expanded=format!("A{}B","\t".repeat(9998));
        let response=private_request(&case,"/post",Some(staff_form(&case,thread,"Owned","Owned",&expanded))).await;
        assert_eq!(response.status(),StatusCode::SEE_OTHER);
        let (id,_,_)=case.latest().await;
        let preview=public_json(&case.public,&format!("/_watch/{}/post/{id}",case.board)).await;
        assert!(preview["post"]["html"].as_str().unwrap().contains(&format!("A{}B"," ".repeat(39992))));
        let snapshot=posting_snapshot(&case,&case.board,thread).await;
        let public_form=url::form_urlencoded::Serializer::new(String::new())
            .append_pair("resto",&thread.to_string()).append_pair("name",&"n".repeat(101))
            .append_pair("com","Public still limited").append_pair("pwd","owned").finish();
        let response=case.public.clone().oneshot(Request::post(format!("/{}/post",case.board))
            .header("origin","http://127.0.0.1:3000").header("accept","application/json")
            .header("content-type","application/x-www-form-urlencoded")
            .body(Body::from(public_form)).unwrap()).await.unwrap();
        assert_eq!(response.status(),StatusCode::OK);
        let json:serde_json::Value=serde_json::from_slice(&to_bytes(response.into_body(),8192).await.unwrap()).unwrap();
        assert_eq!(json["error"],"Name or subject is too long.");
        assert_eq!(posting_snapshot(&case,&case.board,thread).await,snapshot);
        // A later policy change must retain earlier frames and render the full
        // expanded unfiltered staff comment through every saved-post reader.
        sqlx::query("UPDATE content.boards SET max_authorized_comment_chars=50000,word_filter_enabled=false WHERE slug=$1")
            .bind(&case.board).execute(&case.owner).await.unwrap();
        let raw=format!("A{}Z","\t".repeat(49998));
        assert_eq!(private_request(&case,"/post",Some(staff_form(&case,thread,"Owned","Owned",&raw))).await.status(),StatusCode::SEE_OTHER);
        let (expanded_id,_,_)=case.latest().await;
        let expected=format!("A{}Z"," ".repeat(199992));
        let expanded=board_store::find_post(&public,&case.board,expanded_id).await.unwrap();
        assert!(expanded.staff_authorized_limits); assert!(expanded.wordfilter_payload.is_none());
        assert_eq!(board_domain::formatting::plain_text(&expanded.formatted_lines()),expected);
        for router in [&case.public,&case.api] {
            let json=public_json(router,&format!("/{}/thread/{thread}.json",case.board)).await;
            let row=json["posts"].as_array().unwrap().iter().find(|post|post["no"]==expanded_id).unwrap();
            assert_eq!(row["com"],expected); assert!(row.get("staff_authorized_limits").is_none());
        }
        let preview=public_json(&case.public,&format!("/_watch/{}/post/{expanded_id}",case.board)).await;
        assert!(preview["post"]["html"].as_str().unwrap().contains(&expected));
        sqlx::query("INSERT INTO content.reports(board,post_id,reason) VALUES($1,$2,'Owned long staff preview')")
            .bind(&case.board).bind(expanded_id).execute(&case.owner).await.unwrap();
        assert!(private_html(&case,"/reports").await.contains(&expected));
        let search=public_json(&case.public,&format!("/search/api?q=Z&b={}",case.board)).await;
        let expanded_id_text=expanded_id.to_string();
        let excerpt=search["threads"][0]["posts"].as_array().unwrap().iter()
            .find(|post|post["no"].as_str()==Some(expanded_id_text.as_str())).unwrap()["html"].as_str().unwrap();
        assert!(excerpt.contains('Z')); assert!(excerpt.len()<20000);
        assert_eq!(board_store::find_post(&public,&case.board,thread).await.unwrap().wordfilter_payload,saved.wordfilter_payload);
        // Owned stored rows exercise the maximum SQL body size without changing
        // the posting parser. PostgreSQL compresses this repetitive fixture.
        let inserted=sqlx::query("WITH numbered AS (SELECT nextval('content.post_number') AS id FROM generate_series(1,40)), heads AS (INSERT INTO content.threads(id,board) SELECT id,$1 FROM numbered RETURNING id) INSERT INTO content.posts(id,board,thread_id,name,subject,comment) SELECT id,$1,id,'Owned read bound','','Owned' FROM heads")
            .bind(&case.board).execute(&case.owner).await.unwrap().rows_affected();
        assert_eq!(inserted,40);
        let changed=sqlx::query("UPDATE content.posts SET staff_authorized_limits=true,comment=repeat('x',2097152),wordfilter_payload=decode('5746303200000000ffff00','hex'),wordfilter_search='' WHERE board=$1 AND name='Owned read bound'")
            .bind(&case.board).execute(&case.owner).await.unwrap().rows_affected();
        assert_eq!(changed,40);
        let page=board_store::board_snapshot(&public,&case.board,board_store::BoardSelection::Page(1),Some(0)).await.unwrap();
        assert_eq!(page.threads.len(),page.board.threads_per_page as usize);
        assert!(page.threads.iter().all(|thread|thread.posts[0].staff_authorized_limits && thread.posts[0].comment.len()==2097152));
        let all=board_store::board_snapshot(&public,&case.board,board_store::BoardSelection::All,Some(0)).await;
        assert!(matches!(all,Err(board_store::StoreError::ReadLimit)),"The whole catalog must retain its former byte ceiling");
        public.close().await;
    }).await;
    fixture.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn authorized_proofs_require_current_rank_policy_and_non_null_bound_inputs() {
    let fixture = Fixture::new().await;
    let case = fixture.clone();
    let result = tokio::spawn(async move {
        sqlx::query("UPDATE content.boards SET max_comment_chars=2000 WHERE slug=$1")
            .bind(&case.board).execute(&case.owner).await.unwrap();
        assert_eq!(case.submit(0,"",&case.csrf,"http://localhost:3001").await.0,StatusCode::SEE_OTHER);
        let (thread,_,_)=case.latest().await;
        let post=BoundPost::new(&case,thread).await;
        let before=posting_snapshot(&case,&case.board,thread).await;
        for (authorized,limit) in [(None,Some(10000)),(Some(true),None),
            (Some(true),Some(0)),(Some(true),Some(50001)),(Some(true),Some(9999)),(Some(false),Some(10000))] {
            assert_eq!(sql_code(&post.issue_limited(&case,authorized,limit).await.unwrap_err()).as_deref(),Some("28000"));
            assert_eq!(posting_snapshot(&case,&case.board,thread).await,before);
        }
        post.issue_limited(&case,Some(true),Some(10000)).await.unwrap();
        let intent:(bool,Option<i32>)=sqlx::query_as("SELECT authorized_limits,comment_limit FROM post_secrets.staff_post_intents WHERE token_hash=$1")
            .bind(&post.ticket).fetch_one(&case.owner).await.unwrap();
        assert_eq!(intent,(true,Some(10000)));
        let pending=posting_snapshot(&case,&case.board,thread).await;
        sqlx::query("UPDATE content.boards SET max_authorized_comment_chars=9000 WHERE slug=$1")
            .bind(&case.board).execute(&case.owner).await.unwrap();
        assert_eq!(sql_code(&post.insert(&case).await.unwrap_err()).as_deref(),Some("28000"));
        assert_eq!(posting_snapshot(&case,&case.board,thread).await,pending);
        sqlx::query("UPDATE content.boards SET max_authorized_comment_chars=10000 WHERE slug=$1")
            .bind(&case.board).execute(&case.owner).await.unwrap();
        sqlx::query("UPDATE staff_identity.accounts SET role='janitor' WHERE id=$1")
            .bind(case.account).execute(&case.owner).await.unwrap();
        assert_eq!(sql_code(&post.insert(&case).await.unwrap_err()).as_deref(),Some("28000"));
        assert_eq!(posting_snapshot(&case,&case.board,thread).await,pending);
        sqlx::query("UPDATE staff_identity.accounts SET role='moderator' WHERE id=$1")
            .bind(case.account).execute(&case.owner).await.unwrap();
        assert_eq!(post.insert(&case).await.unwrap(),"mod");
        assert!(sqlx::query_scalar::<_,bool>("SELECT staff_authorized_limits FROM content.posts WHERE id=$1")
            .bind(post.id).fetch_one(&case.owner).await.unwrap());
        assert_eq!(sql_code(&post.insert(&case).await.unwrap_err()).as_deref(),Some("28000"));
        // The old issuer retains an ordinary proof, even for a current moderator.
        let legacy=BoundPost::new(&case,thread).await; legacy.issue(&case).await.unwrap();
        assert_eq!(legacy.insert(&case).await.unwrap(),"mod");
        assert!(!sqlx::query_scalar::<_,bool>("SELECT staff_authorized_limits FROM content.posts WHERE id=$1")
            .bind(legacy.id).fetch_one(&case.owner).await.unwrap());
        let public=pool("TEST_PUBLIC_DATABASE_URL").await;
        denied(&public,"SELECT staff_identity.issue_limited_post_authority(NULL,NULL,NULL,900,false,1,'x',1,'x','','x',clock_timestamp(),true,10000,NULL,NULL)").await;
        denied(&case.state.staff,"SELECT staff_identity.issue_limited_post_authority(NULL,NULL,NULL,900,false,1,'x',1,'x','','x',clock_timestamp(),true,10000,NULL,NULL)").await;
        for role in [&public,&case.state.staff,&case.state.auth] {
            denied(role,"UPDATE content.posts SET staff_authorized_limits=true WHERE false").await;
            denied(role,"UPDATE content.boards SET max_authorized_comment_chars=50000 WHERE false").await;
        }
        // A forged transaction setting cannot select the larger ordinary SQL bounds.
        let mut tx=public.begin().await.unwrap();
        sqlx::query("SELECT set_config('board.staff_authorized_limits','true',true)").execute(&mut *tx).await.unwrap();
        let id:i64=sqlx::query_scalar("SELECT nextval('content.post_number')").fetch_one(&mut *tx).await.unwrap();
        let error=sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$3,'Anonymous',$4,'Owned forged bound')")
            .bind(id).bind(&case.board).bind(thread).bind("s".repeat(401)).execute(&mut *tx).await.unwrap_err();
        assert_eq!(sql_code(&error).as_deref(),Some("23514"));
        tx.rollback().await.unwrap();
        public.close().await;
    }).await;
    fixture.cleanup().await;
    result.unwrap();
}

#[tokio::test]
async fn real_staff_posting_rechecks_policy_and_rank_after_the_board_lock_wait() {
    for downgrade in [true, false] {
        let fixture = Fixture::new().await;
        let case = fixture.clone();
        let result = tokio::spawn(async move {
            assert_eq!(case.submit(0,"",&case.csrf,"http://localhost:3001").await.0,StatusCode::SEE_OTHER);
            let (thread,_,_)=case.latest().await;
            let before=posting_snapshot(&case,&case.board,thread).await;
            let mut operator=case.owner.begin().await.unwrap();
            sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
                .bind(&case.board).execute(&mut *operator).await.unwrap();
            let locker:i32=sqlx::query_scalar("SELECT pg_backend_pid()").fetch_one(&mut *operator).await.unwrap();
            let child_case=case.clone();
            let child=tokio::spawn(async move {
                private_request(&child_case,"/post",Some(staff_form(&child_case,thread,"Owned","Owned",&"x".repeat(501)))).await
            });
            let mut observed=false;
            for _ in 0..150 {
                observed=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE usename='board_staff' AND $1=ANY(pg_blocking_pids(pid)))")
                    .bind(locker).fetch_one(&mut *operator).await.unwrap();
                if observed {break;} tokio::time::sleep(Duration::from_millis(10)).await;
            }
            if downgrade {
                sqlx::query("UPDATE staff_identity.accounts SET role='janitor' WHERE id=$1")
                    .bind(case.account).execute(&mut *operator).await.unwrap();
            } else {
                sqlx::query("UPDATE content.boards SET max_authorized_comment_chars=500 WHERE slug=$1")
                    .bind(&case.board).execute(&mut *operator).await.unwrap();
            }
            operator.commit().await.unwrap();
            let response=child.await.unwrap();
            assert!(observed,"Actual HTTP staff writer must wait for the selected board row");
            assert_eq!(response.status(),if downgrade {StatusCode::UNAUTHORIZED} else {StatusCode::BAD_REQUEST});
            assert_eq!(posting_snapshot(&case,&case.board,thread).await,before);
            if downgrade {
                sqlx::query("UPDATE staff_identity.accounts SET role='moderator' WHERE id=$1")
                    .bind(case.account).execute(&case.owner).await.unwrap();
            } else {
                sqlx::query("UPDATE content.boards SET max_authorized_comment_chars=10000 WHERE slug=$1")
                    .bind(&case.board).execute(&case.owner).await.unwrap();
            }
            assert_eq!(private_request(&case,"/post",Some(staff_form(&case,thread,"Owned","Owned",&"x".repeat(501)))).await.status(),StatusCode::SEE_OTHER);
        }).await;
        fixture.cleanup().await;
        result.unwrap();
    }
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
            board_store::StaffPostAuthority{auth_pool:&case.state.auth,session_hash:&auth::hash(&case.token),csrf_hash:&auth::hash(&case.csrf),ticket_hash:&auth::hash(&auth::token()).try_into().unwrap(),idle_seconds:900,highlight:false,authorized_limits:true,identity:None}).await;
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
