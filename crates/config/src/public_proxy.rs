use crate::ConfigError;
use std::path::{Component, Path, PathBuf};

/// Kernel-authenticated local proxy; this is never a public HTTP credential.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicProxy {
    socket: PathBuf,
    uid: u32,
}

impl PublicProxy {
    pub fn socket(&self) -> &Path {
        &self.socket
    }
    pub fn uid(&self) -> u32 {
        self.uid
    }

    pub fn from_env(production: bool) -> Result<Option<Self>, ConfigError> {
        let value = |name| {
            std::env::var_os(name)
                .map(|value| {
                    value
                        .into_string()
                        .map_err(|_| ConfigError("Public proxy settings must be Unicode."))
                })
                .transpose()
        };
        Self::parse(
            value("PUBLIC_PROXY_SOCKET")?.as_deref(),
            value("PUBLIC_PROXY_UID")?.as_deref(),
            production,
            cfg!(target_os = "linux"),
        )
    }

    fn parse(
        socket: Option<&str>,
        uid: Option<&str>,
        production: bool,
        linux: bool,
    ) -> Result<Option<Self>, ConfigError> {
        let (socket, uid) = match (socket, uid) {
            (None, None) if !production => return Ok(None),
            (Some(socket), Some(uid)) => (socket, uid),
            _ => {
                return Err(ConfigError(
                    "Configure both PUBLIC_PROXY_SOCKET and PUBLIC_PROXY_UID; production requires the verified local proxy.",
                ));
            }
        };
        if !linux {
            return Err(ConfigError("Verified public proxy sockets require Linux."));
        }
        let path = Path::new(socket);
        if socket.is_empty()
            || socket.len() > 100
            || !socket.starts_with('/')
            || socket.chars().any(char::is_control)
            || socket.contains('\\')
            || path
                .components()
                .any(|c| !matches!(c, Component::RootDir | Component::Normal(_)))
            || socket
                .split('/')
                .skip(1)
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(ConfigError(
                "Public proxy socket must be a bounded canonical absolute path.",
            ));
        }
        if uid.is_empty()
            || uid.len() > 10
            || !uid.bytes().all(|b| b.is_ascii_digit())
            || (uid.len() > 1 && uid.starts_with('0'))
        {
            return Err(ConfigError(
                "Public proxy UID must be a canonical numeric user ID.",
            ));
        }
        let uid: u32 = uid
            .parse()
            .map_err(|_| ConfigError("Public proxy UID is out of range."))?;
        if uid == u32::MAX || (production && uid == 0) {
            return Err(ConfigError(
                "Production proxy workers must use a non-root user ID.",
            ));
        }
        Ok(Some(Self {
            socket: path.to_owned(),
            uid,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn verified_proxy_is_explicit_bounded_and_required_in_production() {
        assert_eq!(PublicProxy::parse(None, None, false, true).unwrap(), None);
        assert!(PublicProxy::parse(None, None, true, true).is_err());
        assert!(PublicProxy::parse(Some("/run/board/public.sock"), None, false, true).is_err());
        assert!(PublicProxy::parse(None, Some("33"), false, true).is_err());
        assert!(
            PublicProxy::parse(Some("/run/board/public.sock"), Some("33"), false, false).is_err()
        );
        let settings = PublicProxy::parse(Some("/run/board/public.sock"), Some("33"), true, true)
            .unwrap()
            .unwrap();
        assert_eq!(settings.uid(), 33);
        for invalid in [
            "",
            "relative.sock",
            "/tmp/../public.sock",
            "/tmp/./public.sock",
            "/tmp//public.sock",
            "/tmp/public.sock/",
            "/tmp/bad\0.sock",
        ] {
            assert!(PublicProxy::parse(Some(invalid), Some("33"), false, true).is_err());
        }
        for invalid in ["", " 33", "+33", "033", "4294967295", "4294967296", "user"] {
            assert!(
                PublicProxy::parse(Some("/tmp/public.sock"), Some(invalid), false, true).is_err()
            );
        }
        assert!(PublicProxy::parse(Some("/tmp/public.sock"), Some("0"), true, true).is_err());
    }
}
