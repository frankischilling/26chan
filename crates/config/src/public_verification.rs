use crate::ConfigError;
use std::{env, net::SocketAddr};

#[derive(Clone)]
pub struct PublicVerificationSettings {
    endpoint: SocketAddr,
    token: String,
}
impl PublicVerificationSettings {
    pub fn parse(endpoint: Option<&str>, token: Option<&str>) -> Result<Option<Self>, ConfigError> {
        let (endpoint, token) = match (endpoint, token) {
            (None, None) => return Ok(None),
            (Some(endpoint), Some(token)) => (endpoint, token),
            _ => {
                return Err(ConfigError(
                    "PUBLIC_VERIFICATION_ADDR and PUBLIC_VERIFICATION_TOKEN must be set together.",
                ));
            }
        };
        let endpoint: SocketAddr = endpoint
            .parse()
            .map_err(|_| ConfigError("Invalid verification endpoint."))?;
        if !endpoint.ip().is_loopback()
            || endpoint.port() == 0
            || token.len() != 64
            || !token
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(ConfigError(
                "Verification requires a numeric loopback endpoint and a separate service credential.",
            ));
        }
        Ok(Some(Self {
            endpoint,
            token: token.into(),
        }))
    }
    pub(crate) fn from_env() -> Result<Option<Self>, ConfigError> {
        for (name, value) in env::vars_os() {
            let name = name.to_string_lossy().to_ascii_uppercase();
            if !value.is_empty()
                && matches!(
                    name.as_str(),
                    "VERIFICATION_DATABASE_URL"
                        | "VERIFICATION_MAIL_DATABASE_URL"
                        | "TEST_VERIFICATION_DATABASE_URL"
                        | "TEST_VERIFICATION_MAIL_DATABASE_URL"
                        | "VERIFICATION_KEY"
                        | "VERIFICATION_MAIL_KEY"
                        | "VERIFICATION_SERVICE_TOKEN"
                )
            {
                return Err(ConfigError(
                    "The public runtime must not inherit private verification credentials.",
                ));
            }
        }
        let value = |name| match env::var(name) {
            Ok(value) => Ok(Some(value)),
            Err(env::VarError::NotPresent) => Ok(None),
            Err(_) => Err(ConfigError("Invalid verification configuration.")),
        };
        let endpoint = value("PUBLIC_VERIFICATION_ADDR")?;
        let token = value("PUBLIC_VERIFICATION_TOKEN")?;
        if token.as_ref().is_some_and(|token| {
            env::var("PUBLIC_INTAKE_TOKEN").is_ok_and(|intake| intake == *token)
        }) {
            return Err(ConfigError(
                "Verification and intake require separate service credentials.",
            ));
        }
        Self::parse(endpoint.as_deref(), token.as_deref())
    }
    pub fn endpoint(&self) -> SocketAddr {
        self.endpoint
    }
    pub fn token(&self) -> &str {
        &self.token
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn optional_service_is_all_or_nothing_and_never_resolves_remote_hosts() {
        assert!(
            PublicVerificationSettings::parse(None, None)
                .unwrap()
                .is_none()
        );
        assert!(
            PublicVerificationSettings::parse(Some("127.0.0.1:9081"), Some(&"ab".repeat(32)))
                .unwrap()
                .is_some()
        );
        for endpoint in [
            "localhost:9081",
            "https://127.0.0.1:9081",
            "192.0.2.1:9081",
            "0.0.0.0:9081",
            "127.0.0.1:0",
        ] {
            assert!(
                PublicVerificationSettings::parse(Some(endpoint), Some(&"ab".repeat(32))).is_err()
            );
        }
        assert!(PublicVerificationSettings::parse(Some("127.0.0.1:9081"), None).is_err());
        assert!(PublicVerificationSettings::parse(None, Some(&"ab".repeat(32))).is_err());
        assert!(PublicVerificationSettings::parse(Some("127.0.0.1:9081"), Some("")).is_err());
    }
}
