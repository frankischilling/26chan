#![forbid(unsafe_code)]
use sqlx::{Connection, PgConnection, postgres::PgConnectOptions};
use std::{ffi::OsString, process::ExitCode, str::FromStr};

const HELP: &str = "Usage: board-homepage refresh\n       board-homepage --help\n\nRefresh the bounded public homepage snapshot using MIGRATION_DATABASE_URL and\nthe actual board_migrator login. Generations publish atomically; if a commit\ncannot be confirmed, inspect the stored generation before retrying.\nRun every five minutes if fresh homepage content is wanted; this command does\nnot install a scheduler. Deleted, private, reported and spoiler posts are\nrechecked when read. No post bodies or raw visitor addresses are cached.";
#[derive(Debug, PartialEq, Eq)]
enum Command {
    Help,
    Refresh,
}
fn parse(args: &[OsString]) -> Result<Command, &'static str> {
    match args {
        [arg] if arg == "--help" || arg == "-h" => Ok(Command::Help),
        [arg] if arg == "refresh" => Ok(Command::Refresh),
        _ => Err(HELP),
    }
}
async fn run(args: Vec<OsString>) -> Result<(), String> {
    if parse(&args)? == Command::Help {
        println!("{HELP}");
        return Ok(());
    }
    let url = std::env::var("MIGRATION_DATABASE_URL")
        .map_err(|_| "MIGRATION_DATABASE_URL must be set.")?;
    let options = PgConnectOptions::from_str(&url)
        .map_err(|_| "Invalid migration connection configuration.")?;
    let mut connection = PgConnection::connect_with(&options)
        .await
        .map_err(|_| "Cannot connect to the migration database.")?;
    let generation = board_store::homepage::refresh(&mut connection)
        .await
        .map_err(|e| e.to_string())?;
    println!("Published homepage snapshot generation {generation}.");
    Ok(())
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
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn refresh_is_explicit_and_has_no_extra_arguments() {
        assert_eq!(parse(&["refresh".into()]), Ok(Command::Refresh));
        assert_eq!(parse(&["--help".into()]), Ok(Command::Help));
        for args in [
            vec![],
            vec!["Refresh".into()],
            vec!["refresh".into(), "now".into()],
        ] {
            assert!(parse(&args).is_err());
        }
    }
}
