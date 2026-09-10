use crate::{Error, Result};
use serde::Deserialize;
use std::{fs::File, io::Read};
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClientSettings {
    pub endpoint: SocketAddr,
    pub server_name: String,
    pub server_ca: PathBuf,
    pub client_certificate: PathBuf,
    pub client_key: PathBuf,
}

impl ClientSettings {
    pub fn read(path: &Path) -> Result<Self> {
        let settings: Self = serde_json::from_slice(&read_file(path, false, false, 65_536)?)
            .map_err(|_| Error::Configuration)?;
        settings.validate()?;
        Ok(settings)
    }

    pub(crate) fn validate(&self) -> Result<()> {
        if self.endpoint.port() == 0
            || !matches!(
                rustls::pki_types::ServerName::try_from(self.server_name.as_str()),
                Ok(rustls::pki_types::ServerName::DnsName(_))
            )
        {
            return Err(Error::Configuration);
        }
        for path in [&self.server_ca, &self.client_certificate, &self.client_key] {
            absolute(path)?;
        }
        Ok(())
    }
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GatewaySettings {
    pub listen: SocketAddr,
    pub server_certificate: PathBuf,
    pub server_key: PathBuf,
    pub client_ca: PathBuf,
    pub authorization_file: PathBuf,
    pub broker_socket: PathBuf,
}

impl GatewaySettings {
    pub fn read(path: &Path) -> Result<Self> {
        let settings: Self = serde_json::from_slice(&read_file(path, false, false, 65_536)?)
            .map_err(|_| Error::Configuration)?;
        settings.validate()?;
        Ok(settings)
    }

    pub(crate) fn validate(&self) -> Result<()> {
        for path in [
            &self.server_certificate,
            &self.server_key,
            &self.client_ca,
            &self.authorization_file,
            &self.broker_socket,
        ] {
            absolute(path)?;
        }
        Ok(())
    }
}

fn absolute(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return Err(Error::Configuration);
    }
    Ok(())
}

// The parent directories are operator protected. NOFOLLOW prevents a final-component
// substitution, NONBLOCK keeps a substituted FIFO from blocking before fstat.
pub(crate) fn read_file(
    path: &Path,
    private: bool,
    root_only: bool,
    limit: u64,
) -> Result<Vec<u8>> {
    absolute(path)?;
    #[cfg(unix)]
    let file: File = rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::NONBLOCK
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::empty(),
    )
    .map_err(|_| Error::Configuration)?
    .into();
    #[cfg(not(unix))]
    let file = {
        let _ = (private, root_only);
        if std::fs::symlink_metadata(path)
            .map_err(|_| Error::Configuration)?
            .file_type()
            .is_symlink()
        {
            return Err(Error::Configuration);
        }
        File::open(path).map_err(|_| Error::Configuration)?
    };
    let metadata = file.metadata().map_err(|_| Error::Configuration)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > limit {
        return Err(Error::Configuration);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let uid = rustix::process::geteuid().as_raw();
        if (metadata.uid() != 0 && (root_only || metadata.uid() != uid))
            || metadata.mode() & if private { 0o077 } else { 0o022 } != 0
            || metadata.nlink() != 1
        {
            return Err(Error::Configuration);
        }
    }
    let length = metadata.len();
    let mut bytes = Vec::with_capacity(length as usize);
    file.take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Configuration)?;
    if bytes.len() as u64 != length {
        return Err(Error::Configuration);
    }
    Ok(bytes)
}

#[cfg(target_os = "linux")]
pub(crate) fn authorizations(path: &Path) -> Result<Vec<[u8; 32]>> {
    let bytes = read_file(path, false, true, 520).map_err(|_| Error::Authentication)?;
    let text = std::str::from_utf8(&bytes).map_err(|_| Error::Authentication)?;
    let mut values = Vec::new();
    for line in text.strip_suffix('\n').unwrap_or(text).split('\n') {
        if line.len() != 64
            || !line
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || values.len() == 8
        {
            return Err(Error::Authentication);
        }
        let mut fingerprint = [0; 32];
        for (index, byte) in fingerprint.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&line[index * 2..index * 2 + 2], 16)
                .map_err(|_| Error::Authentication)?;
        }
        if values.contains(&fingerprint) {
            return Err(Error::Authentication);
        }
        values.push(fingerprint);
    }
    Ok(values)
}
