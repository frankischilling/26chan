#![forbid(unsafe_code)]

use board_store::report_catalog::{
    CatalogRevision, import_catalog, read_catalog, set_report_catalog_active,
};
use sqlx::{Connection, PgConnection, postgres::PgConnectOptions};
use std::{ffi::OsString, fs::File, process::ExitCode, str::FromStr};

const HELP: &str = "Usage: board-report-catalog validate <local-file.json>
       board-report-catalog import <local-file.json>
       board-report-catalog activate <revision>
       board-report-catalog deactivate

validate checks an explicit version-1 envelope offline, with no database access.
import requires MIGRATION_DATABASE_URL using the actual board_migrator login.
It appends a private immutable revision and verifies readback before committing.
Import alone does not activate a catalog or change runtime report admission.
activate explicitly switches reports to an existing nonempty imported revision.
WARNING: Activation switches report admission to imported categories; old
free-text report requests fail while categorical mode is active.
deactivate restores free-text report admission without deleting any revision.
Both commands require MIGRATION_DATABASE_URL and the actual board_migrator login.
Revision must be a canonical positive i64 decimal: no sign, whitespace, or leading
zeros. Obtain actual category data from the operator; no production source or
category values are supplied by this tool.

Envelope: {\"version\":1,\"categories\":[...]}
Each row must explicitly supply all nine keys: id, board, op_only, reply_only,
image_only, exclude_boards, title, weight, filtered. Unknown or duplicate keys
are errors. board and exclude_boards accept null; flags are booleans. IDs are
positive i64 integers; filtered is an i64 integer; weight is a finite number.
Order, exact text, empty strings, and null are preserved. No rows are supplied.

Deployment safety bounds (not legacy source limits): 8 MiB input, 4096 rows,
256 UTF-8 bytes for board, 4096 for title, 65536 for exclude_boards; no U+0000.
SQL additionally limits normalized JSONB text to 8 MiB and keeps at most 64
revisions. Offline validation does not guarantee remaining database capacity.";

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

#[derive(Debug, Eq, PartialEq)]
enum Command {
    Help,
    Validate(OsString),
    Import(OsString),
    Activate(CatalogRevision),
    Deactivate,
}

fn parse_command(args: Vec<OsString>) -> Result<Command, String> {
    match args.as_slice() {
        [command] if command == "--help" || command == "-h" => Ok(Command::Help),
        [command] if command == "deactivate" => Ok(Command::Deactivate),
        [command, path] if command == "validate" => Ok(Command::Validate(path.clone())),
        [command, path] if command == "import" => Ok(Command::Import(path.clone())),
        [command, revision] if command == "activate" => {
            let revision = revision
                .to_str()
                .filter(|text| {
                    !text.is_empty()
                        && !text.starts_with('0')
                        && text.bytes().all(|byte| byte.is_ascii_digit())
                })
                .and_then(|text| text.parse::<i64>().ok())
                .and_then(CatalogRevision::new)
                .ok_or("Revision must be a canonical positive i64 decimal.")?;
            Ok(Command::Activate(revision))
        }
        _ => Err(format!("Invalid command.\n{HELP}")),
    }
}

async fn connect_database(command: &str) -> Result<PgConnection, String> {
    let database = std::env::var("MIGRATION_DATABASE_URL")
        .map_err(|_| format!("MIGRATION_DATABASE_URL must be set for {command}."))?;
    // Do not render URL parsing, connection, or SQL error details: these can
    // contain credentials or private server information.
    let options = PgConnectOptions::from_str(&database)
        .map_err(|_| "Invalid migration database connection configuration.")?;
    PgConnection::connect_with(&options)
        .await
        .map_err(|_| "Cannot connect to the migration database.".to_owned())
}

async fn run(args: Vec<OsString>) -> Result<(), String> {
    match parse_command(args)? {
        Command::Help => println!("{HELP}"),
        Command::Validate(path) => {
            let file = File::open(path).map_err(|_| "Cannot open catalog input file.")?;
            let catalog = read_catalog(file).map_err(|error| error.to_string())?;
            println!(
                "Valid version-1 catalog: {} categories. No database accessed.",
                catalog.len()
            );
        }
        Command::Import(path) => {
            // Validate the original bounded file before accessing the database.
            let file = File::open(path).map_err(|_| "Cannot open catalog input file.")?;
            let catalog = read_catalog(file).map_err(|error| error.to_string())?;
            let mut connection = connect_database("import").await?;
            let revision = import_catalog(&mut connection, &catalog)
                .await
                .map_err(|error| error.to_string())?;
            println!(
                "Imported and verified private catalog revision {revision}: {} categories. No activation performed.",
                catalog.len()
            );
        }
        Command::Activate(revision) => {
            let mut connection = connect_database("activate").await?;
            set_report_catalog_active(&mut connection, Some(revision))
                .await
                .map_err(|error| error.to_string())?;
            println!(
                "Activated catalog revision {}. Reports now require imported categories; old free-text requests fail.",
                revision.get()
            );
        }
        Command::Deactivate => {
            let mut connection = connect_database("deactivate").await?;
            set_report_catalog_active(&mut connection, None)
                .await
                .map_err(|error| error.to_string())?;
            println!("Deactivated categorical report admission. Free-text reporting restored.");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Command, String> {
        parse_command(args.iter().copied().map(OsString::from).collect())
    }

    #[test]
    fn existing_commands_and_paths_are_preserved() {
        assert_eq!(parse(&["-h"]).unwrap(), Command::Help);
        assert_eq!(parse(&["--help"]).unwrap(), Command::Help);
        assert_eq!(
            parse(&["validate", " file.json "]).unwrap(),
            Command::Validate(OsString::from(" file.json "))
        );
        assert_eq!(
            parse(&["import", "file.json"]).unwrap(),
            Command::Import(OsString::from("file.json"))
        );
    }

    #[test]
    fn activation_accepts_only_canonical_positive_i64() {
        for value in ["1", "9223372036854775807"] {
            let expected = CatalogRevision::new(value.parse().unwrap()).unwrap();
            assert_eq!(
                parse(&["activate", value]).unwrap(),
                Command::Activate(expected)
            );
        }
        for value in [
            "",
            "0",
            "00",
            "01",
            "+1",
            "-1",
            " 1",
            "1 ",
            "1\n",
            "1.0",
            "1e1",
            "9223372036854775808",
            "18446744073709551615",
            "１２",
            "null",
        ] {
            assert!(parse(&["activate", value]).is_err(), "{value:?}");
        }
        assert_eq!(parse(&["deactivate"]).unwrap(), Command::Deactivate);
    }

    #[test]
    fn rejects_wrong_arity_and_unknown_commands() {
        for args in [
            vec![],
            vec!["activate"],
            vec!["activate", "1", "extra"],
            vec!["deactivate", "1"],
            vec!["validate"],
            vec!["import"],
            vec!["import", "file.json", "activate"],
            vec!["--help", "extra"],
            vec!["activate-all"],
        ] {
            assert!(parse(&args).is_err(), "{args:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn rejects_non_utf8_revision_but_preserves_non_utf8_file_paths() {
        use std::os::unix::ffi::OsStringExt;
        let value = OsString::from_vec(vec![0xff]);
        assert!(parse_command(vec![OsString::from("activate"), value.clone()]).is_err());
        assert_eq!(
            parse_command(vec![OsString::from("validate"), value.clone()]).unwrap(),
            Command::Validate(value)
        );
    }

    #[test]
    fn rejected_revision_does_not_echo_input() {
        let error = parse(&["activate", "secret-database-url"]).unwrap_err();
        assert!(!error.contains("secret-database-url"));
    }
}
