#![forbid(unsafe_code)]

use board_config::MediaReaderSettings;
use board_media::ApprovedFiles;
use board_media_admin::{PublicationResult, read_approved};
use board_store::media_assets::MediaReader;
use std::{fs::OpenOptions, io::Write, path::Path};

#[tokio::main]
async fn main() {
    if run().await.is_err() {
        eprintln!("approved media read rejected");
        std::process::exit(1);
    }
}

async fn run() -> PublicationResult<()> {
    let settings = MediaReaderSettings::from_env()?;
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.len() != 3 {
        return Err("usage: media-read OUTPUT_ID PRIVATE_STORE DESTINATION".into());
    }
    let id = args[0].to_str().ok_or("invalid output id")?;
    let reader = MediaReader::connect(&settings.database_url).await?;
    let files = ApprovedFiles::open(Path::new(&args[1]))?;
    let bytes = read_approved(&reader, &files, id).await?;
    let mut destination = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(Path::new(&args[2]))?;
    destination.write_all(&bytes)?;
    destination.sync_all()?;
    Ok(())
}
