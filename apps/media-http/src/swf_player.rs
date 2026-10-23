//! Trusted player assets are pinned independently from all uploaded content.
use axum::body::Bytes;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, Read},
    path::Path,
};

pub const PREFIX: &str = "/player-assets/ruffle-0.7.1-memory-cap-v1/";

pub struct PlayerAssets {
    files: BTreeMap<String, Bytes>,
}

impl PlayerAssets {
    /// Load immutable in-memory copies only after exact built-in digest checks.
    /// The adjacent on-disk manifest is not an authority source.
    pub fn open(root: &Path) -> io::Result<Self> {
        let manifest: serde_json::Value = serde_json::from_str(include_str!(
            "../../../fixtures/ruffle-player-manifest.json"
        ))
        .map_err(io::Error::other)?;
        let entries = manifest["files"].as_object().ok_or_else(rejected)?;
        let mut files = BTreeMap::new();
        for (name, metadata) in entries {
            if name.contains(['/', '\\']) {
                return Err(rejected());
            }
            let size = metadata["bytes"]
                .as_u64()
                .filter(|&size| size <= 16 * 1024 * 1024)
                .ok_or_else(rejected)?;
            let path = root.join(name);
            if !path.symlink_metadata()?.file_type().is_file() {
                return Err(rejected());
            }
            let file = File::open(path)?;
            if !file.metadata()?.is_file() || file.metadata()?.len() != size {
                return Err(rejected());
            }
            let mut bytes = Vec::new();
            file.take(size + 1).read_to_end(&mut bytes)?;
            if bytes.len() as u64 != size
                || format!("{:x}", Sha256::digest(&bytes))
                    != metadata["sha256"].as_str().ok_or_else(rejected)?
            {
                return Err(rejected());
            }
            files.insert(name.clone(), Bytes::from(bytes));
        }
        Ok(Self { files })
    }

    pub fn get(&self, name: &str) -> Option<(&'static str, Bytes)> {
        let bytes = self.files.get(name)?.clone();
        let mime = if name.ends_with(".wasm") {
            "application/wasm"
        } else if name.ends_with(".js") {
            "text/javascript; charset=utf-8"
        } else {
            "text/plain; charset=utf-8"
        };
        Some((mime, bytes))
    }
}

fn rejected() -> io::Error {
    io::Error::other("player asset does not match the pinned runtime")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn absent_or_substituted_assets_are_not_a_runtime() {
        let dir = tempfile::tempdir().unwrap();
        assert!(PlayerAssets::open(dir.path()).is_err());
        std::fs::write(
            dir.path().join("172de42e6619eac8f371.wasm"),
            b"untrusted replacement",
        )
        .unwrap();
        assert!(PlayerAssets::open(dir.path()).is_err());
    }

    #[test]
    fn assets_cannot_be_selected_by_path_or_unlisted_name() {
        let assets = PlayerAssets {
            files: BTreeMap::from([("ruffle.js".into(), Bytes::from_static(b"owned"))]),
        };
        assert_eq!(
            assets.get("ruffle.js").unwrap().0,
            "text/javascript; charset=utf-8"
        );
        for name in [
            "../ruffle.js",
            "/ruffle.js",
            "manifest.json",
            "movie.swf",
            "ruffle.js?x",
            "ruffle.js.map",
        ] {
            assert!(assets.get(name).is_none());
        }
    }
}
