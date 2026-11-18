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
pub(super) async fn run(app: axum::Router, owner: &PgPool, campaign: i64, cookie: &str) {
    let mut pairs = Vec::new();
    let mut ids = Vec::new();
    for index in 0..4 {
        let mut pair = Vec::new();
        for choice in 0..2 {
            let id:i64=sqlx::query_scalar("INSERT INTO submission_private.applications(campaign_id,age,timezone,hours,times,board1,answers,created_at,updated_at) VALUES($1,21,-4,3,'Evenings','g',ARRAY['Owned browser answer','Two','Three','Four'],1,1) RETURNING id").bind(campaign).fetch_one(owner).await.unwrap();
            sqlx::query("INSERT INTO submission_private.application_identity(application_id,campaign_id,ownership_hash,actor_hash,firstname,handle,email,email_identity) VALUES($1,$2,sha256(convert_to(gen_random_uuid()::text,'UTF8')),sha256(convert_to(gen_random_uuid()::text,'UTF8')),'Owned browser name','Owned browser handle',$3,$3)").bind(id).bind(campaign).bind(format!("browser-{index}-{choice}@example.invalid")).execute(owner).await.unwrap();
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
        .arg("tests/browser/staff-application-native.mjs")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    input.write_all(format!("{}\n",json!({"origin":"http://localhost:3001","cookie":cookie,"campaign":campaign,"pairs":pairs})).as_bytes()).await.unwrap();
    drop(input);
    let result = tokio::time::timeout(Duration::from_secs(180), child.wait()).await;
    server.abort();
    let _ = server.await;
    assert!(
        result
            .expect("Owned staff application browser deadline")
            .unwrap()
            .success()
    );
    let states: Vec<i16> = sqlx::query_scalar(
        "SELECT state FROM submission_private.applications WHERE id=ANY($1) ORDER BY id",
    )
    .bind(&ids)
    .fetch_all(owner)
    .await
    .unwrap();
    assert_eq!(states, vec![2, 1, 2, 1, 2, 1, 2, 1]);
    let count:i64=sqlx::query_scalar("SELECT count(*) FROM submission_private.application_review_log WHERE application_id=ANY($1)").bind(&ids).fetch_one(owner).await.unwrap();
    assert_eq!(count, 8);
}
