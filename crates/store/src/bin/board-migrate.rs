#![forbid(unsafe_code)]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database = std::env::var("MIGRATION_DATABASE_URL")?;
    let pool = sqlx::PgPool::connect(&database).await?;
    let user: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&pool)
        .await?;
    if user != "board_migrator" {
        return Err("Migration requires the board_migrator login.".into());
    }
    sqlx::migrate!("../../migrations").run(&pool).await?;
    println!("Migrations applied.");
    pool.close().await;
    Ok(())
}
