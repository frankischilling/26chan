use super::*;
use axum::body::to_bytes;
use board_domain::{
    challenge_secrets::{ReceiptToken, TokenKind, VerificationKey, VerificationToken},
    verification_mail::MailKey,
};
use board_store::verification::VerificationStore;
use std::sync::Arc;

fn encoded(pairs: &[(&str, &str)]) -> Bytes {
    Bytes::from(
        url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(pairs.iter().copied())
            .finish(),
    )
}
fn cookies(response: &Response) -> HeaderMap {
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        "application/x-www-form-urlencoded".parse().unwrap(),
    );
    for value in response.headers().get_all(header::SET_COOKIE) {
        headers.append(
            header::COOKIE,
            value
                .to_str()
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .parse()
                .unwrap(),
        );
    }
    headers
}
async fn decoded(response: Response) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), 262144).await.unwrap();
    let body = std::str::from_utf8(&bytes).unwrap();
    assert!(
        !body.contains("peer_hash") && !body.contains("context_hash") && !body.contains("receipt")
    );
    serde_json::from_slice(&bytes).unwrap()
}
#[tokio::test]
#[ignore = "Run explicitly with isolated verifier credentials."]
async fn public_captcha_requests_keep_private_scope_and_source_report_opt_out() {
    let owner = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let public = board_store::connect_public(&std::env::var("TEST_PUBLIC_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let issuer =
        VerificationStore::connect(&std::env::var("TEST_VERIFICATION_DATABASE_URL").unwrap())
            .await
            .unwrap();
    let board: String =
        sqlx::query_scalar("SELECT 'pc'||substr(replace(gen_random_uuid()::text,'-',''),1,8)")
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,captcha_twister,captcha_allow_bypass) VALUES($1,'Public CAPTCHA','Synthetic',2000,100,100,100,10,false,false)").bind(&board).execute(&owner).await.unwrap();
    let target: i64 =
        sqlx::query_scalar("INSERT INTO content.threads(board) VALUES($1) RETURNING id")
            .bind(&board)
            .fetch_one(&owner)
            .await
            .unwrap();
    sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES($1,$2,$1,'Anonymous','Owned target','Synthetic')").bind(target).bind(&board).execute(&owner).await.unwrap();
    sqlx::query("INSERT INTO content.post_media(post_id,job_id,asset_id,filename,bytes,width,height,spoiler,file_deleted) VALUES($1,replace(gen_random_uuid()::text,'-',''),replace(gen_random_uuid()::text,'-',''),'owned.png',100,1,1,false,true)").bind(target).execute(&owner).await.unwrap();
    let original:(Option<Vec<u8>>,bool,chrono::DateTime<chrono::Utc>)=sqlx::query_as("SELECT report_history_epoch,report_history_epoch_tainted,report_history_complete_after FROM verification_private.policy WHERE singleton").fetch_one(&owner).await.unwrap();
    let material = ReceiptToken::generate().unwrap().encode();
    let key = VerificationKey::parse(&material[3..]).unwrap();
    let poster_key = Arc::new(board_domain::poster_id::PosterIdKey::parse(&material[3..]).unwrap());
    let peer = "192.0.2.172".parse().unwrap();
    let peer_hash = key.network_identity(peer);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = listener.local_addr().unwrap();
    let app = board_verification::router(
        board_verification::AppState::new(
            issuer.clone(),
            VerificationKey::parse(&material[3..]).unwrap(),
            MailKey::parse(&material[3..]).unwrap(),
            &"11".repeat(32),
        )
        .unwrap(),
    );
    let serving = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    let state = AppState {
        pool: public.clone(),
        origin: "http://127.0.0.1:3000".into(),
        production: false,
        limits: Arc::new(crate::security::Limits::new(
            board_config::PublicRequestLimits::default(),
        )),
        media: None,
        verification: Some(
            board_verification_client::VerificationClient::new(endpoint, "11".repeat(32)).unwrap(),
        ),
        proxy_uid: None,
        poster_id_key: Some(poster_key.clone()),
        tripcode_key: None,
        country_database: None,
    };
    let token = Arc::new(std::sync::Mutex::new(None));
    let outcome=tokio::spawn({let owner=owner.clone();let board=board.clone();let token=token.clone();async move {
        let mut initial=HeaderMap::new();initial.insert(header::CONTENT_TYPE,"application/x-www-form-urlencoded".parse().unwrap());
        let response=request(State(state.clone()),Extension(RequestPeer(Some(peer))),initial.clone(),encoded(&[("action","init"),("board",&board),("purpose","thread"),("target","0")])).await.unwrap();
        let headers=cookies(&response);let result=decoded(response).await;
        assert_eq!(result["challenge"]["state"],"ready");assert_eq!(result["challenge"]["slider"],false);assert_eq!(result["ticket_challenge"],false);
        let nonce=result["csrf"].as_str().unwrap();
        assert!(PostingSecret::read(&headers,false).unwrap().unwrap().matches(nonce));
        assert!(crate::verification_form::FormSecret::read(&headers,false).unwrap().is_none());
        let session=Session::resolve(&state,&headers,Some(peer)).await.unwrap();assert!(!session.posting.minted);
        *token.lock().unwrap()=Some(session.posting.fingerprints.token);
        let challenge=VerificationToken::parse(TokenKind::Challenge,result["challenge"]["token"].as_str().unwrap()).unwrap();
        let scope:(String,String,i64,Vec<u8>)=sqlx::query_as("SELECT purpose,board,target,session_hash FROM verification_private.challenges WHERE token_hash=$1").bind(key.token_hash(&challenge).as_slice()).fetch_one(&owner).await.unwrap();
        assert_eq!(scope,("thread".into(),board.clone(),0,session.posting.fingerprints.token.to_vec()));
        let bad=request(State(state.clone()),Extension(RequestPeer(Some(peer))),headers.clone(),encoded(&[("action","challenge"),("board",&board),("purpose","thread"),("target","0"),("csrf","wrong")])).await.err().unwrap();assert_eq!(bad.0,StatusCode::FORBIDDEN);
        let forged=request(State(state.clone()),Extension(RequestPeer(Some(peer))),headers.clone(),encoded(&[("action","init"),("board",&board),("purpose","thread"),("target","0"),("peer","192.0.2.1")])).await;assert_eq!(forged.err().unwrap().0,StatusCode::BAD_REQUEST);
        let ordinary_token=result["challenge"]["token"].as_str().unwrap();
        let ordinary_answer=key.challenge_answer(&challenge,6).unwrap();
        let input=FormInput{csrf:nonce.into(),challenge:ordinary_token.into(),answer:ordinary_answer.as_str().into(),credit:String::new()};
        let wrong_scope=proof(&state,&headers,&session,Some(peer),Scope{board:board.clone(),target,purpose:protocol::Purpose::Report},&input).await;
        assert_eq!(wrong_scope.err().unwrap().0,StatusCode::BAD_REQUEST);
        let accepted=proof(&state,&headers,&session,Some(peer),Scope{board:board.clone(),target:0,purpose:protocol::Purpose::Thread},&input).await.unwrap();
        assert!(accepted.is_some());
        let replay=proof(&state,&headers,&session,Some(peer),Scope{board:board.clone(),target:0,purpose:protocol::Purpose::Thread},&input).await;
        assert_eq!(replay.err().unwrap().0,StatusCode::BAD_REQUEST);
        let old_form=FormInput{csrf:"00".repeat(32),..FormInput::default()};
        assert_eq!(proof(&state,&headers,&session,Some(peer),Scope{board:board.clone(),target:0,purpose:protocol::Purpose::Thread},&old_form).await.err().unwrap().0,StatusCode::FORBIDDEN);
        // A waiting ticket is distinct from the ordinary posting challenge.
        sqlx::query("UPDATE content.boards SET captcha_twister=true WHERE slug=$1").bind(&board).execute(&owner).await.unwrap();
        sqlx::query("UPDATE verification_private.peers SET refresh_after=clock_timestamp()-interval '1 second' WHERE peer_hash=$1").bind(peer_hash.as_slice()).execute(&owner).await.unwrap();
        let response=request(State(state.clone()),Extension(RequestPeer(Some(peer))),headers.clone(),encoded(&[("action","challenge"),("board",&board),("purpose","thread"),("target","0"),("csrf",nonce)])).await.unwrap();
        let ticket=decoded(response).await;assert_eq!(ticket["ticket_challenge"],true);
        let challenge=ticket["challenge"]["token"].as_str().unwrap();let parsed=VerificationToken::parse(TokenKind::Challenge,challenge).unwrap();let answer=key.challenge_answer(&parsed,8).unwrap();
        let response=request(State(state.clone()),Extension(RequestPeer(Some(peer))),headers.clone(),encoded(&[("action","challenge"),("board",&board),("purpose","thread"),("target","0"),("csrf",nonce),("ticket_challenge",challenge),("ticket_answer",answer.as_str())])).await.unwrap();
        let waiting=decoded(response).await;assert_eq!(waiting["challenge"]["state"],"wait");assert_eq!(waiting["ticket_challenge"],false);assert!(waiting["challenge"]["seconds"].as_u64().unwrap()<=900);
        let ticket=waiting["challenge"]["ticket"].as_str().unwrap();
        let response=request(State(state.clone()),Extension(RequestPeer(Some(peer))),headers.clone(),encoded(&[("action","challenge"),("board",&board),("purpose","thread"),("target","0"),("csrf",nonce),("ticket",ticket)])).await.unwrap();
        assert_eq!(decoded(response).await["challenge"]["ticket"],ticket);
        // Mature UserPwd-equivalent state can omit report CAPTCHA, even when
        // the attachment was deleted and the board prohibits credit bypass.
        sqlx::query("UPDATE post_secrets.anonymous_sessions SET created_at=extract(epoch FROM statement_timestamp())::bigint-700000,network_at=extract(epoch FROM statement_timestamp())::bigint-700000,address_at=extract(epoch FROM statement_timestamp())::bigint-700000,activity_at=extract(epoch FROM statement_timestamp())::bigint WHERE token_hash=$1").bind(session.posting.fingerprints.token.as_slice()).execute(&owner).await.unwrap();
        let actor=poster_key.public_report_rate_identity(peer);
        sqlx::query("UPDATE verification_private.policy SET report_history_epoch=$1,report_history_epoch_tainted=false,report_history_complete_after=clock_timestamp()-interval '1 second' WHERE singleton").bind(actor.report_history_epoch().as_slice()).execute(&owner).await.unwrap();
        let target=target.to_string();
        let response=request(State(state.clone()),Extension(RequestPeer(Some(peer))),headers.clone(),encoded(&[("action","challenge"),("board",&board),("purpose","report"),("target",&target),("csrf",nonce)])).await.unwrap();assert_eq!(decoded(response).await["report_exempt"],true);
        let empty_proof=FormInput{csrf:nonce.into(),..FormInput::default()};
        let current=Session::resolve(&state,&headers,Some(peer)).await.unwrap();
        assert!(proof(&state,&headers,&current,Some(peer),Scope{board:board.clone(),target:target.parse().unwrap(),purpose:protocol::Purpose::Report},&empty_proof).await.unwrap().is_none());
        let mut opted_out=headers.clone();opted_out.append(header::COOKIE,"pass_enabled=".parse().unwrap());
        sqlx::query("UPDATE verification_private.peers SET refresh_after=clock_timestamp()-interval '1 second' WHERE peer_hash=$1").bind(peer_hash.as_slice()).execute(&owner).await.unwrap();
        let response=request(State(state.clone()),Extension(RequestPeer(Some(peer))),opted_out,encoded(&[("action","challenge"),("board",&board),("purpose","report"),("target",&target),("csrf",nonce)])).await.unwrap();let result=decoded(response).await;assert_eq!(result["report_exempt"],false);assert_ne!(result["challenge"]["state"],"disabled");
        sqlx::query("UPDATE content.boards SET captcha_allow_bypass=true WHERE slug=$1").bind(&board).execute(&owner).await.unwrap();
        sqlx::query("UPDATE post_secrets.anonymous_sessions SET posts=5 WHERE token_hash=$1").bind(current.posting.fingerprints.token.as_slice()).execute(&owner).await.unwrap();
        sqlx::query("INSERT INTO verification_private.credits(identity_key,remaining,expires_at) VALUES(sha256(convert_to('26chan:captcha-credit:browser:v1','UTF8')||$1::bytea),1,clock_timestamp()+interval '1 hour')").bind(current.posting.fingerprints.token.as_slice()).execute(&owner).await.unwrap();
        let credit=FormInput{csrf:nonce.into(),credit:"1".into(),..FormInput::default()};
        assert!(proof(&state,&headers,&current,Some(peer),Scope{board:board.clone(),target:target.parse().unwrap(),purpose:protocol::Purpose::Reply},&credit).await.unwrap().is_some());
        let balance:i16=sqlx::query_scalar("SELECT remaining FROM verification_private.credits WHERE identity_key=sha256(convert_to('26chan:captcha-credit:browser:v1','UTF8')||$1::bytea)").bind(current.posting.fingerprints.token.as_slice()).fetch_one(&owner).await.unwrap();assert_eq!(balance,1);
        assert_eq!(proof(&state,&headers,&current,Some(peer),Scope{board:board.clone(),target:0,purpose:protocol::Purpose::Thread},&credit).await.err().unwrap().0,StatusCode::BAD_REQUEST);
        let mut unavailable_state=state.clone(); unavailable_state.verification=None;
        assert_eq!(proof(&unavailable_state,&headers,&current,Some(peer),Scope{board:board.clone(),target:target.parse().unwrap(),purpose:protocol::Purpose::Reply},&credit).await.err().unwrap().0,StatusCode::SERVICE_UNAVAILABLE);
        sqlx::query("UPDATE content.boards SET captcha_enabled=false WHERE slug=$1").bind(&board).execute(&owner).await.unwrap();
        let mut offline=state.clone();offline.verification=None;
        let response=request(State(offline),Extension(RequestPeer(Some(peer))),headers,encoded(&[("action","challenge"),("board",&board),("purpose","thread"),("target","0"),("csrf",nonce)])).await.unwrap();assert_eq!(decoded(response).await["challenge"]["state"],"disabled");
    }}).await;
    serving.abort();
    let _ = serving.await;
    sqlx::query("DELETE FROM verification_private.tickets WHERE board=$1")
        .bind(&board)
        .execute(&owner)
        .await
        .unwrap();
    sqlx::query("DELETE FROM verification_private.peers WHERE peer_hash=$1")
        .bind(peer_hash.as_slice())
        .execute(&owner)
        .await
        .unwrap();
    let token = *token.lock().unwrap();
    if let Some(token) = token {
        sqlx::query("DELETE FROM verification_private.credits WHERE identity_key=sha256(convert_to('26chan:captcha-credit:browser:v1','UTF8')||$1::bytea)").bind(token.as_slice()).execute(&owner).await.unwrap();
        sqlx::query("DELETE FROM post_secrets.anonymous_sessions WHERE token_hash=$1")
            .bind(token.as_slice())
            .execute(&owner)
            .await
            .unwrap();
    }
    let mut tx = owner.begin().await.unwrap();
    sqlx::query("SELECT slug FROM content.boards WHERE slug=$1 FOR UPDATE")
        .bind(&board)
        .execute(&mut *tx)
        .await
        .unwrap();
    for query in [
        "DELETE FROM content.post_media WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(query)
            .bind(&board)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    tx.commit().await.unwrap();
    sqlx::query("UPDATE verification_private.policy SET report_history_epoch=$1,report_history_epoch_tainted=$2,report_history_complete_after=$3 WHERE singleton").bind(original.0).bind(original.1).bind(original.2).execute(&owner).await.unwrap();
    issuer.close().await;
    public.close().await;
    owner.close().await;
    outcome.unwrap();
}
