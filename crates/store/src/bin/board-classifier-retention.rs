#![forbid(unsafe_code)]

use sqlx::{Connection, PgConnection, postgres::PgConnectOptions};
use std::{ffi::OsString, process::ExitCode, str::FromStr, time::Duration};

const HELP: &str = "Usage: board-classifier-retention run

Requires MODERATION_RETENTION_DATABASE_URL with the board_moderation_retention
login and migration 0199. Permanently removes private classifier diagnostic rows
strictly older than seven days. Each transaction handles at most 1,000 rows;
one invocation handles at most 16 batches. Run every minute with the supplied
timer or an equivalent local scheduler. Locked or interrupted work remains for
the next run. Recent diagnostics and all classifier cooldown state remain.

This command cannot read diagnostic payloads or delete posts, media, anonymous
sessions or moderation audit snapshots. It prints only deletion counts. A
restored database must run expiry before operators inspect old diagnostics.";

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Help,
    Run,
}
fn parse(args: &[OsString]) -> Result<Command, &'static str> {
    match args {
        [value] if value == "--help" || value == "-h" => Ok(Command::Help),
        [value] if value == "run" => Ok(Command::Run),
        _ => Err("Use --help or run. Retention duration and cutoff are server-owned."),
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        run(std::env::args_os().skip(1).collect()),
    )
    .await;
    match result {
        Ok(Ok(())) => ExitCode::SUCCESS,
        Ok(Err(message)) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
        Err(_) => {
            eprintln!(
                "Retention deadline exceeded; completed batches remain committed. The next scheduled run can continue."
            );
            ExitCode::FAILURE
        }
    }
}

async fn run(args: Vec<OsString>) -> Result<(), &'static str> {
    if parse(&args)? == Command::Help {
        println!("{HELP}");
        return Ok(());
    }
    let database = std::env::var("MODERATION_RETENTION_DATABASE_URL")
        .map_err(|_| "MODERATION_RETENTION_DATABASE_URL must be set.")?;
    let options = PgConnectOptions::from_str(&database)
        .map_err(|_| "Invalid retention connection configuration.")?;
    let mut connection = PgConnection::connect_with(&options)
        .await
        .map_err(|_| "Cannot connect to the retention database.")?;
    let identity: bool = sqlx::query_scalar(
        "SELECT current_user=session_user AND current_user='board_moderation_retention'",
    )
    .fetch_one(&mut connection)
    .await
    .map_err(|_| "Cannot verify retention identity.")?;
    if !identity {
        return Err("Retention requires the dedicated board_moderation_retention login.");
    }
    let mut expired = 0_i64;
    let mut reached_limit = false;
    for batch in 0..16 {
        let mut tx = connection
            .begin()
            .await
            .map_err(|_| "Cannot begin retention transaction.")?;
        sqlx::raw_sql("SET TRANSACTION ISOLATION LEVEL READ COMMITTED; SET LOCAL statement_timeout='5s'; SET LOCAL lock_timeout='2s'; SET LOCAL idle_in_transaction_session_timeout='5s'")
            .execute(&mut *tx).await.map_err(|_| "Cannot bound retention transaction.")?;
        let count: i64 = sqlx::query_scalar("SELECT content.expire_classifier_events()")
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| "Retention refused; verify schema, permissions and database health.")?;
        if !(0..=1000).contains(&count) {
            return Err("Unexpected retention result; transaction rolled back.");
        }
        tx.commit().await.map_err(
            |_| "Retention commit outcome is uncertain; the next scheduled run can safely recheck.",
        )?;
        expired += count;
        reached_limit = batch == 15 && count == 1000;
        if count < 1000 {
            break;
        }
    }
    connection
        .close()
        .await
        .map_err(|_| "Connection close failed after committed retention work.")?;
    println!("{{\"expired_classifier_events\":{expired},\"batch_limit_reached\":{reached_limit}}}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn operator_cannot_supply_ids_deadlines_or_unbounded_work() {
        for input in [
            vec![],
            vec!["run", "30"],
            vec!["run", "--all"],
            vec!["run", "--cutoff", "2100-01-01"],
            vec!["preview"],
        ] {
            assert!(parse(&input.into_iter().map(OsString::from).collect::<Vec<_>>()).is_err());
        }
        assert_eq!(parse(&["run".into()]), Ok(Command::Run));
        assert_eq!(parse(&["--help".into()]), Ok(Command::Help));
    }
}
