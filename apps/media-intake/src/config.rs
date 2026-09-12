use std::{
    env, fmt,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};
use url::Url;

pub struct Settings {
    pub bind: SocketAddr,
    pub database_url: String,
    pub quarantine_dir: PathBuf,
    pub token: String,
}

#[derive(Debug)]
pub struct ConfigError;
impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("media intake configuration rejected")
    }
}
impl std::error::Error for ConfigError {}

pub(crate) fn valid_token(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

impl Settings {
    pub fn from_env() -> Result<Self, ConfigError> {
        if env::var("MEDIA_INTAKE_MODE").as_deref() != Ok("development")
            || env::var_os("MEDIA_ENABLED").is_some_and(|v| v != "false")
            || env::vars_os().any(|(name, value)| {
                let name = name.to_string_lossy();
                !value.is_empty()
                    && ((name.ends_with("DATABASE_URL") && name != "INTAKE_DATABASE_URL")
                        || name.starts_with("AWS_")
                        || name.starts_with("AZURE_")
                        || name.starts_with("GOOGLE_")
                        || name.starts_with("MEDIA_DISPATCH_")
                        || name.starts_with("DEPLOY_")
                        || name == "GH_TOKEN"
                        || name == "GITHUB_TOKEN"
                        || name == "DOCKER_HOST"
                        || name == "SSH_AUTH_SOCK")
            })
        {
            return Err(ConfigError);
        }
        let bind: SocketAddr = env::var("MEDIA_INTAKE_BIND")
            .map_err(|_| ConfigError)?
            .parse()
            .map_err(|_| ConfigError)?;
        if !bind.ip().is_loopback() || bind.port() == 0 {
            return Err(ConfigError);
        }
        let database_url = env::var("INTAKE_DATABASE_URL").map_err(|_| ConfigError)?;
        let parsed = Url::parse(&database_url).map_err(|_| ConfigError)?;
        let loopback = parsed.host_str().is_some_and(|host| {
            host == "localhost"
                || host
                    .trim_matches(['[', ']'])
                    .parse::<IpAddr>()
                    .is_ok_and(|ip| ip.is_loopback())
        });
        if !matches!(parsed.scheme(), "postgres" | "postgresql")
            || parsed.username() != "board_media_intake"
            || !loopback
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || parsed.password().is_none_or(str::is_empty)
            || parsed.path().len() <= 1
        {
            return Err(ConfigError);
        }
        let quarantine_dir = env::var_os("MEDIA_QUARANTINE_DIR")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .ok_or(ConfigError)?;
        let token = env::var("MEDIA_INTAKE_TOKEN").map_err(|_| ConfigError)?;
        if !valid_token(&token) || env::var_os("METRICS_TOKEN").is_some_and(|v| v == token.as_str())
        {
            return Err(ConfigError);
        }
        Ok(Self {
            bind,
            database_url,
            quarantine_dir,
            token,
        })
    }
}
