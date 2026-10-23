use super::*;
use serde_json::json;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
struct Stop(tokio::task::AbortHandle);
impl Drop for Stop {
    fn drop(&mut self) {
        self.0.abort();
    }
}
pub(super) async fn run(app: axum::Router, owner: &PgPool, profile: i64, cookie: &str) {
    let mut pairs = Vec::new();
    let mut ids = Vec::new();
    for index in 0..4 {
        let mut pair = Vec::new();
        for state in [0i16, 3] {
            let id:i64=sqlx::query_scalar("INSERT INTO submission_private.feedback(profile_id,category,subject,message,state,actor_hash,peer,user_agent,language,created_at,updated_at,reply,reply_capcode) VALUES($1,1,$2,'Owned browser feedback',$3,decode(repeat('ab',32),'hex'),'192.0.2.225','Owned browser','en',100,100,$4,$5) RETURNING id")
                .bind(profile).bind(format!("Owned browser {index}-{state}")).bind(state).bind((state==3).then_some("Owned draft reply")).bind((state==3).then_some("mod")).fetch_one(owner).await.unwrap();
            pair.push(id);
            ids.push(id);
        }
        pairs.push(pair);
    }
    let listener = tokio::net::TcpListener::bind("127.0.0.1:3001")
        .await
        .unwrap();
    let server = tokio::spawn(async move {
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
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
        .arg("tests/browser/staff-feedback-native.mjs")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    input.write_all(format!("{}\n",json!({"origin":"http://localhost:3001","cookie":cookie,"profile":profile,"pairs":pairs})).as_bytes()).await.unwrap();
    drop(input);
    let result = tokio::time::timeout(Duration::from_secs(180), child.wait()).await;
    server.abort();
    let _ = server.await;
    assert!(
        result
            .expect("Owned staff feedback browser deadline")
            .unwrap()
            .success()
    );
    let states: Vec<i16> = sqlx::query_scalar(
        "SELECT state FROM submission_private.feedback WHERE id=ANY($1) ORDER BY id",
    )
    .bind(&ids)
    .fetch_all(owner)
    .await
    .unwrap();
    assert_eq!(states, vec![2, 1, 2, 1, 2, 1, 2, 1]);
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM submission_private.feedback_review_log WHERE feedback_id=ANY($1)",
    )
    .bind(&ids)
    .fetch_one(owner)
    .await
    .unwrap();
    assert_eq!(count, 8);
}
