#![forbid(unsafe_code)]

use board_config::MediaBackfillSettings;
use board_media::{ApprovedFiles, ObjectId, PublicationStore, Quarantine};
use board_media_admin::{PublicationResult, backfill::backfill};
use board_media_dispatch::{ClientSettings, DispatchClient};
use board_store::legacy_media::LegacyMediaStore;
use std::{path::Path, time::Duration};

#[tokio::main]
async fn main() {
    match tokio::time::timeout(Duration::from_secs(45), run()).await {
        Ok(Ok(changed)) => println!(
            "{}",
            if changed {
                "legacy manifest upgraded"
            } else {
                "manifest already current; files verified"
            }
        ),
        _ => {
            eprintln!(
                "legacy media upgrade rejected; keep existing files and inspect private state before retrying"
            );
            std::process::exit(1);
        }
    }
}

async fn run() -> PublicationResult<bool> {
    let settings = MediaBackfillSettings::from_env()?;
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 4 {
        return Err(
            "usage: media-backfill CLIENT_CONFIG APPROVED_STORE QUARANTINE ASSET_ID".into(),
        );
    }
    let id: ObjectId = args[3].to_str().ok_or("invalid ID")?.parse()?;
    let root = Path::new(&args[1]);
    let quarantine_root = Path::new(&args[2]);
    // An upgrade must not create a replacement or partial store by typo.
    for directory in [root, quarantine_root] {
        if !directory.is_absolute() || !std::fs::symlink_metadata(directory)?.is_dir() {
            return Err("existing absolute private directories required".into());
        }
    }
    let files = ApprovedFiles::open(root)?;
    let quarantine = Quarantine::new(quarantine_root)?;
    let store = if settings.group_read {
        PublicationStore::new_group_readable(root, &quarantine)?
    } else {
        PublicationStore::new(root, &quarantine)?
    };
    let client = DispatchClient::new(&ClientSettings::read(Path::new(&args[0]))?)?;
    let admin = LegacyMediaStore::connect(&settings.database_url).await?;
    backfill(&admin, &store, &files, &client, &id.to_string()).await
}
