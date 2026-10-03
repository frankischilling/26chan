use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;
use url::Url;

#[derive(Clone)]
pub struct Config {
    pub origin: String,
    pub public_origin: String,
    pub media_origin: String,
    pub bind: SocketAddr,
    pub production: bool,
    pub auth_database: String,
    pub staff_database: String,
    pub idle_timeout: Duration,
    pub tripcode_key: Option<Arc<board_domain::identity::SecureKey>>,
    pub poster_id_key: Option<Arc<board_domain::poster_id::PosterIdKey>>,
    pub country_database: Option<Arc<board_domain::country::CountryDatabase>>,
    pub proxy: Option<board_config::PublicProxy>,
}

pub fn parse_staff_poster_id_key(
    value: Option<&str>,
) -> Result<Option<Arc<board_domain::poster_id::PosterIdKey>>, &'static str> {
    value
        .map(|value| {
            board_domain::poster_id::PosterIdKey::parse(value)
                .map(Arc::new)
                .map_err(|_| {
                    "STAFF_POSTER_ID_KEY must contain 64 hexadecimal digits and cannot be all zeroes"
                })
        })
        .transpose()
}

pub fn load_staff_country_database(
    path: Option<&std::path::Path>,
) -> Result<Option<Arc<board_domain::country::CountryDatabase>>, &'static str> {
    path.map(|path| {
        board_domain::country::CountryDatabase::load(path)
            .map(Arc::new)
            .map_err(|_| "Invalid STAFF_COUNTRY_DATABASE")
    })
    .transpose()
}

pub fn parse_staff_tripcode_key(
    value: Option<&str>,
) -> Result<Option<Arc<board_domain::identity::SecureKey>>, &'static str> {
    value.map(|value| board_domain::identity::SecureKey::parse(value).map(Arc::new)
        .map_err(|_| "STAFF_TRIPCODE_KEY must contain 64 hexadecimal digits and cannot be all zeroes"))
        .transpose()
}
pub fn parse_staff_idle_timeout(value: Option<&str>) -> Result<Duration, &'static str> {
    let seconds = match value {
        None => 900,
        Some(value) => value
            .parse::<u64>()
            .map_err(|_| "Invalid STAFF_IDLE_TIMEOUT_SECONDS")?,
    };
    if !(60..=3600).contains(&seconds) {
        return Err("Invalid STAFF_IDLE_TIMEOUT_SECONDS");
    }
    Ok(Duration::from_secs(seconds))
}
fn valid_database(value: &str, identity: &str, production: bool) -> bool {
    let Ok(url) = Url::parse(value) else {
        return false;
    };
    if !matches!(url.scheme(), "postgres" | "postgresql")
        || url.username() != identity
        || url.host_str().is_none()
        || url.fragment().is_some()
        || url.path().len() < 2
    {
        return false;
    }
    if !production
        && !url.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .trim_matches(['[', ']'])
                    .parse::<IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        })
    {
        return false;
    }
    let mut keys = std::collections::HashSet::new();
    let mut verified = false;
    for (key, value) in url.query_pairs() {
        if !matches!(
            key.as_ref(),
            "sslmode" | "sslrootcert" | "sslcert" | "sslkey" | "application_name"
        ) || !keys.insert(key.to_string())
        {
            return false;
        }
        if key == "sslmode" {
            verified = value == "verify-full";
        }
    }
    !production || verified
}
pub fn valid_origin(origin: &str, production: bool) -> bool {
    let Ok(url) = Url::parse(origin) else {
        return false;
    };
    let loopback = url
        .host_str()
        .is_some_and(|h| h == "localhost" || h.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback()));
    url.origin().ascii_serialization() == origin
        && url.username().is_empty()
        && url.password().is_none()
        && (url.scheme() == "https" || (!production && loopback && url.scheme() == "http"))
}
impl Config {
    pub fn from_env() -> Result<Self, &'static str> {
        for key in [
            "MIGRATION_DATABASE_URL",
            "DATABASE_URL",
            "TEST_PUBLIC_DATABASE_URL",
            "MEDIA_DATABASE_URL",
            "MEDIA_READ_DATABASE_URL",
            "MONITOR_DATABASE_URL",
            "INTAKE_DATABASE_URL",
            "PUBLIC_INTAKE_TOKEN",
            "TRIPCODE_KEY",
            "POSTER_ID_KEY",
            "COUNTRY_DATABASE",
            "PUBLIC_PROXY_SOCKET",
            "PUBLIC_PROXY_UID",
        ] {
            if std::env::var_os(key).is_some_and(|s| !s.is_empty()) {
                return Err(
                    "Staff runtime received an unrelated database credential or identity source",
                );
            }
        }
        let production = match std::env::var("STAFF_MODE").as_deref() {
            Ok("production") => true,
            Ok("development") => false,
            _ => return Err("STAFF_MODE must be production or development"),
        };
        let origin = std::env::var("STAFF_ORIGIN").map_err(|_| "STAFF_ORIGIN required")?;
        if !valid_origin(&origin, production) {
            return Err("Invalid staff origin");
        }
        let public = std::env::var("PUBLIC_ORIGIN").map_err(|_| "PUBLIC_ORIGIN required")?;
        let media = std::env::var("MEDIA_ORIGIN").map_err(|_| "MEDIA_ORIGIN required")?;
        let origins = board_config::validate_origins(&public, &origin, &media, production)
            .map_err(|_| "Invalid application origin separation")?;
        if Url::parse(&origin)
            .map_err(|_| "Invalid staff origin")?
            .host_str()
            == Url::parse(&media)
                .map_err(|_| "Invalid media origin")?
                .host_str()
        {
            return Err("Staff and media need different cookie hostnames, including development");
        }
        let bind: SocketAddr = std::env::var("STAFF_BIND")
            .map_err(|_| "STAFF_BIND required")?
            .parse()
            .map_err(|_| "Invalid staff bind")?;
        // TLS terminates at a local proxy. Development is also loopback-only.
        if !bind.ip().is_loopback() {
            return Err("Staff must bind loopback");
        }
        let auth_database =
            std::env::var("AUTH_DATABASE_URL").map_err(|_| "AUTH_DATABASE_URL required")?;
        let staff_database =
            std::env::var("STAFF_DATABASE_URL").map_err(|_| "STAFF_DATABASE_URL required")?;
        let idle_timeout = match std::env::var("STAFF_IDLE_TIMEOUT_SECONDS") {
            Ok(value) => parse_staff_idle_timeout(Some(&value))?,
            Err(std::env::VarError::NotPresent) => parse_staff_idle_timeout(None)?,
            Err(std::env::VarError::NotUnicode(_)) => {
                return Err("Invalid STAFF_IDLE_TIMEOUT_SECONDS");
            }
        };
        let tripcode_key = match std::env::var("STAFF_TRIPCODE_KEY") {
            Ok(value) => parse_staff_tripcode_key(Some(&value))?,
            Err(std::env::VarError::NotPresent) => None,
            Err(std::env::VarError::NotUnicode(_)) => return Err("Invalid STAFF_TRIPCODE_KEY"),
        };
        let poster_id_key = match std::env::var("STAFF_POSTER_ID_KEY") {
            Ok(value) => parse_staff_poster_id_key(Some(&value))?,
            Err(std::env::VarError::NotPresent) => None,
            Err(std::env::VarError::NotUnicode(_)) => return Err("Invalid STAFF_POSTER_ID_KEY"),
        };
        let country_database = load_staff_country_database(
            std::env::var_os("STAFF_COUNTRY_DATABASE")
                .as_deref()
                .map(std::path::Path::new),
        )?;
        let proxy_value = |name| {
            std::env::var_os(name)
                .map(|value| {
                    value
                        .into_string()
                        .map_err(|_| "Invalid staff proxy setting")
                })
                .transpose()
        };
        let proxy_socket = proxy_value("STAFF_PROXY_SOCKET")?;
        let proxy_uid = proxy_value("STAFF_PROXY_UID")?;
        let proxy = if proxy_socket.is_none() && proxy_uid.is_none() {
            None
        } else {
            board_config::PublicProxy::from_values(
                proxy_socket.as_deref(),
                proxy_uid.as_deref(),
                production,
            )
            .map_err(|_| "Invalid staff proxy setting")?
        };
        if !valid_database(&auth_database, "board_auth", production)
            || !valid_database(&staff_database, "board_staff", production)
        {
            return Err(
                "Staff databases require their dedicated PostgreSQL login and unambiguous verified TLS in production",
            );
        }
        Ok(Self {
            origin,
            public_origin: origins[0].as_string(),
            media_origin: origins[2].as_string(),
            bind,
            production,
            auth_database,
            staff_database,
            idle_timeout,
            tripcode_key,
            poster_id_key,
            country_database,
            proxy,
        })
    }
    pub fn cookie_name(&self) -> &'static str {
        if self.production {
            "__Host-staff"
        } else {
            "staff"
        }
    }
    pub fn cookie(&self, name: &str, value: &str, age: u32) -> String {
        format!(
            "{name}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age={age}{}",
            if self.production { "; Secure" } else { "" }
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn staff_poster_key_preserves_public_identity_without_echoing_invalid_input() {
        assert!(parse_staff_poster_id_key(None).unwrap().is_none());
        let value = "12".repeat(32);
        let staff = parse_staff_poster_id_key(Some(&value)).unwrap().unwrap();
        let public = board_domain::poster_id::PosterIdKey::parse(&value).unwrap();
        for peer in ["81.2.69.142", "::ffff:81.2.69.142", "2001:218::"] {
            let peer = peer.parse().unwrap();
            assert_eq!(
                staff.label("g", 42, peer).unwrap(),
                public.label("g", 42, peer).unwrap()
            );
            let staff_count = staff.count_context("g", 42, peer).unwrap();
            let public_count = public.count_context("g", 42, peer).unwrap();
            assert_eq!(staff_count.fingerprint, public_count.fingerprint);
            assert_eq!(staff_count.epoch, public_count.epoch);
            assert_eq!(
                staff.robot9000_fingerprint("g", peer).unwrap(),
                public.robot9000_fingerprint("g", peer).unwrap()
            );
        }
        for value in [
            "",
            "owned-secret-invalid",
            &"00".repeat(32),
            &"g1".repeat(32),
            &"11".repeat(31),
        ] {
            assert_eq!(
                parse_staff_poster_id_key(Some(value)).err().unwrap(),
                "STAFF_POSTER_ID_KEY must contain 64 hexadecimal digits and cannot be all zeroes"
            );
        }
    }

    #[test]
    fn staff_country_data_uses_the_bounded_verified_operator_loader() {
        assert!(load_staff_country_database(None).unwrap().is_none());
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../crates/domain/tests/fixtures");
        let path = root.join("GeoIP2-Country-Test.mmdb");
        let db = load_staff_country_database(Some(&path)).unwrap().unwrap();
        assert_eq!(
            db.lookup("81.2.69.142".parse().unwrap()).unwrap().code,
            "GB"
        );
        assert_eq!(
            db.lookup("::ffff:81.2.69.142".parse().unwrap())
                .unwrap()
                .code,
            "GB"
        );
        assert_eq!(db.lookup("2001:218::".parse().unwrap()).unwrap().code, "JP");
        for path in [
            std::path::PathBuf::from("relative-private.mmdb"),
            root.clone(),
            root.join("missing-private.mmdb"),
        ] {
            assert_eq!(
                load_staff_country_database(Some(&path)).err().unwrap(),
                "Invalid STAFF_COUNTRY_DATABASE"
            );
        }
    }
    #[test]
    fn staff_tripcode_key_is_optional_and_validated_without_exposing_input() {
        assert!(parse_staff_tripcode_key(None).unwrap().is_none());
        assert!(
            parse_staff_tripcode_key(Some(&"11".repeat(32)))
                .unwrap()
                .is_some()
        );
        for value in [
            "",
            "secret",
            &"00".repeat(32),
            &"g1".repeat(32),
            &"11".repeat(31),
        ] {
            assert!(parse_staff_tripcode_key(Some(value)).is_err());
        }
    }
    #[test]
    fn idle_timeout_policy_defaults_and_rejects_invalid_values() {
        assert_eq!(
            parse_staff_idle_timeout(None).unwrap(),
            std::time::Duration::from_secs(900)
        );
        for value in ["", "0", "59", "3601", "minutes", "900 ", "-1"] {
            assert!(parse_staff_idle_timeout(Some(value)).is_err(), "{value}");
        }
        assert_eq!(
            parse_staff_idle_timeout(Some("60")).unwrap(),
            std::time::Duration::from_secs(60)
        );
        assert_eq!(
            parse_staff_idle_timeout(Some("3600")).unwrap(),
            std::time::Duration::from_secs(3600)
        );
    }
    #[test]
    fn staff_database_requires_identity_and_unambiguous_verified_tls() {
        for query in [
            "",
            "sslmode=disable",
            "ssl-mode=verify-full",
            "sslmode=verify-full&sslmode=disable",
            "sslmode=verify-full&ssl-mode=require",
        ] {
            assert!(!valid_database(
                &format!("postgres://board_auth:secret@db.example.org/imageboard?{query}"),
                "board_auth",
                true
            ));
        }
        assert!(valid_database(
            "postgres://board_auth:secret@db.example.org/imageboard?sslmode=verify-full",
            "board_auth",
            true
        ));
        assert!(!valid_database(
            "postgres://board_migrator:secret@db.example.org/imageboard?sslmode=verify-full",
            "board_auth",
            true
        ));
        assert!(!valid_database(
            "https://board_auth:secret@db.example.org/imageboard?sslmode=verify-full",
            "board_auth",
            true
        ));
        assert!(!valid_database(
            "postgres://board_auth:secret@db.example.org/imageboard?sslmode=verify-full&user=board_migrator",
            "board_auth",
            true
        ));
        assert!(valid_database(
            "postgres://board_staff:secret@127.0.0.1:55432/imageboard",
            "board_staff",
            false
        ));
    }
}
