#![forbid(unsafe_code)]

use std::{
    env,
    net::{IpAddr, SocketAddr},
};
use url::Url;

mod media_http;
pub use media_http::MediaHttpSettings;
mod monitor;
pub use monitor::MonitorSettings;

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
        self.0.domain().and_then(psl::domain_str).ok_or(ConfigError(
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
    if origins[0].0.host_str() == origins[1].0.host_str() {
        return Err(ConfigError(
            "Public and staff need different hostnames because cookies are not port-scoped.",
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
    pub api: Option<ApiListener>,
}

#[derive(Clone)]
pub struct ApiListener {
    pub origin: Origin,
    pub bind: SocketAddr,
}

impl ApiListener {
    fn parse(
        origin: Option<&str>,
        bind: Option<&str>,
        public_bind: SocketAddr,
        origins: &[Origin; 3],
        production: bool,
    ) -> Result<Option<Self>, ConfigError> {
        let (origin, bind) = match (origin, bind) {
            (None, None) => return Ok(None),
            (Some(origin), Some(bind)) => (origin, bind),
            _ => {
                return Err(ConfigError(
                    "API_ORIGIN and API_BIND_ADDR must be set together.",
                ));
            }
        };
        let origin = Origin::parse(origin)
            .map_err(|_| ConfigError("API_ORIGIN must be an HTTP(S) origin without a path."))?;
        if origins
            .iter()
            .any(|other| origin.as_string() == other.as_string())
        {
            return Err(ConfigError(
                "API_ORIGIN must differ from public, staff and media origins.",
            ));
        }
        if production {
            if origin.0.scheme() != "https" || origin.loopback() {
                return Err(ConfigError(
                    "Production API_ORIGIN requires HTTPS and a DNS domain.",
                ));
            }
            let domain = origin.domain().map_err(|_| {
                ConfigError("Production API_ORIGIN needs a registrable DNS domain.")
            })?;
            if domain == origins[2].domain()? {
                return Err(ConfigError(
                    "Media must use a different registrable domain from the API.",
                ));
            }
            if origin.0.host_str() == origins[1].0.host_str() {
                return Err(ConfigError(
                    "API and staff must use different cookie hostnames.",
                ));
            }
        } else if !origin.loopback() {
            return Err(ConfigError("Development API_ORIGIN must be loopback."));
        }
        let bind: SocketAddr = bind
            .parse()
            .map_err(|_| ConfigError("Invalid API_BIND_ADDR."))?;
        if !production && !bind.ip().is_loopback() {
            return Err(ConfigError("Development API_BIND_ADDR must be loopback."));
        }
        if bind == public_bind || bind.port() == 0 {
            return Err(ConfigError(
                "API_BIND_ADDR needs a nonzero port and a distinct listener address.",
            ));
        }
        Ok(Some(Self { origin, bind }))
    }
}

impl Settings {
    pub fn from_env() -> Result<Self, ConfigError> {
        if [
            "MIGRATION_DATABASE_URL",
            "STAFF_DATABASE_URL",
            "AUTH_DATABASE_URL",
            "MONITOR_DATABASE_URL",
            "INTAKE_DATABASE_URL",
        ]
        .iter()
        .any(|name| env::var_os(name).is_some_and(|value| !value.is_empty()))
        {
            return Err(ConfigError(
                "Operator or staff credentials must not be inherited by the public runtime.",
            ));
        }
        if ["MEDIA_DATABASE_URL", "MEDIA_READ_DATABASE_URL"]
            .iter()
            .any(|name| env::var_os(name).is_some_and(|value| !value.is_empty()))
        {
            return Err(ConfigError(
                "The public runtime must not inherit media credentials.",
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
        let origins = validate_origins(&public, &staff, &media, production)?;
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
        let api = ApiListener::parse(
            env::var("API_ORIGIN").ok().as_deref(),
            env::var("API_BIND_ADDR").ok().as_deref(),
            bind,
            &origins,
            production,
        )?;
        let [public_origin, _, _] = origins;
        Ok(Self {
            database_url,
            bind,
            public_origin,
            production,
            api,
        })
    }
}

pub struct MediaAdminSettings {
    pub database_url: String,
    pub quarantine: Option<std::path::PathBuf>,
    pub group_read: bool,
}

impl MediaAdminSettings {
    pub fn from_env() -> Result<Self, ConfigError> {
        if env::var("APP_ENV").as_deref().unwrap_or("development") != "development" {
            return Err(ConfigError(
                "Development intake only; isolated media processing remains unverified.",
            ));
        }
        if [
            "DATABASE_URL",
            "MIGRATION_DATABASE_URL",
            "STAFF_DATABASE_URL",
            "AUTH_DATABASE_URL",
            "TEST_PUBLIC_DATABASE_URL",
            "MONITOR_DATABASE_URL",
            "MEDIA_READ_DATABASE_URL",
            "INTAKE_DATABASE_URL",
        ]
        .iter()
        .any(|name| env::var_os(name).is_some_and(|value| !value.is_empty()))
        {
            return Err(ConfigError(
                "The media operator command must not inherit public, staff, migration or reader credentials.",
            ));
        }
        if env::var("MEDIA_ENABLED").as_deref().unwrap_or("false") != "false" {
            return Err(ConfigError(
                "Media processing is unavailable; MEDIA_ENABLED must be false.",
            ));
        }
        let database_url = env::var("MEDIA_DATABASE_URL")
            .map_err(|_| ConfigError("MEDIA_DATABASE_URL is required."))?;
        let parsed =
            Url::parse(&database_url).map_err(|_| ConfigError("Invalid media database URL."))?;
        let loopback = parsed.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .trim_matches(['[', ']'])
                    .parse::<IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
        if !matches!(parsed.scheme(), "postgres" | "postgresql")
            || parsed.username() != "board_media"
            || !loopback
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(ConfigError(
                "Development media intake requires a loopback board_media PostgreSQL login.",
            ));
        }
        let quarantine = env::var_os("MEDIA_QUARANTINE_DIR").map(std::path::PathBuf::from);
        if quarantine.as_ref().is_some_and(|path| !path.is_absolute()) {
            return Err(ConfigError(
                "MEDIA_QUARANTINE_DIR must be an absolute private path.",
            ));
        }
        Ok(Self {
            database_url,
            quarantine,
            group_read: match env::var("MEDIA_GROUP_READ").as_deref().unwrap_or("false") {
                "false" => false,
                "true" => true,
                _ => return Err(ConfigError("MEDIA_GROUP_READ must be true or false.")),
            },
        })
    }
}

pub struct MediaReaderSettings {
    pub database_url: String,
}

impl MediaReaderSettings {
    pub fn from_env() -> Result<Self, ConfigError> {
        if env::var("APP_ENV").as_deref() != Ok("development") {
            return Err(ConfigError(
                "Media reader commands require explicit development mode.",
            ));
        }
        if [
            "DATABASE_URL",
            "MIGRATION_DATABASE_URL",
            "MEDIA_DATABASE_URL",
            "STAFF_DATABASE_URL",
            "AUTH_DATABASE_URL",
            "TEST_PUBLIC_DATABASE_URL",
            "MONITOR_DATABASE_URL",
            "INTAKE_DATABASE_URL",
        ]
        .iter()
        .any(|name| env::var_os(name).is_some_and(|value| !value.is_empty()))
        {
            return Err(ConfigError(
                "Media readers must not inherit writer or application credentials.",
            ));
        }
        if env::var("MEDIA_ENABLED").as_deref().unwrap_or("false") != "false" {
            return Err(ConfigError("Public media must remain disabled."));
        }
        let database_url = env::var("MEDIA_READ_DATABASE_URL")
            .map_err(|_| ConfigError("MEDIA_READ_DATABASE_URL is required."))?;
        let parsed = Url::parse(&database_url)
            .map_err(|_| ConfigError("Invalid media reader database URL."))?;
        let loopback = parsed.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .trim_matches(['[', ']'])
                    .parse::<IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
        if !matches!(parsed.scheme(), "postgres" | "postgresql")
            || parsed.username() != "board_media_read"
            || !loopback
            || parsed.query().is_some()
            || parsed.fragment().is_some()
        {
            return Err(ConfigError(
                "Development media readers require a loopback board_media_read PostgreSQL login.",
            ));
        }
        Ok(Self { database_url })
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
    fn api_origin_preserves_staff_cookie_and_media_domain_boundaries() {
        let origins = validate_origins(
            "https://boards.example.com",
            "https://staff.example.com",
            "https://images.example.net",
            true,
        )
        .unwrap();
        for origin in [
            "http://api.example.com",
            "https://127.0.0.1:3003",
            "https://192.0.2.1",
            "https://[2001:db8::1]",
            "https://localhost",
            "https://api.example.net",
            "https://staff.example.com:8443",
            "https://boards.example.com",
            "https://staff.example.com",
            "https://images.example.net",
        ] {
            assert!(
                ApiListener::parse(
                    Some(origin),
                    Some("127.0.0.1:3003"),
                    "127.0.0.1:3000".parse().unwrap(),
                    &origins,
                    true
                )
                .is_err(),
                "{origin}"
            );
        }
        let api = ApiListener::parse(
            Some("https://api.example.com"),
            Some("127.0.0.1:3003"),
            "127.0.0.1:3000".parse().unwrap(),
            &origins,
            true,
        )
        .unwrap()
        .unwrap();
        assert_eq!(api.origin.as_string(), "https://api.example.com");
        assert_eq!(api.bind.to_string(), "127.0.0.1:3003");
    }

    #[test]
    fn api_listener_is_optional_and_development_stays_on_loopback() {
        let origins = validate_origins(
            "http://127.0.0.1:3000",
            "http://localhost:3001",
            "http://127.0.0.1:3002",
            false,
        )
        .unwrap();
        let public = "127.0.0.1:3000".parse().unwrap();
        assert!(
            ApiListener::parse(None, None, public, &origins, false)
                .unwrap()
                .is_none()
        );
        assert!(
            ApiListener::parse(
                Some("http://127.0.0.1:3003/"),
                Some("127.0.0.1:3003"),
                public,
                &origins,
                false
            )
            .unwrap()
            .is_some()
        );
        assert!(
            ApiListener::parse(
                Some("http://[::1]:3003"),
                Some("[::1]:3003"),
                public,
                &origins,
                false
            )
            .unwrap()
            .is_some()
        );
        assert!(
            ApiListener::parse(
                Some("http://127.0.0.1:3003"),
                Some("127.0.0.1:0"),
                public,
                &origins,
                false
            )
            .is_err()
        );
    }

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
    fn production_staff_cookie_host_cannot_be_shared_across_ports() {
        assert!(
            validate_origins(
                "https://board.example.com",
                "https://board.example.com:8443",
                "https://images.example.net",
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
