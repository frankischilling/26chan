#![forbid(unsafe_code)]

use board_config::MediaAdminSettings;
use board_media::{ObjectId, Quarantine};
use board_store::media::MediaQueue;
use std::{path::Path, time::Duration};

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), String> {
    let settings = MediaAdminSettings::from_env().map_err(|error| error.to_string())?;
    let args: Vec<_> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [command, _, _] if command == "intake" => {}
        [command, id] if command == "status" => { id.parse::<ObjectId>().map_err(|_| "Invalid object ID.")?; }
        [command] if command == "cleanup" => {}
        _ => return Err("Usage: board-media-admin intake INPUT_FILE DISPLAY_FILENAME | status OBJECT_ID | cleanup".into()),
    }
    let queue = MediaQueue::connect(&settings.database_url)
        .await
        .map_err(|_| "Media database unavailable or role unsafe.")?;
    match args.as_slice() {
        [command, input, filename] if command == "intake" => {
            let quarantine = quarantine(&settings)?;
            intake(&queue, &quarantine, Path::new(input), filename).await?;
        }
        [command, id] if command == "status" => {
            let job = queue.get(id).await.map_err(|_| "Media job unavailable.")?;
            println!(
                "{}",
                serde_json::json!({ "id": job.id, "state": job.state, "input_bytes": job.input_bytes, "attempts": job.attempts, "failure": job.failure })
            );
        }
        [command] if command == "cleanup" => {
            let quarantine = quarantine(&settings)?;
            let expired = queue
                .expire()
                .await
                .map_err(|_| "Queue expiration failed.")?;
            let candidates = queue
                .cleanup_candidates()
                .await
                .map_err(|_| "Queue cleanup unavailable.")?;
            let mut removed = 0;
            for job in candidates {
                let id: ObjectId = job.id.parse().map_err(|_| "Invalid stored object ID.")?;
                quarantine
                    .remove(id)
                    .map_err(|_| "Private file cleanup failed; metadata retained.")?;
                removed += usize::from(
                    queue
                        .forget_terminal(&job.id)
                        .await
                        .map_err(|_| "Metadata cleanup failed; retry is safe.")?,
                );
            }
            println!(
                "{}",
                serde_json::json!({ "expired": expired, "removed": removed, "batch_limit": 64 })
            );
        }
        _ => unreachable!("validated command"),
    }
    Ok(())
}

fn quarantine(settings: &MediaAdminSettings) -> Result<Quarantine, String> {
    let root = settings
        .quarantine
        .as_ref()
        .ok_or("MEDIA_QUARANTINE_DIR is required.")?;
    Quarantine::new(root).map_err(|_| "Private quarantine root is unavailable or unsafe.".into())
}

async fn intake(
    queue: &MediaQueue,
    quarantine: &Quarantine,
    input: &Path,
    filename: &str,
) -> Result<(), String> {
    let metadata = tokio::fs::metadata(input)
        .await
        .map_err(|_| "Input file is unavailable.")?;
    if !metadata.is_file() {
        return Err("Input must be a regular operator-selected file.".into());
    }
    let file = tokio::fs::File::open(input)
        .await
        .map_err(|_| "Input file is unavailable.")?;
    let job = queue
        .reserve(filename)
        .await
        .map_err(|error| error.to_string())?;
    let id: ObjectId = job.id.parse().map_err(|_| "Invalid generated object ID.")?;
    let bytes =
        match tokio::time::timeout(Duration::from_secs(15), quarantine.receive(id, file)).await {
            Ok(Ok(bytes)) => bytes,
            _ => {
                // If the database is unavailable, the finite reservation expires on reconciliation.
                let _ = queue.abort_intake(&job.id).await;
                return Err(
                    "Upload failed or exceeded its byte/time limit; no input was published.".into(),
                );
            }
        };
    // On an uncertain database commit, preserve the private object for reconciliation.
    queue
        .queue(&job.id, bytes)
        .await
        .map_err(|_| "Queue finalization unavailable; private input retained for cleanup.")?;
    println!(
        "{}",
        serde_json::json!({ "id": job.id, "state": "queued", "input_bytes": bytes })
    );
    Ok(())
}
