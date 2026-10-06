#![forbid(unsafe_code)]

use board_store::report_catalog::{import_catalog, read_catalog};
use sqlx::{Connection, PgConnection, postgres::PgConnectOptions};
use std::{ffi::OsString, fs::File, process::ExitCode, str::FromStr};

const HELP: &str = "Usage: board-report-catalog validate <local-file.json>
       board-report-catalog import <local-file.json>

validate checks an explicit version-1 envelope offline, with no database access.
import requires MIGRATION_DATABASE_URL using the actual board_migrator login.
It appends a private immutable revision and verifies readback before committing.
No command activates a catalog or changes runtime report admission.

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

async fn run(args: Vec<OsString>) -> Result<(), String> {
    if args.len() == 1 && (args[0] == "--help" || args[0] == "-h") {
        println!("{HELP}");
        return Ok(());
    }
    if args.len() != 2 || (args[0] != "validate" && args[0] != "import") {
        return Err(format!("Invalid command.\n{HELP}"));
    }
    let file = File::open(&args[1]).map_err(|_| "Cannot open catalog input file.")?;
    let catalog = read_catalog(file).map_err(|error| error.to_string())?;
    if args[0] == "validate" {
        println!(
            "Valid version-1 catalog: {} categories. No database accessed.",
            catalog.len()
        );
        return Ok(());
    }
    let database = std::env::var("MIGRATION_DATABASE_URL")
        .map_err(|_| "MIGRATION_DATABASE_URL must be set for import.")?;
    // Do not render URL parsing, connection, or SQL error details: these can
    // contain credentials or private server information.
    let options = PgConnectOptions::from_str(&database)
        .map_err(|_| "Invalid migration database connection configuration.")?;
    let mut connection = PgConnection::connect_with(&options)
        .await
        .map_err(|_| "Cannot connect to the migration database.")?;
    let revision = import_catalog(&mut connection, &catalog)
        .await
        .map_err(|error| error.to_string())?;
    println!(
        "Imported and verified private catalog revision {revision}: {} categories. No activation performed.",
        catalog.len()
    );
    Ok(())
}
