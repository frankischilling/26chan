use board_observe::{SERVICE_TARGETS, STORAGE_TARGETS};
use serde::Deserialize;
use std::{
    fs::File,
    io::Read,
    path::{Path, PathBuf},
};

const MAX_CONFIG: usize = 16 * 1024;

#[derive(Clone, Debug)]
pub struct Targets {
    pub storages: [Option<PathBuf>; 4],
    pub services: [Option<PathBuf>; 11],
}

#[derive(Debug)]
pub struct ConfigError;

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Invalid resource observer configuration.")
    }
}
impl std::error::Error for ConfigError {}

/// The exporter credential is parsed separately by the application startup code.
pub fn from_env() -> Result<Targets, ConfigError> {
    for (key, _) in std::env::vars_os() {
        let key = key.to_string_lossy().to_ascii_uppercase();
        if key.starts_with("PG")
            || matches!(
                key.as_str(),
                "DATABASE_URL"
                    | "TEST_PUBLIC_DATABASE_URL"
                    | "MIGRATION_DATABASE_URL"
                    | "MEDIA_DATABASE_URL"
                    | "MEDIA_READ_DATABASE_URL"
                    | "AUTH_DATABASE_URL"
                    | "STAFF_DATABASE_URL"
                    | "MONITOR_DATABASE_URL"
            )
        {
            return Err(ConfigError);
        }
    }
    if !matches!(
        std::env::var("APP_ENV").as_deref(),
        Ok("development" | "production")
    ) {
        return Err(ConfigError);
    }
    let path = std::env::var_os("RESOURCE_CONFIG_FILE").ok_or(ConfigError)?;
    read_config(Path::new(&path))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    storages: Vec<Entry>,
    services: Vec<Entry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    target: String,
    path: PathBuf,
}

pub fn parse_config(bytes: &[u8]) -> Result<Targets, ConfigError> {
    if bytes.len() > MAX_CONFIG {
        return Err(ConfigError);
    }
    let document: Document = serde_json::from_slice(bytes).map_err(|_| ConfigError)?;
    fn slots<const N: usize>(
        entries: Vec<Entry>,
        names: [&str; N],
    ) -> Result<[Option<PathBuf>; N], ConfigError> {
        if entries.is_empty() || entries.len() > N {
            return Err(ConfigError);
        }
        let mut result = std::array::from_fn(|_| None);
        for entry in entries {
            let index = names
                .iter()
                .position(|name| *name == entry.target)
                .ok_or(ConfigError)?;
            if result[index].is_some() {
                return Err(ConfigError);
            }
            checked_path(&entry.path, true)?;
            result[index] = Some(entry.path);
        }
        Ok(result)
    }
    Ok(Targets {
        storages: slots(document.storages, STORAGE_TARGETS)?,
        services: slots(document.services, SERVICE_TARGETS)?,
    })
}

/// Compare raw path spelling as Path equality normalizes some dot components.
fn checked_path(path: &Path, directory: bool) -> Result<(), ConfigError> {
    if !path.is_absolute() {
        return Err(ConfigError);
    }
    for ancestor in path.ancestors() {
        if std::fs::symlink_metadata(ancestor)
            .map_err(|_| ConfigError)?
            .file_type()
            .is_symlink()
        {
            return Err(ConfigError);
        }
    }
    if path.canonicalize().map_err(|_| ConfigError)?.as_os_str() != path.as_os_str() {
        return Err(ConfigError);
    }
    let metadata = std::fs::symlink_metadata(path).map_err(|_| ConfigError)?;
    if (directory && !metadata.is_dir()) || (!directory && !metadata.is_file()) {
        return Err(ConfigError);
    }
    Ok(())
}

pub fn read_config(path: &Path) -> Result<Targets, ConfigError> {
    checked_path(path, false)?;
    #[cfg(target_os = "linux")]
    let file = {
        use rustix::fs::{Mode, OFlags, openat};
        let parent = open_directory(path.parent().ok_or(ConfigError)?)?;
        let fd = openat(
            &parent,
            path.file_name().ok_or(ConfigError)?,
            OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| ConfigError)?;
        File::from(fd)
    };
    #[cfg(not(target_os = "linux"))]
    let file = File::open(path).map_err(|_| ConfigError)?;
    let metadata = file.metadata().map_err(|_| ConfigError)?;
    if !metadata.is_file() || metadata.len() > MAX_CONFIG as u64 {
        return Err(ConfigError);
    }
    let mut bytes = Vec::with_capacity(MAX_CONFIG + 1);
    file.take((MAX_CONFIG + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ConfigError)?;
    parse_config(&bytes)
}

/// Recheck every source and pin each directory component without following links.
/// O_PATH needs traversal permission, not permission to enumerate the directory.
#[cfg(target_os = "linux")]
pub(crate) fn open_directory(path: &Path) -> Result<rustix::fd::OwnedFd, ConfigError> {
    use rustix::fs::{Mode, OFlags, open, openat};
    use std::path::Component;
    checked_path(path, true)?;
    let flags = OFlags::PATH | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut fd = open("/", flags, Mode::empty()).map_err(|_| ConfigError)?;
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                fd = openat(&fd, name, flags, Mode::empty()).map_err(|_| ConfigError)?;
            }
            _ => return Err(ConfigError),
        }
    }
    Ok(fd)
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
