#![forbid(unsafe_code)]

use board_media::{ObjectId, Promoter, Quarantine, ValidatedOutput};
use std::path::PathBuf;

/// Operator validation of a stopped guest's disk into a private result folder.
/// No database or queue publication authority is involved in this command.
#[tokio::main]
async fn main() {
    if run().await.is_err() {
        eprintln!("media output rejected; no approved result produced");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 2 || std::env::var("APP_ENV").as_deref() == Ok("production") {
        return Err("usage: media-validate OUTPUT_DISK PRIVATE_RESULT_DIRECTORY".into());
    }
    let disk = std::fs::canonicalize(PathBuf::from(&args[0]))?;
    let reader = tokio::fs::File::open(&disk).await?;
    if !reader.metadata().await?.is_file() {
        return Err("expected a regular stopped output disk".into());
    }
    let output = ValidatedOutput::read_disk(reader).await?;
    let quarantine = Quarantine::new(disk.parent().ok_or("missing parent")?)?;
    let promoter = Promoter::new(PathBuf::from(&args[1]), &quarantine)?;
    let id = ObjectId::generate()?;
    let promotion = promoter.promote(id, &output)?;
    println!(
        "{}",
        serde_json::json!({"id": id.to_string(), "width": output.dimensions().0,
        "height": output.dimensions().1, "sha256": promotion.sha256, "bytes": promotion.bytes})
    );
    Ok(())
}
