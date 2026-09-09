use crate::{MediaError, ObjectId, Quarantine, ValidatedOutput};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

/// Trusted host-side promotion. Both roots and their parents must be controlled
/// by the operator and inaccessible to workers throughout every operation.
pub struct Promoter {
    root: PathBuf,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Promotion {
    pub sha256: String,
    pub bytes: u64,
    pub already_published: bool,
}

impl Promoter {
    pub fn new(public_root: impl AsRef<Path>, quarantine: &Quarantine) -> Result<Self, MediaError> {
        fs::create_dir_all(public_root.as_ref())?;
        let root = fs::canonicalize(public_root)?;
        if root.starts_with(&quarantine.root) || quarantine.root.starts_with(&root) {
            return Err(MediaError::OverlappingRoots);
        }
        Ok(Self { root })
    }

    /// Encode validated pixels and atomically link a complete PNG into place.
    /// The filesystem must support atomic, no-clobber hard links. No decoder is
    /// invoked. Identical replay compares bounded bytes, never worker metadata.
    pub fn promote(&self, id: ObjectId, output: &ValidatedOutput) -> Result<Promotion, MediaError> {
        let bytes = output.encode_png()?;
        let mut staging = tempfile::Builder::new()
            .prefix(".publish-")
            .tempfile_in(&self.root)?;
        staging.write_all(&bytes)?;
        staging.as_file().sync_all()?;
        let destination = self.root.join(format!("{id}.png"));
        let already_published = match fs::hard_link(staging.path(), &destination) {
            Ok(()) => false,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                if !identical(&destination, &bytes)? {
                    return Err(MediaError::Conflict);
                }
                true
            }
            Err(error) => return Err(error.into()),
        };
        Ok(Promotion {
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            bytes: bytes.len() as u64,
            already_published,
        })
    }
}

fn identical(path: &Path, expected: &[u8]) -> Result<bool, MediaError> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() || metadata.len() != expected.len() as u64 {
        return Ok(false);
    }
    let mut file = fs::File::open(path)?;
    let mut buffer = [0; 8192];
    for chunk in expected.chunks(buffer.len()) {
        file.read_exact(&mut buffer[..chunk.len()])?;
        if &buffer[..chunk.len()] != chunk {
            return Ok(false);
        }
    }
    Ok(file.read(&mut [0; 1])? == 0)
}
