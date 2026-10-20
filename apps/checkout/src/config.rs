//! Local sandbox adapter configuration. Live payment providers remain disabled.
use board_domain::checkout::signature::{CallbackKey, Provider};
use std::{
    collections::BTreeMap,
    fmt,
    net::{IpAddr, SocketAddr},
};
pub struct Settings {
    pub bind: SocketAddr,
    pub database: String,
    pub key: CallbackKey,
}
#[derive(Debug)]
pub struct ConfigError;
impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("checkout configuration rejected")
    }
}
impl std::error::Error for ConfigError {}
impl Settings {
    pub fn from_env() -> Result<Self, ConfigError> {
        let mut values = BTreeMap::new();
        for (name, value) in std::env::vars_os() {
            let name = name
                .into_string()
                .map_err(|_| ConfigError)?
                .to_ascii_uppercase();
            let value = value.into_string().map_err(|_| ConfigError)?;
            if values.insert(name, value).is_some() {
                return Err(ConfigError);
            }
        }
        Self::parse(&values)
    }
    fn parse(values: &BTreeMap<String, String>) -> Result<Self, ConfigError> {
        for (name, value) in values {
            if value.is_empty() {
                continue;
            }
            if (name.ends_with("DATABASE_URL") && name != "CHECKOUT_DATABASE_URL")
                || name.starts_with("AWS_")
                || name.starts_with("AZURE_")
                || name.starts_with("GOOGLE_")
                || name.starts_with("DEPLOY_")
                || matches!(
                    name.as_str(),
                    "GH_TOKEN" | "GITHUB_TOKEN" | "SSH_AUTH_SOCK" | "DOCKER_HOST"
                )
                || ((name.ends_with("_KEY")
                    || name.ends_with("_TOKEN")
                    || name.ends_with("_PASSWORD")
                    || name.ends_with("_SECRET"))
                    && name != "CHECKOUT_CALLBACK_KEY")
            {
                return Err(ConfigError);
            }
        }
        let get = |name: &str| values.get(name).ok_or(ConfigError);
        if get("APP_ENV")? != "development" || get("CHECKOUT_PROVIDER")? != "sandbox" {
            return Err(ConfigError);
        }
        let bind: SocketAddr = get("CHECKOUT_BIND")?.parse().map_err(|_| ConfigError)?;
        if !bind.ip().is_loopback() || bind.port() == 0 {
            return Err(ConfigError);
        }
        let database = get("CHECKOUT_DATABASE_URL")?.clone();
        let url = url::Url::parse(&database).map_err(|_| ConfigError)?;
        let local = url.host_str().is_some_and(|h| {
            h == "localhost"
                || h.trim_matches(['[', ']'])
                    .parse::<IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
        if !matches!(url.scheme(), "postgres" | "postgresql")
            || url.username() != "board_checkout"
            || !local
            || url.query().is_some()
            || url.fragment().is_some()
            || url.password().is_none_or(str::is_empty)
            || url.path().len() <= 1
        {
            return Err(ConfigError);
        }
        let raw = get("CHECKOUT_CALLBACK_KEY")?;
        if url.password() == Some(raw.as_str()) {
            return Err(ConfigError);
        }
        Ok(Self {
            bind,
            database,
            key: CallbackKey::new(Provider::Sandbox, raw).ok_or(ConfigError)?,
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn values() -> BTreeMap<String, String> {
        [
            ("APP_ENV", "development"),
            ("CHECKOUT_PROVIDER", "sandbox"),
            ("CHECKOUT_BIND", "127.0.0.1:9083"),
            (
                "CHECKOUT_DATABASE_URL",
                "postgres://board_checkout:owned-password@127.0.0.1:55432/owned",
            ),
            (
                "CHECKOUT_CALLBACK_KEY",
                "owned-sandbox-key-12345678901234567890",
            ),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v.into()))
        .collect()
    }
    #[test]
    fn sandbox_requires_isolated_role_local_listener_and_explicit_development() {
        assert!(Settings::parse(&values()).is_ok());
        for (key, value) in [
            ("APP_ENV", "production"),
            ("CHECKOUT_PROVIDER", "stripe"),
            ("CHECKOUT_BIND", "0.0.0.0:9083"),
            ("DATABASE_URL", "postgres://other"),
            ("VERIFICATION_KEY", "secret"),
            (
                "CHECKOUT_DATABASE_URL",
                "postgres://board_migrator:password@127.0.0.1/owned",
            ),
        ] {
            let mut v = values();
            v.insert(key.into(), value.into());
            assert!(Settings::parse(&v).is_err(), "{key}");
        }
    }
}
