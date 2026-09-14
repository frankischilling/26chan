//! Owned synthetic board setup for native updater tail browser tests.
#![forbid(unsafe_code)]

#[tokio::main]
async fn main() {
    if run().await.is_err() {
        eprintln!("Synthetic tail fixture failed");
        std::process::exit(1);
    }
}
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2
        || args[1].len() != 10
        || !args[1].starts_with("ut")
        || !args[1][2..].bytes().all(|b| b.is_ascii_hexdigit())
    {
        return Err("Invalid fixture arguments".into());
    }
    let pool = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL")?).await?;
    let user: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&pool)
        .await?;
    if user != "board_migrator" {
        return Err("Migration identity required".into());
    }
    let slug = &args[1];
    let mut tx = pool.begin().await?;
    if args[0] == "setup" {
        sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,json_tail_size) VALUES ($1,'Synthetic tail test','Owned updater tail browser fixture',4000,1000,300,100,10,2)").bind(slug).execute(&mut *tx).await?;
    } else {
        let owned: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM content.boards WHERE slug=$1 AND description='Owned updater tail browser fixture')").bind(slug).fetch_one(&mut *tx).await?;
        if !owned {
            return Err("Owned fixture board required".into());
        }
        match args[0].as_str() {
            "age" => {
                sqlx::query("UPDATE content.posts SET created_at=now()-interval '10 minutes' WHERE board=$1").bind(slug).execute(&mut *tx).await?;
            }
            "tail-off" => {
                sqlx::query("UPDATE content.boards SET json_tail_size=0 WHERE slug=$1")
                    .bind(slug)
                    .execute(&mut *tx)
                    .await?;
            }
            "cleanup" => {
                for query in [
                    "DELETE FROM content.reports WHERE board=$1",
                    "DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
                    "DELETE FROM content.posts WHERE board=$1",
                    "DELETE FROM content.threads WHERE board=$1",
                    "DELETE FROM content.boards WHERE slug=$1",
                ] {
                    sqlx::query(query).bind(slug).execute(&mut *tx).await?;
                }
            }
            _ => return Err("Unknown fixture command".into()),
        }
    }
    tx.commit().await?;
    pool.close().await;
    Ok(())
}
