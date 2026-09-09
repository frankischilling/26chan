use std::net::{IpAddr, SocketAddr};
use url::Url;

#[derive(Clone)]
pub struct Config {
    pub origin: String,
    pub bind: SocketAddr,
    pub production: bool,
    pub auth_database: String,
    pub staff_database: String,
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
        ] {
            if std::env::var(key).is_ok_and(|s| !s.is_empty()) {
                return Err("Staff runtime received an unrelated database credential");
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
        board_config::validate_origins(&public, &origin, &media, production)
            .map_err(|_| "Invalid application origin separation")?;
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
        if !valid_database(&auth_database, "board_auth", production)
            || !valid_database(&staff_database, "board_staff", production)
        {
            return Err(
                "Staff databases require their dedicated PostgreSQL login and unambiguous verified TLS in production",
            );
        }
        Ok(Self {
            origin,
            bind,
            production,
            auth_database,
            staff_database,
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
