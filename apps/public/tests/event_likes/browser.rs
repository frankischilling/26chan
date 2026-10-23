use super::*;
use serde_json::{Value, json};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

struct StopServer(tokio::task::AbortHandle);
impl Drop for StopServer {
    fn drop(&mut self) {
        self.0.abort();
    }
}

pub(super) struct Fixture<'a> {
    pub owner: &'a PgPool,
    pub public: &'a PgPool,
    pub endpoint: SocketAddr,
    pub key: VerificationKey,
    pub board: &'a str,
    pub op: i64,
    pub run: i64,
    pub sessions: Arc<Mutex<Vec<[u8; 32]>>>,
}
pub(super) async fn run(f: Fixture<'_>) {
    let mut posts = Vec::new();
    for _ in 0..4 {
        let post:i64=sqlx::query_scalar("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES($1,$2,'Anonymous','','Owned browser like') RETURNING id")
            .bind(f.board).bind(f.op).fetch_one(f.owner).await.unwrap();
        sqlx::query("INSERT INTO post_secrets.posting_history(post_id,board,thread_id,actor_hash,request_at) VALUES($1,$2,$3,$4,1)")
            .bind(post).bind(f.board).bind(f.op).bind([55u8;32].as_slice()).execute(f.owner).await.unwrap();
        posts.push(post);
    }
    sqlx::query("UPDATE event_private.runs SET enabled=true WHERE id=$1")
        .bind(f.run)
        .execute(f.owner)
        .await
        .unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let settings = board_config::PublicVerificationSettings::parse(
        Some(&f.endpoint.to_string()),
        Some(&"11".repeat(32)),
    )
    .unwrap();
    let web = board_public::routers_with_events(
        f.public.clone(),
        board_public::PublicRouterOptions {
            origin: origin.clone(),
            production: false,
            media: None,
            limits: Default::default(),
            proxy_uid: None,
            poster_id_key: Some(Arc::new(PosterIdKey::parse(&"44".repeat(32)).unwrap())),
            tripcode_key: None,
            country_database: None,
        },
        settings,
        Default::default(),
        true,
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
    let _stop_server = StopServer(server.abort_handle());
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
        .arg("tests/browser/event-likes-native.mjs")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    let mut output = BufReader::new(child.stdout.take().unwrap()).lines();
    input
        .write_all(
            format!(
                "{}\n",
                json!({"origin":origin,"board":f.board,"posts":posts})
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let peer = f.key.network_identity("127.0.0.1".parse().unwrap());
    let result=tokio::time::timeout(std::time::Duration::from_secs(180),async {
        let mut prepared=0;let mut answered=0;
        while let Some(line)=output.next_line().await.unwrap() {
            assert!(line.len()<=1024);
            let request:Value=serde_json::from_str(&line).unwrap();
            let response=match request["step"].as_str().unwrap() {
                "prepare"=>{
                    assert!(prepared<4);prepared+=1;
                    sqlx::query("UPDATE verification_private.peers SET refresh_after=clock_timestamp()-interval '1 second' WHERE peer_hash=$1").bind(peer.as_slice()).execute(f.owner).await.unwrap();
                    sqlx::query("UPDATE event_private.score_accounts SET last_vote_at=0 WHERE run_id=$1").bind(f.run).execute(f.owner).await.unwrap();
                    json!({"ready":true})
                }
                "answer"=>{
                    assert!(answered<prepared);answered+=1;
                    let post=request["post"].as_i64().unwrap();assert!(posts.contains(&post));
                    let token=VerificationToken::parse(TokenKind::Challenge,request["token"].as_str().unwrap()).unwrap();
                    let (suspicious,session):(bool,Vec<u8>)=sqlx::query_as("SELECT suspicious,session_hash FROM verification_private.challenges WHERE token_hash=$1 AND board=$2 AND target=$3 AND purpose='like'")
                        .bind(f.key.token_hash(&token).as_slice()).bind(f.board).bind(post).fetch_one(f.owner).await.unwrap();
                    f.sessions.lock().unwrap().push(session.try_into().unwrap());
                    let answer=f.key.challenge_answer(&token,if suspicious{8}else{6}).unwrap();
                    json!({"answer":answer.as_str()})
                }
                "done"=>{assert_eq!((prepared,answered),(4,4));break;}
                _=>panic!("Unknown owned browser fixture request"),
            };
            input.write_all(format!("{response}\n").as_bytes()).await.unwrap();
        }
        assert_eq!((prepared,answered),(4,4));
        assert!(child.wait().await.unwrap().success());
        let counts:Vec<i32>=sqlx::query_scalar("SELECT likes FROM event_private.score_posts WHERE run_id=$1 AND post_id=ANY($2) ORDER BY post_id")
            .bind(f.run).bind(&posts).fetch_all(f.owner).await.unwrap();
        assert_eq!(counts,vec![1,1,1,1]);
    }).await;
    server.abort();
    let _ = server.await;
    sqlx::query("DELETE FROM verification_private.peers WHERE peer_hash=$1")
        .bind(peer.as_slice())
        .execute(f.owner)
        .await
        .unwrap();
    result.expect("Native score browser timeout");
}
