use crate::{ConfigError, MediaReaderSettings, Origin, validate_origins};
use std::{
    env,
    net::{IpAddr, SocketAddr},
    path::PathBuf,
};

pub struct MediaHttpSettings {
    pub reader: MediaReaderSettings,
    pub origin: Origin,
    pub bind: SocketAddr,
    pub approved_dir: PathBuf,
}

impl MediaHttpSettings {
    pub fn from_env() -> Result<Self, ConfigError> {
        let reader = MediaReaderSettings::from_env()?;
        let public = env::var("PUBLIC_ORIGIN").unwrap_or_else(|_| "http://127.0.0.1:3000".into());
        let staff = env::var("STAFF_ORIGIN").unwrap_or_else(|_| "http://127.0.0.1:3001".into());
        let media =
            env::var("MEDIA_ORIGIN").map_err(|_| ConfigError("MEDIA_ORIGIN is required."))?;
        let origins = validate_origins(&public, &staff, &media, false)?;
        if let Ok(api) = env::var("API_ORIGIN") {
            let api = Origin::parse(&api)?;
            if !api.loopback() || origins.iter().any(|o| o.as_string() == api.as_string()) {
                return Err(ConfigError(
                    "Development API origin must be loopback and distinct.",
                ));
            }
        }
        let origin = origins[2].clone();
        let bind: SocketAddr = env::var("MEDIA_BIND_ADDR")
            .map_err(|_| ConfigError("MEDIA_BIND_ADDR is required."))?
            .parse()
            .map_err(|_| ConfigError("Invalid MEDIA_BIND_ADDR."))?;
        if !bind.ip().is_loopback()
            || bind.port() == 0
            || origin.0.scheme() != "http"
            || origin.0.port_or_known_default() != Some(bind.port())
            || origin.0.host_str().is_some_and(|host| {
                host != "localhost"
                    && host.trim_matches(['[', ']']).parse::<IpAddr>() != Ok(bind.ip())
            })
        {
            return Err(ConfigError(
                "Development media HTTP needs a matching loopback HTTP origin and listener.",
            ));
        }
        let approved_dir = env::var_os("MEDIA_APPROVED_DIR")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or(ConfigError("MEDIA_APPROVED_DIR must be an absolute path."))?;
        Ok(Self {
            reader,
            origin,
            bind,
            approved_dir,
        })
    }
}
