use board_observe::MAINTENANCE_TARGETS;
use serde::Deserialize;
use std::{
    fs::File,
    io::Read,
    path::{Component, Path, PathBuf},
};

const MAX_CONFIG: usize = 16 * 1024;

#[derive(Clone, Debug)]
pub struct Target {
    pub path: PathBuf,
    pub max_age_seconds: u64,
    pub run_timeout_seconds: u64,
}

#[derive(Clone, Debug)]
pub struct Targets {
    pub targets: [Option<Target>; 4],
    pub production: bool,
}

#[derive(Debug)]
pub struct ConfigError;

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Invalid maintenance observer configuration.")
    }
}
impl std::error::Error for ConfigError {}

/// Exporter settings are required and parsed separately by application startup.
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
                    | "INTAKE_DATABASE_URL"
            )
        {
            return Err(ConfigError);
        }
    }
    let production = match std::env::var("APP_ENV").as_deref() {
        Ok("development") => false,
        Ok("production") => true,
        _ => return Err(ConfigError),
    };
    let path = std::env::var_os("MAINTENANCE_CONFIG_FILE").ok_or(ConfigError)?;
    read_config(Path::new(&path), production)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    targets: Vec<Entry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    target: String,
    path: PathBuf,
    max_age_seconds: u64,
    run_timeout_seconds: u64,
}

pub fn parse_config(bytes: &[u8], production: bool) -> Result<Targets, ConfigError> {
    if bytes.len() > MAX_CONFIG {
        return Err(ConfigError);
    }
    let document: Document = serde_json::from_slice(bytes).map_err(|_| ConfigError)?;
    if document.targets.is_empty() || document.targets.len() > 4 {
        return Err(ConfigError);
    }
    let mut result = Targets {
        targets: std::array::from_fn(|_| None),
        production,
    };
    for entry in document.targets {
        let index = MAINTENANCE_TARGETS
            .iter()
            .position(|name| *name == entry.target)
            .ok_or(ConfigError)?;
        if result.targets[index].is_some()
            || !(1..=2592000).contains(&entry.max_age_seconds)
            || !(1..=3600).contains(&entry.run_timeout_seconds)
        {
            return Err(ConfigError);
        }
        // Source existence and authority are checked independently at collection:
        // one denied or missing source must not prevent observation of the others.
        lexical_path(&entry.path)?;
        result.targets[index] = Some(Target {
            path: entry.path,
            max_age_seconds: entry.max_age_seconds,
            run_timeout_seconds: entry.run_timeout_seconds,
        });
    }
    Ok(result)
}

pub fn read_config(path: &Path, production: bool) -> Result<Targets, ConfigError> {
    parse_config(&read_source(path, production, MAX_CONFIG)?, production)
}

fn lexical_path(path: &Path) -> Result<(), ConfigError> {
    if !path.is_absolute()
        || path.as_os_str().as_encoded_bytes().contains(&0)
        || path
            .components()
            .any(|part| matches!(part, Component::CurDir | Component::ParentDir))
        || path.components().collect::<PathBuf>().as_os_str() != path.as_os_str()
        || path.file_name().is_none()
    {
        return Err(ConfigError);
    }
    Ok(())
}

#[cfg(any(target_os = "linux", test))]
fn trusted(uid: u32, mode: u32, production: bool) -> bool {
    !production || (uid == 0 && mode & 0o022 == 0)
}

/// Pin each component before proceeding. No file data is read until the opened
/// descriptor passes type, ownership, write-authority and size checks.
pub(crate) fn read_source(
    path: &Path,
    production: bool,
    maximum: usize,
) -> Result<Vec<u8>, ConfigError> {
    lexical_path(path)?;
    #[cfg(target_os = "linux")]
    let file = {
        use rustix::fs::{Mode, OFlags, fstat, open, openat};
        let flags = OFlags::PATH | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
        let mut directory = open("/", flags, Mode::empty()).map_err(|_| ConfigError)?;
        let root = fstat(&directory).map_err(|_| ConfigError)?;
        if !trusted(root.st_uid, root.st_mode, production) {
            return Err(ConfigError);
        }
        for component in path.parent().ok_or(ConfigError)?.components() {
            match component {
                Component::RootDir => {}
                Component::Normal(name) => {
                    directory =
                        openat(&directory, name, flags, Mode::empty()).map_err(|_| ConfigError)?;
                    let metadata = fstat(&directory).map_err(|_| ConfigError)?;
                    if !trusted(metadata.st_uid, metadata.st_mode, production) {
                        return Err(ConfigError);
                    }
                }
                _ => return Err(ConfigError),
            }
        }
        let fd = openat(
            &directory,
            path.file_name().ok_or(ConfigError)?,
            OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| ConfigError)?;
        let metadata = fstat(&fd).map_err(|_| ConfigError)?;
        if !trusted(metadata.st_uid, metadata.st_mode, production) {
            return Err(ConfigError);
        }
        File::from(fd)
    };
    #[cfg(not(target_os = "linux"))]
    let file = {
        // Portable parser tests may read development fixtures. Production owner
        // checks and the actual observer runtime are Linux-only.
        if production {
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
        File::open(path).map_err(|_| ConfigError)?
    };
    let metadata = file.metadata().map_err(|_| ConfigError)?;
    if !metadata.is_file() || metadata.len() > maximum as u64 {
        return Err(ConfigError);
    }
    let mut bytes = Vec::with_capacity(maximum + 1);
    file.take((maximum + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| ConfigError)?;
    if bytes.len() > maximum {
        return Err(ConfigError);
    }
    Ok(bytes)
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
