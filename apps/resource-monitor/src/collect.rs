use crate::config::Targets;
use board_observe::ResourceSample;
#[cfg(any(target_os = "linux", test))]
use board_observe::{ServiceSample, StorageSample};

#[derive(Debug)]
pub struct CollectError;
impl std::fmt::Display for CollectError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Resource observation is unavailable.")
    }
}
impl std::error::Error for CollectError {}

/// Collect a whole snapshot. Cache publication and success timestamps belong to
/// the asynchronous sampler; no partially collected values escape on failure.
pub fn collect(targets: &Targets) -> Result<ResourceSample, CollectError> {
    #[cfg(target_os = "linux")]
    {
        linux::collect(targets)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = targets;
        Err(CollectError)
    }
}

#[cfg(any(target_os = "linux", test))]
fn unsigned(value: &str) -> Result<u64, CollectError> {
    let value = value.trim_ascii();
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(CollectError);
    }
    value.parse().map_err(|_| CollectError)
}

#[cfg(any(target_os = "linux", test))]
fn keys<const N: usize>(text: &str, required: [&str; N]) -> Result<[u64; N], CollectError> {
    let mut seen = std::collections::HashSet::new();
    let mut result = [None; N];
    for line in text.lines() {
        if line.trim_ascii().is_empty() {
            continue;
        }
        let mut fields = line.split_ascii_whitespace();
        let key = fields.next().ok_or(CollectError)?;
        let value = unsigned(fields.next().ok_or(CollectError)?)?;
        if fields.next().is_some()
            || !key.is_ascii()
            || key.bytes().any(|byte| byte.is_ascii_control())
            || !seen.insert(key)
        {
            return Err(CollectError);
        }
        if let Some(index) = required.iter().position(|name| *name == key) {
            result[index] = Some(value);
        }
    }
    let mut values = [0; N];
    for (destination, value) in values.iter_mut().zip(result) {
        *destination = value.ok_or(CollectError)?;
    }
    Ok(values)
}

#[cfg(any(target_os = "linux", test))]
fn parse_service(files: [&[u8]; 7]) -> Result<ServiceSample, CollectError> {
    let mut text = [""; 7];
    for (destination, bytes) in text.iter_mut().zip(files) {
        if bytes.len() > 4096 {
            return Err(CollectError);
        }
        *destination = std::str::from_utf8(bytes).map_err(|_| CollectError)?;
    }
    let memory_bytes = unsigned(text[0])?;
    let memory_limit_bytes = unsigned(text[1])?;
    let [memory_oom_kills] = keys(text[2], ["oom_kill"])?;
    let tasks = unsigned(text[3])?;
    let tasks_limit = unsigned(text[4])?;
    let [cpu_usage_usec, cpu_periods, cpu_throttled_periods] =
        keys(text[5], ["usage_usec", "nr_periods", "nr_throttled"])?;
    let mut cpu_limit = text[6].split_ascii_whitespace();
    let cpu_quota_usec = unsigned(cpu_limit.next().ok_or(CollectError)?)?;
    let cpu_period_usec = unsigned(cpu_limit.next().ok_or(CollectError)?)?;
    if cpu_limit.next().is_some()
        || [
            memory_limit_bytes,
            tasks_limit,
            cpu_quota_usec,
            cpu_period_usec,
        ]
        .contains(&0)
    {
        return Err(CollectError);
    }
    Ok(ServiceSample {
        memory_bytes,
        memory_limit_bytes,
        tasks,
        tasks_limit,
        cpu_usage_usec,
        cpu_quota_usec,
        cpu_period_usec,
        cpu_periods,
        cpu_throttled_periods,
        memory_oom_kills,
    })
}

#[cfg(any(target_os = "linux", test))]
fn storage_sample(
    blocks: u64,
    available: u64,
    size: u64,
    inodes: u64,
    available_inodes: u64,
    read_only: bool,
) -> Result<StorageSample, CollectError> {
    if blocks == 0 || size == 0 || inodes == 0 || available > blocks || available_inodes > inodes {
        return Err(CollectError);
    }
    Ok(StorageSample {
        capacity_bytes: blocks.checked_mul(size).ok_or(CollectError)?,
        available_bytes: available.checked_mul(size).ok_or(CollectError)?,
        inodes,
        available_inodes,
        read_only,
    })
}

#[cfg(target_os = "linux")]
mod linux {
    use super::*;
    use crate::config::open_directory;
    use rustix::{
        fd::OwnedFd,
        fs::{Mode, OFlags, StatVfsMountFlags, fstatfs, fstatvfs, openat},
    };
    use std::{fs::File, io::Read, path::Path};

    // Linux UAPI linux/magic.h: CGROUP2_SUPER_MAGIC.
    const CGROUP2_MAGIC: rustix::fs::FsWord = 0x6367_7270;

    pub(super) fn collect(targets: &Targets) -> Result<ResourceSample, CollectError> {
        if !targets.storages.iter().any(Option::is_some)
            || !targets.services.iter().any(Option::is_some)
        {
            return Err(CollectError);
        }
        let mut sample = ResourceSample {
            available: true,
            ..ResourceSample::default()
        };
        for (slot, path) in sample.storages.iter_mut().zip(&targets.storages) {
            if let Some(path) = path {
                *slot = Some(storage(path)?);
            }
        }
        for (slot, path) in sample.services.iter_mut().zip(&targets.services) {
            if let Some(path) = path {
                *slot = Some(service(path)?);
            }
        }
        Ok(sample)
    }

    pub(super) fn storage(path: &Path) -> Result<StorageSample, CollectError> {
        let directory = open_directory(path).map_err(|_| CollectError)?;
        let stat = fstatvfs(&directory).map_err(|_| CollectError)?;
        storage_sample(
            stat.f_blocks,
            stat.f_bavail,
            stat.f_frsize,
            stat.f_files,
            stat.f_favail,
            stat.f_flag.contains(StatVfsMountFlags::RDONLY),
        )
    }

    fn service(path: &Path) -> Result<ServiceSample, CollectError> {
        let directory = open_directory(path).map_err(|_| CollectError)?;
        if fstatfs(&directory).map_err(|_| CollectError)?.f_type != CGROUP2_MAGIC {
            return Err(CollectError);
        }
        let data = [
            read_stat(&directory, "memory.current")?,
            read_stat(&directory, "memory.max")?,
            read_stat(&directory, "memory.events")?,
            read_stat(&directory, "pids.current")?,
            read_stat(&directory, "pids.max")?,
            read_stat(&directory, "cpu.stat")?,
            read_stat(&directory, "cpu.max")?,
        ];
        parse_service(std::array::from_fn(|index| data[index].as_slice()))
    }

    pub(super) fn read_stat(directory: &OwnedFd, name: &str) -> Result<Vec<u8>, CollectError> {
        let fd = openat(
            directory,
            name,
            OFlags::RDONLY | OFlags::NONBLOCK | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|_| CollectError)?;
        // Check the file's filesystem too: a different filesystem bind-mounted
        // over a statistic must not turn arbitrary numeric data into a sample.
        if fstatfs(&fd).map_err(|_| CollectError)?.f_type != CGROUP2_MAGIC {
            return Err(CollectError);
        }
        let file = File::from(fd);
        if !file.metadata().map_err(|_| CollectError)?.is_file() {
            return Err(CollectError);
        }
        let mut bytes = Vec::with_capacity(4097);
        file.take(4097)
            .read_to_end(&mut bytes)
            .map_err(|_| CollectError)?;
        if bytes.len() > 4096 {
            return Err(CollectError);
        }
        Ok(bytes)
    }
}

#[cfg(test)]
#[path = "collect_tests.rs"]
mod tests;
