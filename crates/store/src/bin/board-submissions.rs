#![forbid(unsafe_code)]
use board_store::site_submissions::admin;
use sqlx::{Connection, PgConnection, postgres::PgConnectOptions};
use std::{ffi::OsString, fs::File, process::ExitCode, str::FromStr};
const HELP: &str = "Usage: board-submissions validate-feedback <profile.json>\n       board-submissions open-feedback <profile.json>\n       board-submissions close-feedback <profile-id>\n\nValidation is offline. Writes require MIGRATION_DATABASE_URL and the actual\nboard_migrator login. Closing retains submissions. See docs/source-site-submissions.md.";
fn id(value: &std::ffi::OsStr) -> Result<i64, String> {
    value
        .to_str()
        .filter(|v| !v.is_empty() && !v.starts_with('0') && v.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|v| v.parse().ok())
        .filter(|v| *v > 0)
        .ok_or_else(|| "Invalid feedback profile ID.".into())
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
        [command, path] if command == "validate-feedback" || command == "open-feedback" => {
            let file = File::open(path).map_err(|_| "Cannot open feedback profile.")?;
            let profile = admin::read_feedback(file).map_err(|error| error.to_string())?;
            if command == "validate-feedback" {
                println!("Valid feedback profile. No database accessed.");
            } else {
                let id = admin::start_feedback(&mut database().await?, &profile)
                    .await
                    .map_err(|error| error.to_string())?;
                println!("Configured feedback profile {id}.");
            }
        }
        [command, value] if command == "close-feedback" => {
            let id = id(value)?;
            let changed = admin::close_feedback(&mut database().await?, id)
                .await
                .map_err(|error| error.to_string())?;
            println!(
                "{}",
                if changed {
                    "Feedback closed; submissions retained."
                } else {
                    "No enabled matching profile."
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
    fn profile_ids_are_canonical() {
        for bad in ["", "0", "01", "+1", "-1", " 1", "9223372036854775808"] {
            assert!(id(bad.as_ref()).is_err());
        }
        assert_eq!(id("1".as_ref()).unwrap(), 1);
    }
}
