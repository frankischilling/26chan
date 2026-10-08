use crate::{MAX_INPUT_BYTES, MediaError, ObjectId};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
};
use tokio::io::{AsyncRead, AsyncReadExt};

/// One bounded, immutable snapshot of the already-open private intake file.
/// Both provenance and transport must consume these same bytes, never reopen
/// the pathname. The hash is integrity metadata, not a decoder admission check.
pub struct InputSnapshot {
    bytes: Box<[u8]>,
    raw_sha256: [u8; 32],
}

impl InputSnapshot {
    pub async fn read(file: File, recorded_bytes: u64) -> Result<Self, MediaError> {
        if !(1..=MAX_INPUT_BYTES).contains(&recorded_bytes) {
            return Err(MediaError::InputLengthMismatch);
        }
        let mut file = tokio::fs::File::from_std(file);
        let metadata = file.metadata().await?;
        if !metadata.is_file() || metadata.len() != recorded_bytes {
            return Err(MediaError::InputLengthMismatch);
        }
        Self::read_bounded(&mut file, recorded_bytes).await
    }

    // Kept private: production snapshots can only originate from a checked file.
    async fn read_bounded<R: AsyncRead + Unpin>(
        mut file: R,
        recorded_bytes: u64,
    ) -> Result<Self, MediaError> {
        let length =
            usize::try_from(recorded_bytes).map_err(|_| MediaError::InputLengthMismatch)?;
        let mut bytes = vec![0; length].into_boxed_slice();
        file.read_exact(&mut bytes).await.map_err(|error| {
            if error.kind() == io::ErrorKind::UnexpectedEof {
                MediaError::InputLengthMismatch
            } else {
                error.into()
            }
        })?;
        let mut lookahead = [0];
        if file.read(&mut lookahead).await? != 0 {
            return Err(MediaError::InputLengthMismatch);
        }
        let raw_sha256 = Sha256::digest(&bytes).into();
        Ok(Self { bytes, raw_sha256 })
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn raw_sha256(&self) -> &[u8; 32] {
        &self.raw_sha256
    }
}

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

    /// Receive a completed v2 pair with bounded buffers. The header is a declaration;
    /// only the bytes read here determine the persisted descriptors. No image or
    /// replay decoder runs here. Missing marker, extra bytes, and body errors fail.
    pub async fn receive_pair<R: AsyncRead + Unpin>(
        &self,
        id: ObjectId,
        mut reader: R,
    ) -> Result<PairReceipt, MediaError> {
        let mut partial = PartialFile::create(self.root.join(format!("{id}.part")))?;
        let file = partial.file.as_mut().expect("guard owns file");
        let receipt = read_pair(&mut reader, file, id).await?;
        file.sync_all()?;
        fs::hard_link(&partial.path, self.root.join(format!("{id}.input")))
            .map_err(intake_error)?;
        File::open(&self.root)?.sync_all()?;
        Ok(receipt)
    }

    /// Reconcile a completed object after an uncertain SQL result. Opens once,
    /// verifies a bounded regular file and its complete frame, and computes the
    /// same actual-byte descriptors without replacing or rewriting the object.
    /// The service authenticates the bearer before calling this method.
    pub async fn inspect_pair(&self, id: ObjectId) -> Result<PairReceipt, MediaError> {
        let path = self.root.join(format!("{id}.input"));
        let metadata = fs::symlink_metadata(&path)?;
        if !metadata.is_file()
            || !(57..=crate::paired::MAX_PAIR_INPUT_BYTES).contains(&metadata.len())
        {
            return Err(MediaError::InputLengthMismatch);
        }
        let file = File::open(path)?;
        let opened = file.metadata()?;
        if !opened.is_file() || opened.len() != metadata.len() {
            return Err(MediaError::InputLengthMismatch);
        }
        let mut reader = tokio::fs::File::from_std(file);
        let receipt = read_pair(&mut reader, &mut io::sink(), id).await?;
        if receipt.bytes != opened.len() {
            return Err(MediaError::InputLengthMismatch);
        }
        Ok(receipt)
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

/// Actual-byte descriptors from one completed quarantine receive. This receipt
/// does not establish decoded validity, processing authority, or publication.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairReceipt {
    pub bytes: u64,
    pub sha256: [u8; 32],
    pub image_bytes: u64,
    pub image_sha256: [u8; 32],
    pub replay_bytes: Option<u64>,
    pub replay_sha256: Option<[u8; 32]>,
}

async fn read_pair<R: AsyncRead + Unpin, W: Write>(
    reader: &mut R,
    writer: &mut W,
    id: ObjectId,
) -> Result<PairReceipt, MediaError> {
    use crate::paired::{INPUT_COMPLETION, InputHeader};
    let mut raw = [0; 48];
    reader.read_exact(&mut raw).await?;
    let header = InputHeader::parse(raw, id.bytes())?;
    writer.write_all(&raw)?;
    let mut bundle = Sha256::new();
    bundle.update(raw);
    let image_sha256 = receive_component(reader, writer, &mut bundle, header.image_bytes()).await?;
    let replay_sha256 = if let Some(length) = header.replay_bytes() {
        Some(receive_component(reader, writer, &mut bundle, length).await?)
    } else {
        None
    };
    let mut trailer = [0; 8];
    reader.read_exact(&mut trailer).await?;
    if &trailer != INPUT_COMPLETION {
        return Err(crate::paired::PairedError::Header.into());
    }
    let mut eof = [0];
    if reader.read(&mut eof).await? != 0 {
        return Err(MediaError::InputLengthMismatch);
    }
    writer.write_all(&trailer)?;
    bundle.update(trailer);
    Ok(PairReceipt {
        bytes: header.total_bytes(),
        sha256: bundle.finalize().into(),
        image_bytes: header.image_bytes(),
        image_sha256,
        replay_bytes: header.replay_bytes(),
        replay_sha256,
    })
}

async fn receive_component<R: AsyncRead + Unpin, W: Write>(
    reader: &mut R,
    file: &mut W,
    bundle: &mut Sha256,
    mut remaining: u64,
) -> Result<[u8; 32], MediaError> {
    let mut hash = Sha256::new();
    let mut buffer = [0; 8192];
    while remaining != 0 {
        let limit = remaining.min(buffer.len() as u64) as usize;
        let count = reader.read(&mut buffer[..limit]).await?;
        if count == 0 {
            return Err(MediaError::InputLengthMismatch);
        }
        file.write_all(&buffer[..count])?;
        hash.update(&buffer[..count]);
        bundle.update(&buffer[..count]);
        remaining -= count as u64;
    }
    Ok(hash.finalize().into())
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

#[cfg(test)]
mod snapshot_tests {
    use super::*;

    use std::{
        future::Future,
        pin::Pin,
        sync::{
            Arc,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        },
        task::{Context, Poll},
    };
    use tokio::io::ReadBuf;

    struct StalledReader {
        remaining: usize,
        consumed: Arc<AtomicUsize>,
        dropped: Arc<AtomicBool>,
    }

    impl AsyncRead for StalledReader {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
            buffer: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            assert!(
                buffer.remaining() <= 3,
                "fixed snapshot allocation or one-byte EOF probe"
            );
            if self.remaining == 0 {
                return Poll::Pending;
            }
            let count = self.remaining.min(buffer.remaining());
            buffer.put_slice(&b"abc"[..count]);
            self.remaining -= count;
            self.consumed.fetch_add(count, Ordering::SeqCst);
            Poll::Ready(Ok(()))
        }
    }

    impl Drop for StalledReader {
        fn drop(&mut self) {
            self.dropped.store(true, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn cancelling_stalled_snapshot_drops_reader_without_completing_snapshot() {
        // Exercise both a partial read and a stalled final EOF probe through
        // the same private helper used by checked regular-file snapshots.
        // This tests future ownership, not physical cancellation of Tokio's
        // internal filesystem operations.
        for available in [1, 3] {
            let consumed = Arc::new(AtomicUsize::new(0));
            let dropped = Arc::new(AtomicBool::new(false));
            let completed = Arc::new(AtomicBool::new(false));
            let reader = StalledReader {
                remaining: available,
                consumed: consumed.clone(),
                dropped: dropped.clone(),
            };
            let reached_completion = completed.clone();
            let mut future = Box::pin(async move {
                let result = InputSnapshot::read_bounded(reader, 3).await;
                reached_completion.store(true, Ordering::SeqCst);
                result
            });
            assert!(
                future
                    .as_mut()
                    .poll(&mut Context::from_waker(std::task::Waker::noop()))
                    .is_pending()
            );
            assert_eq!(consumed.load(Ordering::SeqCst), available);
            assert!(!dropped.load(Ordering::SeqCst));
            drop(future);
            assert!(dropped.load(Ordering::SeqCst));
            tokio::task::yield_now().await;
            assert!(!completed.load(Ordering::SeqCst));
            assert_eq!(consumed.load(Ordering::SeqCst), available);
        }
    }

    #[tokio::test]
    async fn snapshot_exact_read_requires_eof_and_rejects_truncation_or_excess() {
        for bytes in [b"ab".as_slice(), b"abcd".as_slice()] {
            assert!(matches!(
                InputSnapshot::read_bounded(bytes, 3).await,
                Err(MediaError::InputLengthMismatch)
            ));
        }
        let snapshot = InputSnapshot::read_bounded(b"abc".as_slice(), 3)
            .await
            .unwrap();
        assert_eq!(snapshot.bytes(), b"abc");
        assert_eq!(
            snapshot.raw_sha256(),
            &[
                0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae,
                0x22, 0x23, 0xb0, 0x03, 0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61,
                0xf2, 0x00, 0x15, 0xad,
            ]
        );
    }
}
