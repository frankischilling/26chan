use crate::ConfigError;
use std::{collections::HashSet, net::IpAddr};
use url::Url;

pub struct MonitorSettings {
    pub database_url: String,
}

impl MonitorSettings {
    pub fn from_env() -> Result<Self, ConfigError> {
        for name in [
            "DATABASE_URL",
            "TEST_PUBLIC_DATABASE_URL",
            "MIGRATION_DATABASE_URL",
            "MEDIA_DATABASE_URL",
            "MEDIA_READ_DATABASE_URL",
            "AUTH_DATABASE_URL",
            "STAFF_DATABASE_URL",
            "INTAKE_DATABASE_URL",
            "PGHOSTADDR",
            "PGHOST",
            "PGPORT",
            "PGUSER",
            "PGDATABASE",
            "PGPASSWORD",
            "PGPASSFILE",
            "PGSSLROOTCERT",
            "PGSSLCERT",
            "PGSSLKEY",
            "PGSSLMODE",
            "PGAPPNAME",
            "PGOPTIONS",
        ] {
            if std::env::var_os(name).is_some_and(|value| !value.is_empty()) {
                return Err(ConfigError(
                    "Observer received an unrelated database credential.",
                ));
            }
        }
        let mode =
            std::env::var("APP_ENV").map_err(|_| ConfigError("Observer APP_ENV is required."))?;
        let url = std::env::var("MONITOR_DATABASE_URL")
            .map_err(|_| ConfigError("MONITOR_DATABASE_URL is required."))?;
        Self::parse(&mode, &url)
    }

    pub fn parse(mode: &str, value: &str) -> Result<Self, ConfigError> {
        let production = match mode {
            "development" => false,
            "production" => true,
            _ => {
                return Err(ConfigError(
                    "Observer APP_ENV must be development or production.",
                ));
            }
        };
        let invalid = || {
            ConfigError(
                "Observer requires its dedicated PostgreSQL login and verified configuration.",
            )
        };
        let url = Url::parse(value).map_err(|_| invalid())?;
        if !matches!(url.scheme(), "postgres" | "postgresql")
            || url.username() != "board_monitor"
            || url.host_str().is_none_or(|host| host.contains('%'))
            || url.password().is_none_or(str::is_empty)
            || url.port() == Some(0)
            || url.path().len() < 2
            || url.fragment().is_some()
        {
            return Err(invalid());
        }
        let loopback = url.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .trim_matches(['[', ']'])
                    .parse::<IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
        if !production && !loopback {
            return Err(invalid());
        }
        let mut keys = HashSet::new();
        let mut verified = false;
        for (key, value) in url.query_pairs() {
            if !matches!(
                key.as_ref(),
                "sslmode" | "sslrootcert" | "sslcert" | "sslkey" | "application_name"
            ) || !keys.insert(key.to_string())
            {
                return Err(invalid());
            }
            if key == "sslmode" {
                verified = value == "verify-full";
            }
        }
        if production && !verified {
            return Err(invalid());
        }
        Ok(Self {
            database_url: value.to_owned(),
        })
    }
}
