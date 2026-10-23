use super::*;
use serde_json::{Value, json};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
struct Stop(tokio::task::AbortHandle);
impl Drop for Stop {
    fn drop(&mut self) {
        self.0.abort();
    }
}
pub(super) struct Fixture<'a> {
    pub owner: &'a PgPool,
    pub public: &'a PgPool,
    pub endpoint: SocketAddr,
    pub key: &'a VerificationKey,
    pub profile: i64,
    pub sessions: Arc<Mutex<Vec<[u8; 32]>>>,
}
pub(super) async fn run(f: Fixture<'_>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let web = board_public::routers_with_events(
        f.public.clone(),
        board_public::PublicRouterOptions {
            origin: origin.clone(),
            production: false,
            media: None,
            limits: Default::default(),
            proxy_uid: None,
            poster_id_key: None,
            tripcode_key: None,
            country_database: None,
        },
        board_config::PublicVerificationSettings::parse(
            Some(&f.endpoint.to_string()),
            Some(&"11".repeat(32)),
        )
        .unwrap(),
        Default::default(),
        false,
    )
    .0;
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            web.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    let _stop = Stop(server.abort_handle());
    let mut command = tokio::process::Command::new("node");
    command.env_clear();
    for name in [
        "PATH",
        "HOME",
        "PLAYWRIGHT_BROWSERS_PATH",
        "LANG",
        "SystemRoot",
        "TEMP",
        "TMP",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    let mut child = command
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
        .arg("tests/browser/site-applications-native.mjs")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    input
        .write_all(format!("{}\n", json!({"origin":origin})).as_bytes())
        .await
        .unwrap();
    let peer = f.key.network_identity("127.0.0.1".parse().unwrap());
    let before: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM submission_private.applications WHERE campaign_id=$1",
    )
    .bind(f.profile)
    .fetch_one(f.owner)
    .await
    .unwrap();
    let before_edited: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM submission_private.applications WHERE campaign_id=$1 AND revision=2",
    )
    .bind(f.profile)
    .fetch_one(f.owner)
    .await
    .unwrap();
    let outcome=tokio::time::timeout(std::time::Duration::from_secs(180),async {
        let mut prepared=0;let mut answered=0;let mut sessions=0;
        while let Some(line)=lines.next_line().await.unwrap() {
            assert!(line.len()<=1024);let request:Value=serde_json::from_str(&line).unwrap();
            let response=match request["step"].as_str().unwrap() {
                "prepare"=>{assert!(prepared<8);prepared+=1;
                    sqlx::query("UPDATE verification_private.peers SET refresh_after=clock_timestamp()-interval '1 second' WHERE peer_hash=$1").bind(peer.as_slice()).execute(f.owner).await.unwrap();
                    json!({"ready":true})},
                "session"=>{assert!(sessions<prepared);sessions+=1;
                    let capability=Capability::parse(request["token"].as_str().unwrap()).unwrap();
                    let token=capability.storage_hash();
                    assert!(sqlx::query_scalar::<_,bool>("SELECT EXISTS(SELECT 1 FROM post_secrets.anonymous_sessions WHERE token_hash=$1)").bind(token.as_slice()).fetch_one(f.owner).await.unwrap());
                    f.sessions.lock().unwrap().push(token);json!({"recorded":true})},
                "answer"=>{assert!(answered<6);answered+=1;
                    let token=VerificationToken::parse(TokenKind::Challenge,request["token"].as_str().unwrap()).unwrap();
                    let (suspicious,session):(bool,Vec<u8>)=sqlx::query_as("SELECT suspicious,session_hash FROM verification_private.challenges WHERE token_hash=$1 AND board IS NULL AND ((purpose='site_application' AND target=$2) OR (purpose IN('site_application_auth','site_application_edit') AND target IN(SELECT id FROM submission_private.applications WHERE campaign_id=$2)))")
                        .bind(f.key.token_hash(&token).as_slice()).bind(f.profile).fetch_one(f.owner).await.unwrap();
                    f.sessions.lock().unwrap().push(session.try_into().unwrap());
                    json!({"answer":f.key.challenge_answer(&token,if suspicious{8}else{6}).unwrap().as_str()})},
                "done"=>{assert_eq!((prepared,answered,sessions),(8,6,2));break;},
                _=>panic!("Unknown owned application browser fixture request"),
            };
            input.write_all(format!("{response}\n").as_bytes()).await.unwrap();
        }
        assert_eq!((prepared,answered,sessions),(8,6,2));assert!(child.wait().await.unwrap().success());
        let after:i64=sqlx::query_scalar("SELECT count(*) FROM submission_private.applications WHERE campaign_id=$1").bind(f.profile).fetch_one(f.owner).await.unwrap();
        assert_eq!(after,before+2);
        let edited:i64=sqlx::query_scalar("SELECT count(*) FROM submission_private.applications WHERE campaign_id=$1 AND revision=2").bind(f.profile).fetch_one(f.owner).await.unwrap();assert_eq!(edited,before_edited+2);
    }).await;
    server.abort();
    let _ = server.await;
    sqlx::query("DELETE FROM verification_private.peers WHERE peer_hash=$1")
        .bind(peer.as_slice())
        .execute(f.owner)
        .await
        .unwrap();
    outcome.unwrap();
}
