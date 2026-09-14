#![cfg(all(feature = "database-tests", target_os = "linux"))]

use rand_core::{OsRng, RngCore};
use sqlx::PgPool;

#[tokio::test]
async fn real_https_proxy_preserves_owner_identity_and_separate_client_limits() {
    let owner = PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL").unwrap())
        .await
        .unwrap();
    let mut random = [0u8; 5];
    OsRng.fill_bytes(&mut random);
    let slug: String = random.iter().map(|b| format!("{b:02x}")).collect();
    sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,op_markup,op_bump_limit) VALUES($1,'Proxy identity','Owned HTTPS proxy fixture',4000,200,150,100,10,true,true)").bind(&slug).execute(&owner).await.unwrap();
    let fixture = slug.clone();
    let inspector = owner.clone();
    let result = tokio::spawn(async move {
        let output = tokio::process::Command::new("python3")
            .arg(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/../../scripts/test-public-proxy.py"
            ))
            .arg(env!("CARGO_BIN_EXE_board-public"))
            .arg(&fixture)
            .kill_on_drop(true)
            .output()
            .await
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let posts: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        for (field, expected) in [
            ("op", 56),
            ("same", 56),
            ("same_again", 56),
            ("other", 40),
            ("password", 56),
        ] {
            let format: i16 = sqlx::query_scalar(
                "SELECT comment_format FROM content.posts WHERE board=$1 AND id=$2",
            )
            .bind(&fixture)
            .bind(posts[field].as_i64().unwrap())
            .fetch_one(&inspector)
            .await
            .unwrap();
            assert_eq!(format, expected, "{field}");
        }
        let op = posts["op"].as_i64().unwrap();
        let own_replies: Vec<i64> = sqlx::query_scalar(
            "SELECT post_id FROM post_secrets.op_replies WHERE thread_id=$1 ORDER BY post_id",
        )
        .bind(op)
        .fetch_all(&inspector)
        .await
        .unwrap();
        assert_eq!(
            own_replies,
            vec![
                posts["same"].as_i64().unwrap(),
                posts["same_again"].as_i64().unwrap()
            ]
        );
    })
    .await;
    sqlx::query("DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)").bind(&slug).execute(&owner).await.unwrap();
    for query in [
        "DELETE FROM content.posts WHERE board=$1",
        "DELETE FROM content.threads WHERE board=$1",
        "DELETE FROM content.boards WHERE slug=$1",
    ] {
        sqlx::query(query)
            .bind(&slug)
            .execute(&owner)
            .await
            .unwrap();
    }
    owner.close().await;
    result.unwrap();
}
