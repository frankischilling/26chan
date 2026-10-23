use crate::animation::{EncodedGif, MAX_GIF_BYTES};
use crate::canonical_replay::CanonicalReplay;
use crate::pdf::{MAX_PDF_BYTES, RetainedPdf};
use crate::still::{MAX_IMAGE_BYTES, StillFormat, ValidatedStill};
use crate::video::{MAX_VIDEO_BYTES, ValidatedVideo, VideoFormat};
use crate::{EncodedOutput, MAX_PNG_BYTES, MediaError, ObjectId, Promotion, Quarantine};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions, TryLockError},
    io::{self, Read, Write},
    path::{Path, PathBuf},
};

/// Private output storage. Operator-owned roots and parents must be protected
/// from workers and readers. Every writer/cleanup must use this store's lock;
/// neither the lock nor its parent directory may be replaced while in use.
pub struct PublicationStore {
    root: PathBuf,
    #[cfg(unix)]
    group_read: bool,
}

/// Retains the operating system lock through database approval or cleanup.
/// Normal guard destruction explicitly unlocks before closing the file.
pub struct PublicationGuard<'a> {
    store: &'a PublicationStore,
    _lock: File,
}

impl Drop for PublicationGuard<'_> {
    fn drop(&mut self) {
        // Closing only this descriptor can leave flock held by a descriptor
        // inherited during another thread's process launch. Release at the
        // guard's actual lifetime boundary, before the owned file is closed.
        let _ = self._lock.unlock();
    }
}

/// Read-only filesystem access. Callers must obtain approval from the database
/// before each read. This type does not create directories or lock files.
pub struct ApprovedFiles {
    root: PathBuf,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Format {
    Png,
    SourcePng,
    Jpeg,
    Gif,
    Pdf,
    Webm,
    Mp4,
    Swf,
    Replay,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Variant {
    Primary,
    Thumbnail,
    Mobile,
}

impl Format {
    fn extension(self) -> &'static str {
        match self {
            Self::Png | Self::SourcePng => "png",
            Self::Jpeg => "jpg",
            Self::Gif => "gif",
            Self::Pdf => "pdf",
            Self::Webm => "webm",
            Self::Mp4 => "mp4",
            Self::Swf => "swf",
            Self::Replay => "ibr",
        }
    }
    fn other_primary_formats(self) -> &'static [&'static str] {
        match self {
            Self::Png | Self::SourcePng => &["jpg", "gif", "pdf", "webm", "mp4", "swf"],
            Self::Jpeg => &["png", "gif", "pdf", "webm", "mp4", "swf"],
            Self::Gif => &["png", "jpg", "pdf", "webm", "mp4", "swf"],
            Self::Pdf => &["png", "jpg", "gif", "webm", "mp4", "swf"],
            Self::Webm => &["png", "jpg", "gif", "pdf", "mp4", "swf"],
            Self::Mp4 => &["png", "jpg", "gif", "pdf", "webm", "swf"],
            Self::Swf => &["png", "jpg", "gif", "pdf", "webm", "mp4"],
            Self::Replay => unreachable!("replays have no opposite image format"),
        }
    }
    fn byte_limit(self) -> u64 {
        match self {
            Self::Png => MAX_PNG_BYTES as u64,
            Self::SourcePng | Self::Jpeg => u64::from(MAX_IMAGE_BYTES),
            Self::Gif => MAX_GIF_BYTES as u64,
            Self::Pdf => MAX_PDF_BYTES,
            Self::Webm | Self::Mp4 => MAX_VIDEO_BYTES,
            Self::Replay => crate::replay_wire::MAX_WIRE_BYTES as u64,
            Self::Swf => crate::swf::MAX_INPUT_BYTES,
        }
    }
}

impl PublicationStore {
    pub fn new_group_readable(
        root: impl AsRef<Path>,
        quarantine: &Quarantine,
    ) -> Result<Self, MediaError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let metadata = fs::symlink_metadata(root.as_ref())?;
            if !metadata.is_dir() || metadata.mode() & 0o7777 != 0o2750 || metadata.gid() == 0 {
                return Err(MediaError::InvalidStorage);
            }
            let mut store = Self::new(root, quarantine)?;
            store.group_read = true;
            Ok(store)
        }
        #[cfg(not(unix))]
        {
            let _ = (root, quarantine);
            Err(MediaError::InvalidStorage)
        }
    }

    pub fn new(root: impl AsRef<Path>, quarantine: &Quarantine) -> Result<Self, MediaError> {
        fs::create_dir_all(root.as_ref())?;
        let root = fs::canonicalize(root)?;
        if root.starts_with(&quarantine.root) || quarantine.root.starts_with(&root) {
            return Err(MediaError::OverlappingRoots);
        }
        // Persist newly created directory entries as well as each later object.
        // Walking ancestors also covers create_dir_all's intermediate directories.
        for directory in root.ancestors() {
            sync_directory(directory)?;
        }
        Ok(Self {
            root,
            #[cfg(unix)]
            group_read: false,
        })
    }

    pub fn try_lock(&self) -> Result<PublicationGuard<'_>, MediaError> {
        let path = self.root.join(".publication.lock");
        match fs::symlink_metadata(&path) {
            Ok(metadata) if !metadata.is_file() || metadata.len() != 0 => {
                return Err(MediaError::InvalidStorage);
            }
            Ok(_) => (),
            Err(error) if error.kind() == io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options.open(path)?;
        if !lock.metadata()?.is_file() || lock.metadata()?.len() != 0 {
            return Err(MediaError::InvalidStorage);
        }
        match lock.try_lock() {
            Ok(()) => Ok(PublicationGuard {
                store: self,
                _lock: lock,
            }),
            Err(TryLockError::WouldBlock) => Err(MediaError::Busy),
            Err(TryLockError::Error(error)) => Err(error.into()),
        }
    }
}

impl PublicationGuard<'_> {
    /// The caller must first commit a reservation for this ID and exact encoded
    /// metadata under the current lease. A file is still private until database
    /// approval succeeds. Fixed staging names let recovery avoid directory scans.
    pub fn install(&self, id: ObjectId, output: &EncodedOutput) -> Result<Promotion, MediaError> {
        self.install_variant(id, output, false)
    }

    /// GIF bytes must come from the independent neutral animation validator and
    /// host encoder. The caller must reserve GIF metadata before this operation.
    pub fn install_gif(&self, id: ObjectId, output: &EncodedGif) -> Result<Promotion, MediaError> {
        self.install_bytes(
            id,
            output.bytes(),
            output.sha256(),
            Format::Gif,
            Variant::Primary,
        )
    }

    /// Retain only the coordinator's immutable input snapshot after a fenced
    /// PDF reservation. These bytes are never a host-encoded safe image.
    pub fn install_pdf(&self, id: ObjectId, output: &RetainedPdf) -> Result<Promotion, MediaError> {
        self.install_bytes(
            id,
            output.bytes(),
            output.sha256(),
            Format::Pdf,
            Variant::Primary,
        )
    }

    /// Install the bounded remuxed object under its immutable reserved format.
    /// It remains untrusted compressed media, never a host-encoded safe image.
    pub fn install_video(
        &self,
        id: ObjectId,
        output: &ValidatedVideo,
    ) -> Result<Promotion, MediaError> {
        let format = match output.format() {
            VideoFormat::Webm => Format::Webm,
            VideoFormat::Mp4 => Format::Mp4,
        };
        self.install_bytes(
            id,
            output.bytes(),
            output.sha256(),
            format,
            Variant::Primary,
        )
    }

    /// Retained SWF bytes are executable media and must never be served from
    /// the board origin. The caller holds a context-bound SWF reservation.
    pub fn install_swf(
        &self,
        id: ObjectId,
        output: &crate::swf::ValidatedSwf,
    ) -> Result<Promotion, MediaError> {
        self.install_bytes(
            id,
            output.bytes(),
            output.sha256(),
            Format::Swf,
            Variant::Primary,
        )
    }

    /// Reserve this exact retained-file and companion manifest before installing.
    pub fn install_still(
        &self,
        id: ObjectId,
        output: &ValidatedStill,
    ) -> Result<Promotion, MediaError> {
        let format = match output.policy().format {
            StillFormat::Png => Format::SourcePng,
            StillFormat::Jpeg => Format::Jpeg,
            StillFormat::Gif => Format::Gif,
        };
        let file = output.retained();
        self.install_bytes(id, file.bytes(), file.sha256(), format, Variant::Primary)
    }

    pub fn install_still_thumbnail(
        &self,
        id: ObjectId,
        output: &ValidatedStill,
    ) -> Result<Promotion, MediaError> {
        let file = output.thumbnail();
        self.install_bytes(
            id,
            file.bytes(),
            file.sha256(),
            Format::Jpeg,
            Variant::Thumbnail,
        )
    }

    pub fn install_still_mobile(
        &self,
        id: ObjectId,
        output: &ValidatedStill,
    ) -> Result<Option<Promotion>, MediaError> {
        output
            .mobile()
            .map(|file| {
                self.install_bytes(
                    id,
                    file.bytes(),
                    file.sha256(),
                    Format::Jpeg,
                    Variant::Mobile,
                )
            })
            .transpose()
    }

    /// Caller must reserve replay metadata bound to the same paired image job.
    pub fn install_replay(
        &self,
        id: ObjectId,
        replay: &CanonicalReplay,
    ) -> Result<Promotion, MediaError> {
        self.install_bytes(
            id,
            replay.bytes(),
            replay.sha256(),
            Format::Replay,
            Variant::Primary,
        )
    }

    pub fn install_thumbnail(
        &self,
        id: ObjectId,
        output: &EncodedOutput,
    ) -> Result<Promotion, MediaError> {
        if output.dimensions().0 > 250 || output.dimensions().1 > 250 {
            return Err(MediaError::InvalidOutput);
        }
        self.install_variant(id, output, true)
    }

    fn install_variant(
        &self,
        id: ObjectId,
        output: &EncodedOutput,
        thumbnail: bool,
    ) -> Result<Promotion, MediaError> {
        self.install_bytes(
            id,
            &output.bytes,
            output.sha256(),
            Format::Png,
            if thumbnail {
                Variant::Thumbnail
            } else {
                Variant::Primary
            },
        )
    }

    fn install_bytes(
        &self,
        id: ObjectId,
        bytes: &[u8],
        sha256: &str,
        format: Format,
        variant: Variant,
    ) -> Result<Promotion, MediaError> {
        if bytes.is_empty() || bytes.len() as u64 > format.byte_limit() {
            return Err(MediaError::InvalidOutput);
        }
        if variant == Variant::Primary && format != Format::Replay {
            for other in format.other_primary_formats() {
                let opposite = self.store.root.join(format!("{id}.{other}"));
                match fs::symlink_metadata(opposite) {
                    Ok(_) => return Err(MediaError::Conflict),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => (),
                    Err(error) => return Err(error.into()),
                }
            }
        }
        if variant == Variant::Thumbnail {
            let opposite = match format {
                Format::Png => "jpg",
                Format::Jpeg => "png",
                _ => return Err(MediaError::InvalidOutput),
            };
            match fs::symlink_metadata(self.store.root.join(format!("{id}.thumb.{opposite}"))) {
                Ok(_) => return Err(MediaError::Conflict),
                Err(error) if error.kind() == io::ErrorKind::NotFound => (),
                Err(error) => return Err(error.into()),
            }
        }
        if variant == Variant::Mobile && format != Format::Jpeg {
            return Err(MediaError::InvalidOutput);
        }
        let suffix = match variant {
            Variant::Thumbnail => ".thumb",
            Variant::Mobile => ".mobile",
            Variant::Primary if format == Format::Replay => ".replay",
            Variant::Primary => "",
        };
        let staging = self.store.root.join(format!("{id}{suffix}.part"));
        remove_regular(&staging)?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&staging)?;
        file.write_all(bytes)?;
        // Shared output is deliberate and independent of the publisher's umask.
        // The preprovisioned setgid directory selects its nonroot reader group.
        #[cfg(unix)]
        if self.store.group_read {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o640))?;
        }
        file.sync_all()?;
        drop(file);
        let destination = self
            .store
            .root
            .join(format!("{id}{suffix}.{}", format.extension()));
        let already_published = match fs::hard_link(&staging, &destination) {
            Ok(()) => false,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let actual = read_checked(
                    &destination,
                    sha256,
                    bytes.len() as u64,
                    format.byte_limit(),
                )?;
                if actual != bytes {
                    return Err(MediaError::Conflict);
                }
                true
            }
            Err(error) => return Err(error.into()),
        };
        remove_regular(&staging)?;
        sync_directory(&self.store.root)?;
        Ok(Promotion {
            sha256: sha256.to_owned(),
            bytes: bytes.len() as u64,
            already_published,
        })
    }

    /// Caller must first durably mark this reservation deleting while holding
    /// this guard. Approved or currently leased objects must never reach here.
    pub fn remove(&self, id: ObjectId) -> Result<(), MediaError> {
        for suffix in [
            "part",
            "png",
            "jpg",
            "gif",
            "pdf",
            "webm",
            "mp4",
            "swf",
            "thumb.part",
            "thumb.png",
            "thumb.jpg",
            "mobile.part",
            "mobile.jpg",
            "replay.part",
            "replay.ibr",
        ] {
            remove_regular(&self.store.root.join(format!("{id}.{suffix}")))?;
        }
        sync_directory(&self.store.root)?;
        Ok(())
    }
}

impl ApprovedFiles {
    /// Check that the directory is readable without creating or publishing data.
    pub fn ready(&self) -> Result<(), MediaError> {
        fs::read_dir(&self.root)?;
        Ok(())
    }

    pub fn open(root: impl AsRef<Path>) -> Result<Self, MediaError> {
        let root = fs::canonicalize(root)?;
        if !fs::metadata(&root)?.is_dir() {
            return Err(MediaError::InvalidStorage);
        }
        Ok(Self { root })
    }

    pub fn read(&self, id: ObjectId, sha256: &str, bytes: u64) -> Result<Vec<u8>, MediaError> {
        read_checked(
            &self.root.join(format!("{id}.png")),
            sha256,
            bytes,
            MAX_PNG_BYTES as u64,
        )
    }

    /// Call only after obtaining approval for this ID, GIF format, hash and size.
    pub fn read_gif(&self, id: ObjectId, sha256: &str, bytes: u64) -> Result<Vec<u8>, MediaError> {
        read_checked(
            &self.root.join(format!("{id}.gif")),
            sha256,
            bytes,
            MAX_GIF_BYTES as u64,
        )
    }

    /// Call only after live approval for PDF format, hash and size. The caller
    /// must serve this untrusted document with the restricted origin's headers.
    pub fn read_pdf(&self, id: ObjectId, sha256: &str, bytes: u64) -> Result<Vec<u8>, MediaError> {
        read_checked(
            &self.root.join(format!("{id}.pdf")),
            sha256,
            bytes,
            MAX_PDF_BYTES,
        )
    }

    /// Call only after live approval for the exact video format, hash and size.
    /// Range responses must still authenticate the complete stored object.
    pub fn read_swf(&self, id: ObjectId, sha256: &str, bytes: u64) -> Result<Vec<u8>, MediaError> {
        read_checked(
            &self.root.join(format!("{id}.swf")),
            sha256,
            bytes,
            crate::swf::MAX_INPUT_BYTES,
        )
    }

    pub fn read_video(
        &self,
        id: ObjectId,
        sha256: &str,
        bytes: u64,
        format: VideoFormat,
    ) -> Result<Vec<u8>, MediaError> {
        read_checked(
            &self.root.join(format!("{id}.{}", format.extension())),
            sha256,
            bytes,
            MAX_VIDEO_BYTES,
        )
    }

    /// Approval must name the retained still format, exact hash and byte count.
    pub fn read_still(
        &self,
        id: ObjectId,
        sha256: &str,
        bytes: u64,
        format: StillFormat,
    ) -> Result<Vec<u8>, MediaError> {
        read_checked(
            &self.root.join(format!("{id}.{}", format.extension())),
            sha256,
            bytes,
            if format == StillFormat::Gif {
                8 * 1024 * 1024
            } else {
                u64::from(MAX_IMAGE_BYTES)
            },
        )
    }

    pub fn read_still_thumbnail(
        &self,
        id: ObjectId,
        sha256: &str,
        bytes: u64,
    ) -> Result<Vec<u8>, MediaError> {
        read_checked(
            &self.root.join(format!("{id}.thumb.jpg")),
            sha256,
            bytes,
            1024 * 1024,
        )
    }

    pub fn read_still_mobile(
        &self,
        id: ObjectId,
        sha256: &str,
        bytes: u64,
    ) -> Result<Vec<u8>, MediaError> {
        read_checked(
            &self.root.join(format!("{id}.mobile.jpg")),
            sha256,
            bytes,
            4 * 1024 * 1024,
        )
    }

    /// Call only after live-post visibility and replay hash/size approval.
    pub fn read_replay(
        &self,
        id: ObjectId,
        sha256: &str,
        bytes: u64,
    ) -> Result<Vec<u8>, MediaError> {
        read_checked(
            &self.root.join(format!("{id}.replay.ibr")),
            sha256,
            bytes,
            crate::replay_wire::MAX_WIRE_BYTES as u64,
        )
    }

    pub fn read_thumbnail(
        &self,
        id: ObjectId,
        sha256: &str,
        bytes: u64,
    ) -> Result<Vec<u8>, MediaError> {
        read_checked(
            &self.root.join(format!("{id}.thumb.png")),
            sha256,
            bytes,
            MAX_PNG_BYTES as u64,
        )
    }
}

fn read_checked(path: &Path, sha256: &str, bytes: u64, limit: u64) -> Result<Vec<u8>, MediaError> {
    if !(1..=limit).contains(&bytes)
        || sha256.len() != 64
        || !sha256
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(MediaError::Conflict);
    }
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() {
        return Err(MediaError::InvalidStorage);
    }
    if metadata.len() != bytes {
        return Err(MediaError::Conflict);
    }
    let file = File::open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() != bytes {
        return Err(MediaError::Conflict);
    }
    let mut actual = Vec::with_capacity(bytes as usize);
    file.take(bytes + 1).read_to_end(&mut actual)?;
    if actual.len() as u64 != bytes || format!("{:x}", Sha256::digest(&actual)) != sha256 {
        return Err(MediaError::Conflict);
    }
    Ok(actual)
}

fn remove_regular(path: &Path) -> Result<(), MediaError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() => fs::remove_file(path)?,
        Ok(_) => return Err(MediaError::InvalidStorage),
        Err(error) if error.kind() == io::ErrorKind::NotFound => (),
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn sync_directory(root: &Path) -> Result<(), MediaError> {
    #[cfg(unix)]
    File::open(root)?.sync_all()?;
    // Windows is development-only: this does not establish power-loss durability.
    #[cfg(not(unix))]
    let _ = root;
    Ok(())
}

#[cfg(test)]
mod lock_lifetime_tests {
    use super::*;

    #[test]
    fn duplicate_descriptor_cannot_extend_the_publication_guard_lifetime() {
        let directory = tempfile::tempdir().unwrap();
        let quarantine = Quarantine::new(directory.path().join("input")).unwrap();
        let store = PublicationStore::new(directory.path().join("objects"), &quarantine).unwrap();
        let guard = store.try_lock().unwrap();
        let inherited = guard._lock.try_clone().unwrap();
        assert!(matches!(store.try_lock(), Err(MediaError::Busy)));
        drop(guard);
        // Model the descriptor retained between fork and exec without forking
        // the multithreaded test process or changing any process permissions.
        assert!(inherited.metadata().is_ok());
        let next = store.try_lock().unwrap();
        assert!(matches!(store.try_lock(), Err(MediaError::Busy)));
        drop(inherited);
        assert!(matches!(store.try_lock(), Err(MediaError::Busy)));
        drop(next);
        assert!(store.try_lock().is_ok());
    }
}
