use crate::{MAX_INPUT_BYTES, MediaError, ObjectId};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};
use tokio::io::{AsyncRead, AsyncReadExt};

/// Private storage under an operator-controlled directory. The root and all
/// parents must remain inaccessible to workers. Path checks are not a boundary
/// against hostile local processes that can modify those directories.
pub struct Quarantine {
    pub(crate) root: PathBuf,
}

impl Quarantine {
    /// Check actual private create/write/sync/unlink access without touching a job.
    pub fn ready(&self) -> Result<(), MediaError> {
        let id = ObjectId::generate()?;
        let path = self.root.join(format!("{id}.ready"));
        let mut probe = PartialFile::create(path.clone())?;
        let file = probe.file.as_mut().expect("probe owns file");
        file.write_all(b"ready")?;
        file.sync_all()?;
        drop(probe.file.take());
        fs::remove_file(path)?;
        Ok(())
    }

    /// Open only the claimed generated ID, never a display filename or a caller
    /// path. The protected root must remain exclusively operator-controlled.
    /// The transport also checks EOF to detect changes after opening.
    pub fn open_input(&self, id: ObjectId, bytes: u64) -> Result<File, MediaError> {
        if !(1..=MAX_INPUT_BYTES).contains(&bytes) {
            return Err(MediaError::InputLengthMismatch);
        }
        let path = self.root.join(format!("{id}.input"));
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.is_file() || metadata.len() != bytes {
            return Err(MediaError::InputLengthMismatch);
        }
        let file = File::open(path)?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() != bytes {
            return Err(MediaError::InputLengthMismatch);
        }
        Ok(file)
    }
    pub fn new(root: impl AsRef<Path>) -> Result<Self, MediaError> {
        fs::create_dir_all(root.as_ref())?;
        Ok(Self {
            root: fs::canonicalize(root)?,
        })
    }

    /// Consume at most 8 MiB plus one lookahead byte, then atomically publish a
    /// nonempty private input without overwriting any previous intake.
    ///
    /// Small filesystem writes are synchronous so cancellation cannot leave a
    /// detached asynchronous write racing with the temporary-file guard.
    pub async fn receive<R: AsyncRead + Unpin>(
        &self,
        id: ObjectId,
        mut reader: R,
    ) -> Result<u64, MediaError> {
        let mut partial = PartialFile::create(self.root.join(format!("{id}.part")))?;
        let mut bytes = 0;
        let mut buffer = [0; 8192];
        loop {
            let remaining = (MAX_INPUT_BYTES - bytes + 1).min(buffer.len() as u64) as usize;
            let count = reader.read(&mut buffer[..remaining]).await?;
            if count == 0 {
                break;
            }
            bytes += count as u64;
            if bytes > MAX_INPUT_BYTES {
                return Err(MediaError::InputTooLarge);
            }
            partial
                .file
                .as_mut()
                .expect("guard owns open file")
                .write_all(&buffer[..count])?;
        }
        if bytes == 0 {
            return Err(MediaError::Empty);
        }
        partial
            .file
            .as_ref()
            .expect("guard owns open file")
            .sync_all()?;
        fs::hard_link(&partial.path, self.root.join(format!("{id}.input")))
            .map_err(intake_error)?;
        Ok(bytes)
    }

    /// Reconcile one inactive, fenced queue job. Never call while its intake is
    /// active. At most two exact filenames are removed; no directory is scanned.
    /// A process crash can leave a partial file for this operation to remove.
    pub fn remove(&self, id: ObjectId) -> Result<(), MediaError> {
        for suffix in ["input", "part"] {
            match fs::remove_file(self.root.join(format!("{id}.{suffix}"))) {
                Ok(()) => (),
                Err(error) if error.kind() == io::ErrorKind::NotFound => (),
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }
}

fn intake_error(error: io::Error) -> MediaError {
    if error.kind() == io::ErrorKind::AlreadyExists {
        MediaError::AlreadyExists
    } else {
        error.into()
    }
}

struct PartialFile {
    file: Option<File>,
    path: PathBuf,
}

impl PartialFile {
    fn create(path: PathBuf) -> Result<Self, MediaError> {
        // Construct the guard only after exclusive creation succeeds: a losing
        // receiver must never unlink a different receiver's partial file.
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(intake_error)?;
        Ok(Self {
            file: Some(file),
            path,
        })
    }
}

impl Drop for PartialFile {
    fn drop(&mut self) {
        drop(self.file.take());
        // Drop cannot return an error. Queue reconciliation handles remnants if
        // an operator changes permissions or the filesystem refuses removal.
        let _ = fs::remove_file(&self.path);
    }
}
