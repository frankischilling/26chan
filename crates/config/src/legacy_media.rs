use crate::ConfigError;
use std::{env, net::IpAddr};
use url::Url;

/// Offline operator credentials, never part of a public/staff/media runtime.
pub struct MediaBackfillSettings {
    pub database_url: String,
    pub group_read: bool,
}
impl MediaBackfillSettings {
    pub fn from_env() -> Result<Self, ConfigError> {
        if env::var("APP_ENV").as_deref() != Ok("development") {
            return Err(ConfigError(
                "Legacy upgrade requires explicit development mode.",
            ));
        }
        for key in [
            "DATABASE_URL",
            "TEST_PUBLIC_DATABASE_URL",
            "AUTH_DATABASE_URL",
            "STAFF_DATABASE_URL",
            "MEDIA_DATABASE_URL",
            "MEDIA_READ_DATABASE_URL",
            "MONITOR_DATABASE_URL",
            "INTAKE_DATABASE_URL",
            "PUBLIC_INTAKE_TOKEN",
        ] {
            if env::var_os(key).is_some_and(|value| !value.is_empty()) {
                return Err(ConfigError(
                    "Legacy upgrade received unrelated runtime credentials.",
                ));
            }
        }
        let database_url = env::var("MIGRATION_DATABASE_URL")
            .map_err(|_| ConfigError("Offline migration credential required."))?;
        let url =
            Url::parse(&database_url).map_err(|_| ConfigError("Invalid offline database URL."))?;
        if !matches!(url.scheme(), "postgres" | "postgresql")
            || url.username() != "board_migrator"
            || url.password().is_none_or(|password| password.is_empty())
            || url.path().len() < 2
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.host_str().is_some_and(|host| {
                host == "localhost"
                    || host
                        .trim_matches(['[', ']'])
                        .parse::<IpAddr>()
                        .is_ok_and(|ip| ip.is_loopback())
            })
        {
            return Err(ConfigError(
                "Legacy upgrade requires a dedicated loopback migration login.",
            ));
        }
        let group_read = match env::var("MEDIA_GROUP_READ") {
            Err(env::VarError::NotPresent) => false,
            Ok(value) if value == "false" => false,
            Ok(value) if value == "true" => true,
            _ => return Err(ConfigError("MEDIA_GROUP_READ must be true or false.")),
        };
        Ok(Self {
            database_url,
            group_read,
        })
    }
}
