//! Owned two-page board setup for real native navigation browser qualification.
#![forbid(unsafe_code)]

#[tokio::main]
async fn main() {
    if run().await.is_err() {
        eprintln!("Owned navigation fixture failed");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if arguments.len() != 2
        || arguments[1].len() != 10
        || !arguments[1].starts_with("dp")
        || !arguments[1][2..]
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    {
        return Err("Invalid navigation fixture arguments".into());
    }
    let pool = sqlx::PgPool::connect(&std::env::var("MIGRATION_DATABASE_URL")?).await?;
    let identity: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&pool)
        .await?;
    if identity != "board_migrator" {
        return Err("Migration identity required".into());
    }
    let slug = &arguments[1];
    let mut transaction = pool.begin().await?;
    match arguments[0].as_str() {
        "setup" => {
            sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,image_limit) VALUES($1,'Owned navigation board','Owned native navigation browser fixture',4000,20,10,12,2,0)")
                .bind(slug).execute(&mut *transaction).await?;
        }
        "cleanup" => {
            let owned: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM content.boards WHERE slug=$1 AND description='Owned native navigation browser fixture')")
                .bind(slug).fetch_one(&mut *transaction).await?;
            if !owned {
                return Err("Owned navigation board required".into());
            }
            for statement in [
                "DELETE FROM content.reports WHERE board=$1",
                "DELETE FROM post_secrets.deletion WHERE post_id IN (SELECT id FROM content.posts WHERE board=$1)",
                "DELETE FROM content.posts WHERE board=$1",
                "DELETE FROM content.threads WHERE board=$1",
                "DELETE FROM content.boards WHERE slug=$1",
            ] {
                sqlx::query(statement)
                    .bind(slug)
                    .execute(&mut *transaction)
                    .await?;
            }
        }
        _ => return Err("Unknown navigation fixture command".into()),
    }
    transaction.commit().await?;
    pool.close().await;
    Ok(())
}
