//! Operator qualification of the pinned, prepared player directory.
use board_media_http::swf_player::PlayerAssets;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let root = args.next().ok_or("player directory required")?;
    if args.next().is_some() {
        return Err("unexpected argument".into());
    }
    let assets = PlayerAssets::open(std::path::Path::new(&root))?;
    let manifest: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/ruffle-player-manifest.json"
    ))?;
    for (name, metadata) in manifest["files"]
        .as_object()
        .ok_or("invalid pinned manifest")?
    {
        let (_, bytes) = assets.get(name).ok_or("missing pinned asset")?;
        if bytes.len() as u64 != metadata["bytes"].as_u64().ok_or("invalid pinned length")? {
            return Err("invalid asset length".into());
        }
    }
    if assets.get("../movie.swf").is_some() || assets.get("manifest.json").is_some() {
        return Err("unlisted asset exposed".into());
    }
    println!("Pinned player assets verified; playback is a separate qualification.");
    Ok(())
}
