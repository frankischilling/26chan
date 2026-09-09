//! Synthetic browser data only; this executable requires migration authority.
#![forbid(unsafe_code)]
use serde_json::json;
#[tokio::main]
async fn main() {
    if run().await.is_err() {
        eprintln!("Synthetic staff fixture failed");
        std::process::exit(1);
    }
}
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 || args[1].len() != 10 || !args[1].bytes().all(|b| b.is_ascii_alphanumeric())
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
    let board = &args[1];
    match args[0].as_str() {
        "setup" => {
            let mut tx = pool.begin().await?;
            sqlx::query("INSERT INTO content.boards(slug,title,description,max_comment_bytes,reply_limit,bump_limit,thread_limit,threads_per_page) VALUES ($1,'Synthetic staff test','Harmless fixtures',16000,100,100,100,10)").bind(board).execute(&mut *tx).await?;
            let thread: i64 =
                sqlx::query_scalar("INSERT INTO content.threads(board) VALUES ($1) RETURNING id")
                    .bind(board)
                    .fetch_one(&mut *tx)
                    .await?;
            sqlx::query("INSERT INTO content.posts(id,board,thread_id,name,subject,comment) VALUES ($1,$2,$1,'<b>Synthetic</b>','Harmless <em>subject</em>','<i>Preview text</i>')").bind(thread).bind(board).execute(&mut *tx).await?;
            let post:i64=sqlx::query_scalar("INSERT INTO content.posts(board,thread_id,name,subject,comment) VALUES ($1,$2,'Synthetic reply','','Harmless reply') RETURNING id").bind(board).bind(thread).fetch_one(&mut *tx).await?;
            sqlx::query("UPDATE content.threads SET reply_count=1 WHERE id=$1")
                .bind(thread)
                .execute(&mut *tx)
                .await?;
            let report:i64=sqlx::query_scalar("INSERT INTO content.reports(board,post_id,reason) VALUES ($1,$2,'Review <b>text</b>') RETURNING id").bind(board).bind(post).fetch_one(&mut *tx).await?;
            tx.commit().await?;
            println!("{}", json!({"thread":thread,"post":post,"report":report}));
        }
        "inspect" => {
            let states: Vec<(bool, bool, bool)> =
                sqlx::query_as("SELECT closed,sticky,deleted FROM content.threads WHERE board=$1")
                    .bind(board)
                    .fetch_all(&pool)
                    .await?;
            let audit: Vec<String> = sqlx::query_scalar(
                "SELECT action FROM content.moderation_audit WHERE board=$1 ORDER BY id",
            )
            .bind(board)
            .fetch_all(&pool)
            .await?;
            let credentials:i64=sqlx::query_scalar("SELECT count(*) FROM staff_identity.credentials c JOIN staff_identity.accounts a ON a.id=c.account_id WHERE a.username=$1").bind(board).fetch_one(&pool).await?;
            let sessions:i64=sqlx::query_scalar("SELECT count(*) FROM staff_identity.sessions s JOIN staff_identity.accounts a ON a.id=s.account_id WHERE a.username=$1").bind(board).fetch_one(&pool).await?;
            println!(
                "{}",
                json!({"states":states,"audit":audit,"credentials":credentials,"sessions":sessions})
            );
        }
        "stale" => {
            sqlx::query("UPDATE staff_identity.sessions SET authenticated_at=clock_timestamp()-interval '11 minutes' WHERE account_id=(SELECT id FROM staff_identity.accounts WHERE username=$1)").bind(board).execute(&pool).await?;
        }
        "age-activity" | "idle" => {
            // Only the migration-authority fixture can move persisted time.
            // The browser server uses the explicit 900-second production policy.
            let seconds = if args[0] == "idle" { 960 } else { 120 };
            let changed = sqlx::query("UPDATE staff_identity.sessions SET last_activity_at=clock_timestamp()-make_interval(secs => $2) WHERE account_id=(SELECT id FROM staff_identity.accounts WHERE username=$1)")
                .bind(board).bind(f64::from(seconds)).execute(&pool).await?;
            println!("{}", json!({"changed":changed.rows_affected()}));
        }
        "session-times" => {
            let times: Vec<(String, String, String)> = sqlx::query_as("SELECT authenticated_at::text,expires_at::text,last_activity_at::text FROM staff_identity.sessions WHERE account_id=(SELECT id FROM staff_identity.accounts WHERE username=$1) ORDER BY authenticated_at")
                .bind(board).fetch_all(&pool).await?;
            // No token, CSRF secret, credential or key leaves the fixture.
            println!("{}", json!(times));
        }
        "expire-ceremony" => {
            use std::io::Read;
            let mut hash = String::new();
            std::io::stdin().take(65).read_to_string(&mut hash)?;
            if hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                return Err("Invalid synthetic ceremony hash".into());
            }
            let changed=sqlx::query("UPDATE staff_identity.ceremonies SET expires_at=clock_timestamp()-interval '1 second' WHERE token_hash=decode($2,'hex') AND account_id=(SELECT id FROM staff_identity.accounts WHERE username=$1)").bind(board).bind(hash).execute(&pool).await?;
            println!("{}", json!({"expired":changed.rows_affected()}));
        }
        "expire" => {
            sqlx::query("UPDATE staff_identity.sessions SET expires_at=clock_timestamp()-interval '1 second' WHERE account_id=(SELECT id FROM staff_identity.accounts WHERE username=$1)").bind(board).execute(&pool).await?;
        }
        "cleanup" => {
            let mut tx = pool.begin().await?;
            sqlx::query("DELETE FROM staff_identity.sessions WHERE account_id=(SELECT id FROM staff_identity.accounts WHERE username=$1)").bind(board).execute(&mut *tx).await?;
            sqlx::query("DELETE FROM staff_identity.credentials WHERE account_id=(SELECT id FROM staff_identity.accounts WHERE username=$1)").bind(board).execute(&mut *tx).await?;
            sqlx::query("DELETE FROM staff_identity.invitations WHERE account_id=(SELECT id FROM staff_identity.accounts WHERE username=$1)").bind(board).execute(&mut *tx).await?;
            sqlx::query("DELETE FROM staff_identity.ceremonies WHERE account_id=(SELECT id FROM staff_identity.accounts WHERE username=$1)").bind(board).execute(&mut *tx).await?;
            sqlx::query("DELETE FROM staff_identity.accounts WHERE username=$1")
                .bind(board)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM content.moderation_audit WHERE board=$1")
                .bind(board)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM content.reports WHERE board=$1")
                .bind(board)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM content.posts WHERE board=$1")
                .bind(board)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM content.threads WHERE board=$1")
                .bind(board)
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM content.boards WHERE slug=$1")
                .bind(board)
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
        }
        _ => return Err("Unknown fixture command".into()),
    }
    Ok(())
}
