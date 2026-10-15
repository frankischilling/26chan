use board_domain::{challenge_secrets::VerificationKey, verification_mail::MailKey};
use board_verification_client::valid_token;
use std::{
    collections::BTreeMap,
    fmt,
    net::{IpAddr, SocketAddr},
};
use url::Url;

pub struct Settings {
    pub bind: SocketAddr,
    pub database_url: String,
    pub service_token: String,
    pub verification_key: VerificationKey,
    pub mail_key: MailKey,
}

#[derive(Debug)]
pub struct ConfigError;
impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("verification configuration rejected")
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
        Self::from_values(&values)
    }

    fn from_values(values: &BTreeMap<String, String>) -> Result<Self, ConfigError> {
        for (name, value) in values {
            if value.is_empty() {
                continue;
            }
            if (name.ends_with("DATABASE_URL") && name != "VERIFICATION_DATABASE_URL")
                || name.starts_with("AWS_")
                || name.starts_with("AZURE_")
                || name.starts_with("GOOGLE_")
                || name.starts_with("DEPLOY_")
                || name.starts_with("MEDIA_DISPATCH_")
                || matches!(
                    name.as_str(),
                    "GH_TOKEN"
                        | "GITHUB_TOKEN"
                        | "SSH_AUTH_SOCK"
                        | "DOCKER_HOST"
                        | "PUBLIC_VERIFICATION_TOKEN"
                )
                || ((name.ends_with("_KEY")
                    || name.ends_with("_TOKEN")
                    || name.ends_with("_PASSWORD")
                    || name.ends_with("_SECRET"))
                    && !matches!(
                        name.as_str(),
                        "VERIFICATION_KEY" | "VERIFICATION_MAIL_KEY" | "VERIFICATION_SERVICE_TOKEN"
                    ))
            {
                return Err(ConfigError);
            }
        }
        let get = |name: &str| values.get(name).ok_or(ConfigError);
        let bind: SocketAddr = get("VERIFICATION_BIND")?.parse().map_err(|_| ConfigError)?;
        if !bind.ip().is_loopback() || bind.port() == 0 {
            return Err(ConfigError);
        }
        let database_url = get("VERIFICATION_DATABASE_URL")?.clone();
        let database = Url::parse(&database_url).map_err(|_| ConfigError)?;
        let local = database.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .trim_matches(['[', ']'])
                    .parse::<IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
        if !matches!(database.scheme(), "postgres" | "postgresql")
            || database.username() != "board_verifier"
            || !local
            || database.query().is_some()
            || database.fragment().is_some()
            || database.password().is_none_or(str::is_empty)
            || database.path().len() <= 1
        {
            return Err(ConfigError);
        }
        let token = get("VERIFICATION_SERVICE_TOKEN")?;
        let verification = get("VERIFICATION_KEY")?;
        let mail = get("VERIFICATION_MAIL_KEY")?;
        let password = database.password().unwrap_or("");
        if !valid_token(token)
            || token == verification
            || token == mail
            || verification == mail
            || [token, verification, mail]
                .iter()
                .any(|secret| password == secret.as_str())
        {
            return Err(ConfigError);
        }
        Ok(Self {
            bind,
            database_url,
            service_token: token.clone(),
            verification_key: VerificationKey::parse(verification).ok_or(ConfigError)?,
            mail_key: MailKey::parse(mail).ok_or(ConfigError)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn values() -> BTreeMap<String, String> {
        [
            ("VERIFICATION_BIND", "127.0.0.1:9081".into()),
            (
                "VERIFICATION_DATABASE_URL",
                "postgres://board_verifier:owned-fixture@127.0.0.1:55432/imageboard".into(),
            ),
            ("VERIFICATION_SERVICE_TOKEN", "11".repeat(32)),
            ("VERIFICATION_KEY", "22".repeat(32)),
            ("VERIFICATION_MAIL_KEY", "33".repeat(32)),
        ]
        .into_iter()
        .map(|(k, v)| (k.into(), v))
        .collect()
    }
    #[test]
    fn local_isolated_configuration_accepts_distinct_secrets() {
        assert!(Settings::from_values(&values()).is_ok());
    }
    #[test]
    fn inherited_credentials_and_reused_secrets_are_rejected_without_disclosure() {
        for name in [
            "DATABASE_URL",
            "MIGRATION_DATABASE_URL",
            "TEST_PUBLIC_DATABASE_URL",
            "VERIFICATION_MAIL_DATABASE_URL",
            "STAFF_KEY",
            "PUBLIC_VERIFICATION_TOKEN",
            "AWS_PROFILE",
            "SSH_AUTH_SOCK",
        ] {
            let mut values = values();
            values.insert(name.into(), "private-unrelated-material".into());
            assert!(Settings::from_values(&values).is_err(), "{name}");
        }
        for name in ["VERIFICATION_KEY", "VERIFICATION_MAIL_KEY"] {
            let mut values = values();
            values.insert(name.into(), "11".repeat(32));
            assert!(Settings::from_values(&values).is_err());
        }
        assert_eq!(
            ConfigError.to_string(),
            "verification configuration rejected"
        );
    }
    #[test]
    fn remote_endpoints_wrong_roles_and_url_options_are_rejected() {
        for bind in ["0.0.0.0:9081", "192.0.2.2:9081", "127.0.0.1:0"] {
            let mut values = values();
            values.insert("VERIFICATION_BIND".into(), bind.into());
            assert!(Settings::from_values(&values).is_err());
        }
        for url in [
            "postgres://board_public:private@127.0.0.1/db",
            "postgres://board_verifier:private@192.0.2.1/db",
            "postgres://board_verifier@127.0.0.1/db",
            "postgres://board_verifier:private@127.0.0.1/db?options=unexpected",
        ] {
            let mut values = values();
            values.insert("VERIFICATION_DATABASE_URL".into(), url.into());
            assert!(Settings::from_values(&values).is_err());
        }
    }
}
