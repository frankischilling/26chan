#![forbid(unsafe_code)]

use board_store::historical_erasure::cleanup;
use sqlx::{Connection, PgConnection, postgres::PgConnectOptions};
use std::{ffi::OsString, process::ExitCode, str::FromStr, time::Duration};

const HELP: &str = "Usage: board-deletion-cleanup preview <board>
       board-deletion-cleanup apply <board> --confirm-permanent-erasure <receipt> --writers-stopped

Requires MIGRATION_DATABASE_URL with the actual board_migrator login and
migration 0141. Stop application writers and schema changes before cleanup.
Preview changes no rows. Review its database, board, IDs and receipt before apply.
Apply recomputes that selection under the board lock; a changed preview refuses.
Each invocation handles at most 64 reports, 64 posts and 64 empty thread records.
Reports are drained before posts, and posts before parent metadata. Repeat with
a new reviewed preview until all three ID lists are empty; there is no auto-loop.

WARNING: Apply permanently removes already-deleted post payload, its private
author authority and obsolete reports, including report evidence and snapshots.
It does not delete live content, independent staff audit records, media files,
structural attachment tombstones, shared sessions, quotas, logs or backups.
Use the existing media reconciliation command separately for physical outputs.
This is not secure disk erasure. Restore must preserve subsequent deletions.
The receipt checks state; it is not an authorization token or a backup.";

#[derive(Debug, Eq, PartialEq)]
enum Command {
    Help,
    Preview(String),
    Apply(String, String),
}

fn parse(args: &[OsString]) -> Result<Command, &'static str> {
    let text: Option<Vec<_>> = args.iter().map(|value| value.to_str()).collect();
    let text = text.ok_or("Arguments must be valid text.")?;
    let command = match text.as_slice() {
        ["--help" | "-h"] => Command::Help,
        ["preview", board] => Command::Preview((*board).to_owned()),
        [
            "apply",
            board,
            "--confirm-permanent-erasure",
            receipt,
            "--writers-stopped",
        ] if receipt.len() == 64
            && receipt
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) =>
        {
            Command::Apply((*board).to_owned(), (*receipt).to_owned())
        }
        _ => {
            return Err(
                "Invalid command. Use --help; apply requires a reviewed receipt and stopped writers.",
            );
        }
    };
    if let Command::Preview(board) | Command::Apply(board, _) = &command {
        board_domain::BoardSlug::parse(board).map_err(|_| "Invalid canonical board.")?;
    }
    Ok(command)
}

#[tokio::main]
async fn main() -> ExitCode {
    match run(std::env::args_os().skip(1).collect()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(args: Vec<OsString>) -> Result<(), String> {
    let command = parse(&args)?;
    let (board, receipt) = match command {
        Command::Help => {
            println!("{HELP}");
            return Ok(());
        }
        Command::Preview(board) => (board, None),
        Command::Apply(board, receipt) => (board, Some(receipt)),
    };
    let database = std::env::var("MIGRATION_DATABASE_URL")
        .map_err(|_| "MIGRATION_DATABASE_URL must be set.")?;
    // Connection and SQL diagnostics may contain credentials or deleted data.
    let options = PgConnectOptions::from_str(&database)
        .map_err(|_| "Invalid migration database connection configuration.")?;
    let mut connection = tokio::time::timeout(
        Duration::from_secs(10),
        PgConnection::connect_with(&options),
    )
    .await
    .map_err(|_| "Database connection timed out.")?
    .map_err(|_| "Cannot connect to the migration database.")?;
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        cleanup(&mut connection, &board, receipt.as_deref()),
    )
    .await
    .map_err(|_| "Cleanup timed out; confirm database transaction outcome before retrying.")?
    .map_err(|error| error.to_string())?;
    println!(
        "{}",
        serde_json::to_string(&result).map_err(|_| "Cannot format cleanup result.")?
    );
    connection
        .close()
        .await
        .map_err(|_| "Connection close failed after the reported result.")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn command(args: &[&str]) -> Result<Command, &'static str> {
        parse(&args.iter().map(OsString::from).collect::<Vec<_>>())
    }
    #[test]
    fn apply_requires_both_explicit_safety_arguments_and_a_canonical_receipt() {
        let receipt = "ab".repeat(32);
        assert_eq!(
            command(&["preview", "test"]).unwrap(),
            Command::Preview("test".into())
        );
        assert_eq!(
            command(&[
                "apply",
                "test",
                "--confirm-permanent-erasure",
                &receipt,
                "--writers-stopped"
            ])
            .unwrap(),
            Command::Apply("test".into(), receipt.clone())
        );
        for args in [
            vec!["apply", "test"],
            vec!["apply", "test", "--confirm-permanent-erasure", &receipt],
            vec![
                "apply",
                "test",
                "--confirm-permanent-erasure",
                "X",
                "--writers-stopped",
            ],
            vec!["preview", "../test"],
            vec!["preview", "test", "extra"],
        ] {
            assert!(command(&args).is_err());
        }
    }
}
