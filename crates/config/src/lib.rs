#![forbid(unsafe_code)]

use std::{
    env,
    net::{IpAddr, SocketAddr},
};
use url::Url;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ConfigError(pub &'static str);

#[derive(Clone)]
pub struct Origin(Url);

impl Origin {
    pub fn parse(input: &str) -> Result<Self, ConfigError> {
        let url = Url::parse(input).map_err(|_| ConfigError("Invalid origin."))?;
        if !matches!(url.scheme(), "http" | "https")
            || !url.has_host()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(ConfigError(
                "Origins require HTTP(S), a host, and no credentials, path, query, or fragment.",
            ));
        }
        Ok(Self(url))
    }

    pub fn as_string(&self) -> String {
        self.0.origin().ascii_serialization()
    }
    fn loopback(&self) -> bool {
        self.0.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .trim_matches(['[', ']'])
                    .parse::<IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        })
    }
    fn domain(&self) -> Result<&str, ConfigError> {
        self.0
            .host_str()
            .and_then(psl::domain_str)
            .ok_or(ConfigError(
                "Production origins need registrable DNS domains.",
            ))
    }
}

pub fn validate_origins(
    public: &str,
    staff: &str,
    media: &str,
    production: bool,
) -> Result<[Origin; 3], ConfigError> {
    let origins = [
        Origin::parse(public)?,
        Origin::parse(staff)?,
        Origin::parse(media)?,
    ];
    if origins[0].as_string() == origins[1].as_string()
        || origins[0].as_string() == origins[2].as_string()
        || origins[1].as_string() == origins[2].as_string()
    {
        return Err(ConfigError(
            "Public, staff, and media origins must be distinct.",
        ));
    }
    if !production {
        if !origins.iter().all(Origin::loopback) {
            return Err(ConfigError("Development origins must be loopback."));
        }
        return Ok(origins);
    }
    if origins
        .iter()
        .any(|o| o.0.scheme() != "https" || o.loopback())
    {
        return Err(ConfigError(
            "Production origins require HTTPS and non-loopback DNS domains.",
        ));
    }
    if origins[2].domain()? == origins[0].domain()?
        || origins[2].domain()? == origins[1].domain()?
    {
        return Err(ConfigError(
            "Media needs a different registrable domain from both applications.",
        ));
    }
    Ok(origins)
}

#[derive(Clone)]
pub struct Settings {
    pub database_url: String,
    pub bind: SocketAddr,
    pub public_origin: Origin,
    pub production: bool,
}

impl Settings {
    pub fn from_env() -> Result<Self, ConfigError> {
        if ["MIGRATION_DATABASE_URL", "STAFF_DATABASE_URL"]
            .iter()
            .any(|name| env::var(name).is_ok_and(|value| !value.is_empty()))
        {
            return Err(ConfigError(
                "Operator or staff credentials must not be inherited by the public runtime.",
            ));
        }
        let production = match env::var("APP_ENV").as_deref().unwrap_or("development") {
            "development" => false,
            "production" => true,
            _ => return Err(ConfigError("APP_ENV must be development or production.")),
        };
        if env::var("MEDIA_ENABLED").as_deref().unwrap_or("false") != "false" {
            return Err(ConfigError(
                "Media processing is unavailable; MEDIA_ENABLED must be false.",
            ));
        }
        let public = env::var("PUBLIC_ORIGIN").unwrap_or_else(|_| "http://127.0.0.1:3000".into());
        let staff = env::var("STAFF_ORIGIN").unwrap_or_else(|_| "http://127.0.0.1:3001".into());
        let media = env::var("MEDIA_ORIGIN").unwrap_or_else(|_| "http://127.0.0.1:3002".into());
        let [public_origin, _, _] = validate_origins(&public, &staff, &media, production)?;
        let database_url =
            env::var("DATABASE_URL").map_err(|_| ConfigError("DATABASE_URL is required."))?;
        let parsed = Url::parse(&database_url).map_err(|_| ConfigError("Invalid database URL."))?;
        if !matches!(parsed.scheme(), "postgres" | "postgresql")
            || parsed.username() != "board_public"
        {
            return Err(ConfigError(
                "The public runtime requires the board_public PostgreSQL login.",
            ));
        }
        validate_tls(&parsed, production)?;
        let bind: SocketAddr = env::var("BIND_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:3000".into())
            .parse()
            .map_err(|_| ConfigError("Invalid BIND_ADDR."))?;
        if !production && !bind.ip().is_loopback() {
            return Err(ConfigError("Development must bind to loopback."));
        }
        Ok(Self {
            database_url,
            bind,
            public_origin,
            production,
        })
    }
}

fn validate_tls(parsed: &Url, production: bool) -> Result<(), ConfigError> {
    let options: Vec<_> = parsed
        .query_pairs()
        .filter(|(k, _)| k == "sslmode" || k == "ssl-mode")
        .collect();
    if production
        && !(options.len() == 1 && options[0].0 == "sslmode" && options[0].1 == "verify-full")
    {
        return Err(ConfigError(
            "Production database connections require sslmode=verify-full.",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_tls_rejects_conflicting_and_alias_options() {
        for query in [
            "sslmode=verify-full&sslmode=disable",
            "sslmode=verify-full&ssl-mode=prefer",
            "ssl-mode=disable&sslmode=verify-full",
            "sslmode=verify-full&sslmode=verify-full",
        ] {
            let parsed = Url::parse(&format!(
                "postgres://board_public@example.com/board?{query}"
            ))
            .unwrap();
            assert!(validate_tls(&parsed, true).is_err(), "{query}");
        }
    }

    #[test]
    fn origin_validation_rejects_credentials_paths_and_same_site_media() {
        for input in [
            "https://user:password@board.example",
            "https://board.example/path",
            "https://board.example/?x=1",
            "https://board.example/#x",
        ] {
            assert!(Origin::parse(input).is_err(), "{input}");
        }
        assert!(
            validate_origins(
                "https://board.example.com",
                "https://staff.example.com",
                "https://media.example.com",
                true
            )
            .is_err()
        );
        assert!(
            validate_origins(
                "https://board.example.co.uk",
                "https://staff.example.co.uk",
                "https://media.example.co.uk",
                true
            )
            .is_err()
        );
        assert!(
            validate_origins(
                "https://board.example.com",
                "https://staff.example.com",
                "https://example.net",
                true
            )
            .is_ok()
        );
        assert!(
            validate_origins(
                "http://board.example.com",
                "https://staff.example.com",
                "https://example.net",
                true
            )
            .is_err()
        );
    }

    #[test]
    fn development_exception_is_only_loopback() {
        assert!(
            validate_origins(
                "http://127.0.0.1:3000",
                "http://127.0.0.1:3001",
                "http://127.0.0.1:3002",
                false
            )
            .is_ok()
        );
        assert!(
            validate_origins(
                "http://board.example.com",
                "http://staff.example.com",
                "http://example.net",
                false
            )
            .is_err()
        );
    }
}
