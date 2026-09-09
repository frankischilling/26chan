#![forbid(unsafe_code)]

use board_config::MediaAdminSettings;
use board_media::{OUTPUT_DISK_BYTES, ObjectId, PublicationStore, Quarantine, ValidatedOutput};
use board_media_admin::{PublicationResult, publish, reconcile};
use board_store::media::MediaQueue;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::Path,
    time::Duration,
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LeaseManifest {
    job_id: String,
    lease_token: String,
}

#[tokio::main]
async fn main() {
    if run().await.is_err() {
        eprintln!("media publication command rejected; inspect private state before retrying");
        std::process::exit(1);
    }
}

async fn run() -> PublicationResult<()> {
    if std::env::var("APP_ENV").as_deref() != Ok("development") {
        return Err("explicit development mode required".into());
    }
    let settings = MediaAdminSettings::from_env()?;
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    match args.first().and_then(|arg| arg.to_str()) {
        Some("claim") if args.len() == 2 => {
            let path = Path::new(&args[1]);
            let mut options = OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)] {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            // Create exclusively before claiming: an existing operator manifest
            // must never be overwritten. A crash may leave an invalid empty file.
            let mut file = options.open(path)?;
            let queue = MediaQueue::connect(&settings.database_url).await?;
            let job = queue.claim().await?.ok_or("no queued job available")?;
            let manifest = LeaseManifest { job_id: job.id, lease_token: job.lease_token.ok_or("missing lease")? };
            file.write_all(&serde_json::to_vec(&manifest)?)?;
            file.sync_all()?;
        }
        Some("publish") if args.len() == 4 => {
            let manifest = read_manifest(Path::new(&args[1]))?;
            let disk = Path::new(&args[2]);
            let metadata = fs::symlink_metadata(disk)?;
            if !metadata.is_file() || metadata.len() != OUTPUT_DISK_BYTES { return Err("invalid stopped output disk".into()); }
            let file = tokio::fs::File::open(disk).await?;
            let output = tokio::time::timeout(Duration::from_secs(5), ValidatedOutput::read_disk(file)).await??;
            let quarantine = Quarantine::new(settings.quarantine.ok_or("private quarantine path required")?)?;
            let store = PublicationStore::new(Path::new(&args[3]), &quarantine)?;
            let queue = MediaQueue::connect(&settings.database_url).await?;
            let approved = publish(&queue, &store, &manifest.job_id, &manifest.lease_token, &output).await?;
            // Only an approved opaque output ID is printed, never a lease token.
            println!("{}", approved.id);
        }
        Some("reconcile") if args.len() == 2 => {
            let quarantine = Quarantine::new(settings.quarantine.ok_or("private quarantine path required")?)?;
            let store = PublicationStore::new(Path::new(&args[1]), &quarantine)?;
            let queue = MediaQueue::connect(&settings.database_url).await?;
            println!("{}", reconcile(&queue, &store).await?);
        }
        _ => return Err("usage: media-publish claim LEASE_FILE | publish LEASE_FILE OUTPUT_DISK PRIVATE_STORE | reconcile PRIVATE_STORE".into()),
    }
    Ok(())
}

fn read_manifest(path: &Path) -> PublicationResult<LeaseManifest> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.len() > 512 {
        return Err("invalid operator lease file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("operator lease file must be private".into());
        }
    }
    let mut bytes = Vec::new();
    fs::File::open(path)?.take(513).read_to_end(&mut bytes)?;
    if bytes.len() > 512 {
        return Err("operator lease file exceeds limit".into());
    }
    let manifest: LeaseManifest = serde_json::from_slice(&bytes)?;
    manifest.job_id.parse::<ObjectId>()?;
    manifest.lease_token.parse::<ObjectId>()?;
    Ok(manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_operator_manifest_rejects_paths_unknown_fields_and_large_input() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("lease.json");
        let good = serde_json::json!({"job_id": "a".repeat(32), "lease_token": "b".repeat(32)});
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))
                .unwrap();
        }
        file.write_all(good.to_string().as_bytes()).unwrap();
        drop(file);
        assert!(read_manifest(&path).is_ok());
        for value in [
            serde_json::json!({"job_id": "../other", "lease_token": "b".repeat(32)}),
            serde_json::json!({"job_id": "a".repeat(32), "lease_token": "b".repeat(32), "path": "untrusted"}),
        ] {
            fs::write(&path, value.to_string()).unwrap();
            assert!(read_manifest(&path).is_err());
        }
        fs::write(&path, vec![b' '; 513]).unwrap();
        assert!(read_manifest(&path).is_err());
    }
}
