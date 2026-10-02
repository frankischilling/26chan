//! Owned synthetic policy setup; never changes an original board.
#![forbid(unsafe_code)]
#[tokio::main]
async fn main() {
    if run().await.is_err() {
        eprintln!("Synthetic wordfilter fixture failed");
        std::process::exit(1);
    }
}
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !(args.len() == 2 || args.len() == 3)
        || args[1].len() != 10
        || !args[1].starts_with('z')
        || !args[1]
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
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
    match args[0].as_str() {
        "setup" if args.len() == 3 => {
            let profile: i16 = args[2].parse()?;
            if !(0..=4).contains(&profile) {
                return Err("Unknown fixed profile".into());
            }
            sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_chars,reply_limit,bump_limit,thread_limit,threads_per_page,comment_code_spacing,comment_spoiler_cleanup,comment_sjis_spacing,op_markup,word_filter_enabled,word_filter_profile) VALUES($1,'Owned wordfilters','Owned wordfilter browser fixture',4000,100,100,100,10,true,true,true,true,true,$2)").bind(slug).bind(profile).execute(&mut *tx).await?;
        }
        "cleanup" if args.len() == 2 => {
            let owned: bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM content.boards WHERE slug=$1 AND description='Owned wordfilter browser fixture')").bind(slug).fetch_one(&mut *tx).await?;
            if !owned {
                return Err("Owned fixture board required".into());
            }
            for query in [
                "DELETE FROM content.reports WHERE board=$1",
                "DELETE FROM post_secrets.deletion WHERE post_id IN(SELECT id FROM content.posts WHERE board=$1)",
                "DELETE FROM content.posts WHERE board=$1",
                "DELETE FROM content.threads WHERE board=$1",
                "DELETE FROM content.boards WHERE slug=$1",
            ] {
                sqlx::query(query).bind(slug).execute(&mut *tx).await?;
            }
        }
        _ => return Err("Unknown fixture command".into()),
    }
    tx.commit().await?;
    pool.close().await;
    Ok(())
}
