use crate::{ConfigError, Origin};
use std::{env, net::SocketAddr};

/// Only the explicit loopback qualification profile exists. No Debug: token.
#[derive(Clone)]
pub struct PublicMediaSettings {
    pub intake: SocketAddr,
    pub token: String,
    pub origin: Origin,
}

impl PublicMediaSettings {
    pub fn development(intake: &str, token: &str, origin: &str) -> Result<Self, ConfigError> {
        let intake: SocketAddr = intake
            .parse()
            .map_err(|_| ConfigError("Invalid public intake address."))?;
        let origin = Origin::parse(origin)?;
        if !intake.ip().is_loopback() || intake.port() == 0 || !origin.loopback() {
            return Err(ConfigError(
                "Development media endpoints must be loopback with a nonzero intake port.",
            ));
        }
        if token.len() != 64
            || !token
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(ConfigError(
                "Public intake token must be 64 lowercase hexadecimal characters.",
            ));
        }
        Ok(Self {
            intake,
            token: token.into(),
            origin,
        })
    }

    pub(crate) fn from_env(
        enabled: bool,
        origins: &[Origin; 3],
    ) -> Result<Option<Self>, ConfigError> {
        if !enabled {
            if [
                "PUBLIC_MEDIA_PROFILE",
                "PUBLIC_INTAKE_ADDR",
                "PUBLIC_INTAKE_TOKEN",
            ]
            .iter()
            .any(|key| env::var_os(key).is_some())
            {
                return Err(ConfigError(
                    "Public media configuration requires explicit development media enablement.",
                ));
            }
            return Ok(None);
        }
        let settings = Self::development(
            &env::var("PUBLIC_INTAKE_ADDR")
                .map_err(|_| ConfigError("PUBLIC_INTAKE_ADDR is required."))?,
            &env::var("PUBLIC_INTAKE_TOKEN")
                .map_err(|_| ConfigError("PUBLIC_INTAKE_TOKEN is required."))?,
            &origins[2].as_string(),
        )?;
        Ok(Some(settings))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn intake_has_no_dns_proxy_or_external_destination() {
        let token = "a".repeat(64);
        assert!(
            PublicMediaSettings::development("127.0.0.1:4000", &token, "http://localhost:4001")
                .is_ok()
        );
        for addr in [
            "localhost:4000",
            "0.0.0.0:4000",
            "192.0.2.1:4000",
            "127.0.0.1:0",
            "http://127.0.0.1:4000/path",
        ] {
            assert!(
                PublicMediaSettings::development(addr, &token, "http://localhost:4001").is_err()
            );
        }
        for token in [
            "A".repeat(64),
            "a".repeat(63),
            "a".repeat(65),
            String::new(),
        ] {
            assert!(
                PublicMediaSettings::development("127.0.0.1:4000", &token, "http://localhost:4001")
                    .is_err()
            );
        }
        assert!(
            PublicMediaSettings::development("127.0.0.1:4000", &token, "https://example.org")
                .is_err()
        );
    }
}
