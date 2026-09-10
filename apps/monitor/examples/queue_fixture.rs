//! Owned disposable-database transitions for tests/monitoring/queue_qualify.py.
//! This example is never a runtime or public endpoint.
use board_store::media::{Failure, MediaQueue};
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::{env, process::ExitCode, time::Duration};

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[tokio::main]
async fn main() -> ExitCode {
    if run().await.is_err() {
        eprintln!("owned queue qualification transition failed");
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

async fn run() -> Result<()> {
    let args: Vec<_> = env::args().skip(1).collect();
    if env::var("QUEUE_QUALIFICATION").as_deref() != Ok("owned-disposable")
        || args.len() != 2
        || !matches!(
            args[0].as_str(),
            "init"
                | "saturate"
                | "drain"
                | "fail"
                | "age-failures"
                | "revoke"
                | "restore"
                | "cleanup"
        )
        || args[1].len() != 32
        || !args[1]
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err("invalid fixture invocation".into());
    }
    let owner = env::var("MIGRATION_DATABASE_URL")?;
    let writer = env::var("MEDIA_DATABASE_URL")?;
    let owner_options: sqlx::postgres::PgConnectOptions = owner.parse()?;
    let writer_options: sqlx::postgres::PgConnectOptions = writer.parse()?;
    if owner_options.get_database() != Some("board_queue_qualification")
        || writer_options.get_database() != owner_options.get_database()
        || writer_options.get_host() != owner_options.get_host()
        || writer_options.get_port() != owner_options.get_port()
    {
        return Err("fixture requires the owned disposable database".into());
    }
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_secs(2))
        .connect_with(owner_options)
        .await?;
    let owned: bool = sqlx::query_scalar(
        "SELECT current_database() = 'board_queue_qualification' AND current_user = 'board_migrator'",
    ).fetch_one(&admin).await?;
    if !owned {
        return Err("fixture requires its migration owner".into());
    }
    let filename = format!("queue-monitor-{}", args[1]);
    // An owned, otherwise empty database is required even for cleanup. Every
    // row mutation below is additionally scoped to this generated fixture tag.
    let foreign: i64 = sqlx::query_scalar("SELECT count(*) FROM media.jobs WHERE filename <> $1")
        .bind(&filename)
        .fetch_one(&admin)
        .await?;
    if foreign != 0 {
        return Err("foreign jobs present in disposable qualification database".into());
    }
    let command = args[0].as_str();
    let result = exercise(command, &filename, &admin, &writer).await;
    admin.close().await;
    result?;
    println!("ok");
    Ok(())
}

async fn exercise(command: &str, filename: &str, admin: &PgPool, writer: &str) -> Result<()> {
    match command {
        "init" => {
            let mut tx = admin.begin().await?;
            let count: i64 = sqlx::query_scalar("SELECT count(*) FROM media.jobs")
                .fetch_one(&mut *tx)
                .await?;
            let capacity: i32 = sqlx::query_scalar(
                "SELECT capacity FROM media.queue_policy WHERE singleton FOR UPDATE",
            )
            .fetch_one(&mut *tx)
            .await?;
            if count != 0 || capacity != 64 {
                return Err("fixture requires a fresh default queue".into());
            }
            sqlx::query("UPDATE media.queue_policy SET capacity = 4 WHERE singleton")
                .execute(&mut *tx)
                .await?;
            tx.commit().await?;
        }
        "saturate" => {
            let queue = MediaQueue::connect(writer).await?;
            for _ in 0..4 {
                queue.reserve(filename).await?;
            }
            if !matches!(
                queue.reserve(filename).await,
                Err(board_store::StoreError::Conflict(_))
            ) {
                return Err("real queue admission did not reject a fifth reservation".into());
            }
        }
        "drain" => {
            let queue = MediaQueue::connect(writer).await?;
            let ids: Vec<String> = sqlx::query_scalar(
                "SELECT id FROM media.jobs WHERE filename = $1 AND state = 'receiving'",
            )
            .bind(filename)
            .fetch_all(admin)
            .await?;
            if ids.len() != 4 {
                return Err("expected four owned reservations".into());
            }
            for id in ids {
                queue.abort_intake(&id).await?;
            }
        }
        "fail" => {
            let queue = MediaQueue::connect(writer).await?;
            let job = queue.reserve(filename).await?;
            queue.queue(&job.id, 4).await?;
            let leased = queue
                .claim()
                .await?
                .ok_or("expected the owned queued job")?;
            if leased.id != job.id {
                return Err("claim selected an unexpected job".into());
            }
            queue
                .fail(
                    &job.id,
                    leased.lease_token.as_deref().ok_or("missing lease")?,
                    Failure::Processing,
                    false,
                )
                .await?;
            let failed = queue.get(&job.id).await?;
            if failed.state != "failed" || failed.failure.as_deref() != Some("processing_failed") {
                return Err("real queue processing failure was not persisted".into());
            }
        }
        "age-failures" => {
            let changed = sqlx::query("UPDATE media.jobs SET updated_at = statement_timestamp() - interval '16 minutes' WHERE filename = $1 AND state = 'failed'")
                .bind(filename).execute(admin).await?.rows_affected();
            if changed != 5 {
                return Err("expected five owned terminal fixture jobs".into());
            }
        }
        "revoke" => {
            sqlx::query("REVOKE SELECT ON monitoring.media_queue FROM board_monitor")
                .execute(admin)
                .await?;
        }
        "restore" => {
            sqlx::query("GRANT SELECT ON monitoring.media_queue TO board_monitor")
                .execute(admin)
                .await?;
        }
        "cleanup" => {
            let mut tx = admin.begin().await?;
            let capacity: i32 = sqlx::query_scalar(
                "SELECT capacity FROM media.queue_policy WHERE singleton FOR UPDATE",
            )
            .fetch_one(&mut *tx)
            .await?;
            if !matches!(capacity, 4 | 64) {
                return Err("unexpected queue policy; refusing fixture cleanup".into());
            }
            sqlx::query("GRANT SELECT ON monitoring.media_queue TO board_monitor")
                .execute(&mut *tx)
                .await?;
            sqlx::query("DELETE FROM media.jobs WHERE filename = $1")
                .bind(filename)
                .execute(&mut *tx)
                .await?;
            sqlx::query("UPDATE media.queue_policy SET capacity = 64 WHERE singleton")
                .execute(&mut *tx)
                .await?;
            let remaining: i64 =
                sqlx::query_scalar("SELECT count(*) FROM media.jobs WHERE filename = $1")
                    .bind(filename)
                    .fetch_one(&mut *tx)
                    .await?;
            if remaining != 0 {
                return Err("owned jobs remain after cleanup".into());
            }
            tx.commit().await?;
        }
        _ => return Err("unknown fixture command".into()),
    }
    Ok(())
}
