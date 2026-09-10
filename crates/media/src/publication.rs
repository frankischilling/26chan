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
/// The file is closed (releasing the lock) on cancellation or process death.
pub struct PublicationGuard<'a> {
    store: &'a PublicationStore,
    _lock: File,
}

/// Read-only filesystem access. Callers must obtain approval from the database
/// before each read. This type does not create directories or lock files.
pub struct ApprovedFiles {
    root: PathBuf,
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
        let staging = self.store.root.join(format!("{id}.part"));
        remove_regular(&staging)?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&staging)?;
        file.write_all(&output.bytes)?;
        // Shared output is deliberate and independent of the publisher's umask.
        // The preprovisioned setgid directory selects its nonroot reader group.
        #[cfg(unix)]
        if self.store.group_read {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o640))?;
        }
        file.sync_all()?;
        drop(file);
        let destination = self.store.root.join(format!("{id}.png"));
        let already_published = match fs::hard_link(&staging, &destination) {
            Ok(()) => false,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                let actual = read_checked(&destination, output.sha256(), output.len())?;
                if actual != output.bytes {
                    return Err(MediaError::Conflict);
                }
                true
            }
            Err(error) => return Err(error.into()),
        };
        remove_regular(&staging)?;
        sync_directory(&self.store.root)?;
        Ok(Promotion {
            sha256: output.sha256().to_owned(),
            bytes: output.len(),
            already_published,
        })
    }

    /// Caller must first durably mark this reservation deleting while holding
    /// this guard. Approved or currently leased objects must never reach here.
    pub fn remove(&self, id: ObjectId) -> Result<(), MediaError> {
        for suffix in ["part", "png"] {
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
        read_checked(&self.root.join(format!("{id}.png")), sha256, bytes)
    }
}

fn read_checked(path: &Path, sha256: &str, bytes: u64) -> Result<Vec<u8>, MediaError> {
    if !(1..=MAX_PNG_BYTES as u64).contains(&bytes)
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
