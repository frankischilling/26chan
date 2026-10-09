#![forbid(unsafe_code)]
use board_store::blotter::{publish_blotter, read_blotter, retract_blotter};
use sqlx::{Connection, PgConnection, postgres::PgConnectOptions};
use std::{ffi::OsString, fs::File, process::ExitCode, str::FromStr};

const HELP: &str = "Usage: board-blotter validate <local-file.json>\n       board-blotter publish <local-file.json>\n       board-blotter retract <id>\n\nvalidate is offline. publish and retract require MIGRATION_DATABASE_URL and the\nactual board_migrator login. No runtime role can maintain announcements.\nEnvelope: {\"version\":1,\"published_at\":<Unix seconds>,\"content\":\"plain text\"}\nInput: at most 32768 bytes. Content: 1..8192 UTF-8 bytes, no controls except LF/tab.\nUnix seconds: 1..253402300799, strictly newer than every retained message.\nPublishing is immediately public. IDs are allocated monotonically, with at most\n10000 retained entries. Corrections append a new entry; retract hides an old one\nwithout deleting its history. HTML and URLs render as text. No source content is\nsupplied. Cursor and timestamps use decimal integers; IDs must be canonical.";

#[derive(Debug, PartialEq, Eq)]
enum Command {
    Help,
    Validate(OsString),
    Publish(OsString),
    Retract(i64),
}
fn parse(args: Vec<OsString>) -> Result<Command, String> {
    match args.as_slice() {
        [command] if command == "--help" || command == "-h" => Ok(Command::Help),
        [command, file] if command == "validate" => Ok(Command::Validate(file.clone())),
        [command, file] if command == "publish" => Ok(Command::Publish(file.clone())),
        [command, id] if command == "retract" => {
            let id = id
                .to_str()
                .filter(|id| {
                    !id.is_empty() && !id.starts_with('0') && id.bytes().all(|c| c.is_ascii_digit())
                })
                .and_then(|id| id.parse::<i64>().ok())
                .filter(|id| (1..=10000).contains(id))
                .ok_or("Invalid announcement ID.")?;
            Ok(Command::Retract(id))
        }
        _ => Err(format!("Invalid command.\n{HELP}")),
    }
}
async fn database() -> Result<PgConnection, String> {
    let url = std::env::var("MIGRATION_DATABASE_URL")
        .map_err(|_| "MIGRATION_DATABASE_URL must be set.")?;
    let options = PgConnectOptions::from_str(&url)
        .map_err(|_| "Invalid migration connection configuration.")?;
    PgConnection::connect_with(&options)
        .await
        .map_err(|_| "Cannot connect to the migration database.".into())
}
async fn run(args: Vec<OsString>) -> Result<(), String> {
    match parse(args)? {
        Command::Help => println!("{HELP}"),
        Command::Validate(path) => {
            let file = File::open(path).map_err(|_| "Cannot open announcement input.")?;
            read_blotter(file).map_err(|error| error.to_string())?;
            println!("Valid version-1 plain-text announcement. No database accessed.");
        }
        Command::Publish(path) => {
            let file = File::open(path).map_err(|_| "Cannot open announcement input.")?;
            let input = read_blotter(file).map_err(|error| error.to_string())?;
            let mut connection = database().await?;
            let id = publish_blotter(&mut connection, &input)
                .await
                .map_err(|error| error.to_string())?;
            println!("Published and verified local announcement {id}.");
        }
        Command::Retract(id) => {
            let mut connection = database().await?;
            retract_blotter(&mut connection, id)
                .await
                .map_err(|error| error.to_string())?;
            println!("Retracted local announcement {id}. History retained.");
        }
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
    fn canonical_commands() {
        for id in [
            "0",
            "01",
            "+1",
            "-1",
            " 1",
            "1 ",
            "10001",
            "9223372036854775808",
        ] {
            assert!(parse(vec!["retract".into(), id.into()]).is_err());
        }
        assert_eq!(
            parse(vec!["retract".into(), "10000".into()]).unwrap(),
            Command::Retract(10000)
        );
        assert!(parse(vec!["delete".into(), "1".into()]).is_err());
    }
}
