#![forbid(unsafe_code)]
use board_store::events::admin;
use sqlx::{Connection, PgConnection, postgres::PgConnectOptions};
use std::{ffi::OsString, fs::File, process::ExitCode, str::FromStr};
const HELP: &str = "Usage: board-events validate <profile.json>\n       board-events start <profile.json>\n       board-events quote <quotes.json>\n       board-events close <run-id>\n\nvalidate is offline. Other commands require MIGRATION_DATABASE_URL and the\nactual board_migrator login. start creates a new run; it does not replace an\nenabled run. close retains all accounts. Reset by closing and starting a new\nrun. Event balances are fictional. See docs/source-events.md for JSON fields.";
fn id(value: &std::ffi::OsStr) -> Result<i64, String> {
    value
        .to_str()
        .filter(|v| !v.is_empty() && !v.starts_with('0') && v.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|v| v.parse().ok())
        .filter(|v| *v > 0)
        .ok_or_else(|| "Invalid event run ID.".into())
}
async fn database() -> Result<PgConnection, String> {
    let raw = std::env::var("MIGRATION_DATABASE_URL")
        .map_err(|_| "MIGRATION_DATABASE_URL must be set.")?;
    let options = PgConnectOptions::from_str(&raw)
        .map_err(|_| "Invalid migration connection configuration.")?;
    PgConnection::connect_with(&options)
        .await
        .map_err(|_| "Cannot connect to the migration database.".into())
}
async fn run(args: Vec<OsString>) -> Result<(), String> {
    match args.as_slice() {
        [command] if command == "--help" || command == "-h" => println!("{HELP}"),
        [command, path] if command == "validate" || command == "start" => {
            let file = File::open(path).map_err(|_| "Cannot open event profile.")?;
            let profile = admin::read_profile(file).map_err(|error| error.to_string())?;
            if command == "validate" {
                println!("Valid event profile. No database accessed.");
            } else {
                let id = admin::start(&mut database().await?, &profile)
                    .await
                    .map_err(|error| error.to_string())?;
                println!("Configured event run {id}.");
            }
        }
        [command, path] if command == "quote" => {
            let file = File::open(path).map_err(|_| "Cannot open event quotes.")?;
            let input = admin::read_quotes(file).map_err(|error| error.to_string())?;
            admin::quote(&mut database().await?, &input)
                .await
                .map_err(|error| error.to_string())?;
            println!("Updated event quotes.");
        }
        [command, value] if command == "close" => {
            let id = id(value)?;
            let changed = admin::close(&mut database().await?, id)
                .await
                .map_err(|error| error.to_string())?;
            println!(
                "{}",
                if changed {
                    "Event run closed; accounts retained."
                } else {
                    "No enabled matching run."
                }
            );
        }
        _ => return Err(format!("Invalid command.\n{HELP}")),
    }
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
    fn run_ids_are_canonical() {
        for bad in ["", "0", "01", "+1", "-1", " 1", "9223372036854775808"] {
            assert!(id(bad.as_ref()).is_err());
        }
        assert_eq!(id("1".as_ref()).unwrap(), 1);
    }
}
