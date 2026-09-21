use crate::ConfigError;
use std::{collections::BTreeMap, time::Duration};

/// Per-process budgets. Private fields keep unchecked values out of runtimes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PublicRequestLimits {
    active_requests: usize,
    connections: usize,
    hash_operations: usize,
    uploads: usize,
    writes_per_minute: u32,
    tracked_peers: usize,
    handler_timeout: Duration,
    header_timeout: Duration,
    connection_timeout: Duration,
    response_bytes: usize,
    response_buffer_bytes: usize,
}

impl Default for PublicRequestLimits {
    fn default() -> Self {
        Self::from_lookup(|_| None).expect("fixed public request defaults")
    }
}

impl PublicRequestLimits {
    pub const NAMES: [&'static str; 11] = [
        "PUBLIC_MAX_ACTIVE_REQUESTS",
        "PUBLIC_MAX_CONNECTIONS",
        "PUBLIC_MAX_HASH_OPERATIONS",
        "PUBLIC_MAX_UPLOADS",
        "PUBLIC_WRITES_PER_MINUTE",
        "PUBLIC_MAX_TRACKED_PEERS",
        "PUBLIC_HANDLER_TIMEOUT_MS",
        "PUBLIC_HEADER_TIMEOUT_MS",
        "PUBLIC_CONNECTION_TIMEOUT_MS",
        "PUBLIC_MAX_RESPONSE_BYTES",
        "PUBLIC_MAX_RESPONSE_BUFFER_BYTES",
    ];

    pub fn from_env() -> Result<Self, ConfigError> {
        let mut values = BTreeMap::new();
        for name in Self::NAMES {
            if let Some(value) = std::env::var_os(name) {
                values.insert(
                    name,
                    value.into_string().map_err(|_| {
                        ConfigError(
                            "Public request limit settings must contain Unicode decimal integers.",
                        )
                    })?,
                );
            }
        }
        Self::from_lookup(|name| values.get(name).cloned())
    }

    pub fn from_lookup(
        mut lookup: impl FnMut(&str) -> Option<String>,
    ) -> Result<Self, ConfigError> {
        let mut read = |name, default, minimum, maximum, error| {
            let Some(value) = lookup(name) else {
                return Ok(default);
            };
            if value.is_empty()
                || value.len() > 20
                || !value.bytes().all(|byte| byte.is_ascii_digit())
            {
                return Err(ConfigError(error));
            }
            let value = value.parse::<u64>().map_err(|_| ConfigError(error))?;
            if value < minimum || value > maximum {
                return Err(ConfigError(error));
            }
            Ok(value)
        };
        Ok(Self {
            active_requests: read(
                Self::NAMES[0],
                32,
                1,
                1024,
                "PUBLIC_MAX_ACTIVE_REQUESTS must be a decimal integer from 1 through 1024.",
            )? as usize,
            connections: read(
                Self::NAMES[1],
                128,
                1,
                4096,
                "PUBLIC_MAX_CONNECTIONS must be a decimal integer from 1 through 4096.",
            )? as usize,
            hash_operations: read(
                Self::NAMES[2],
                4,
                1,
                32,
                "PUBLIC_MAX_HASH_OPERATIONS must be a decimal integer from 1 through 32.",
            )? as usize,
            uploads: read(
                Self::NAMES[3],
                4,
                1,
                32,
                "PUBLIC_MAX_UPLOADS must be a decimal integer from 1 through 32.",
            )? as usize,
            writes_per_minute: read(
                Self::NAMES[4],
                30,
                1,
                10_000,
                "PUBLIC_WRITES_PER_MINUTE must be a decimal integer from 1 through 10000.",
            )? as u32,
            tracked_peers: read(
                Self::NAMES[5],
                10_000,
                1,
                100_000,
                "PUBLIC_MAX_TRACKED_PEERS must be a decimal integer from 1 through 100000.",
            )? as usize,
            handler_timeout: Duration::from_millis(read(
                Self::NAMES[6],
                10_000,
                1,
                120_000,
                "PUBLIC_HANDLER_TIMEOUT_MS must be a decimal integer from 1 through 120000.",
            )?),
            header_timeout: Duration::from_millis(read(
                Self::NAMES[7],
                10_000,
                1,
                120_000,
                "PUBLIC_HEADER_TIMEOUT_MS must be a decimal integer from 1 through 120000.",
            )?),
            connection_timeout: Duration::from_millis(read(
                Self::NAMES[8],
                120_000,
                1,
                600_000,
                "PUBLIC_CONNECTION_TIMEOUT_MS must be a decimal integer from 1 through 600000.",
            )?),
            response_bytes: read(
                Self::NAMES[9],
                33_554_432,
                1,
                268_435_456,
                "PUBLIC_MAX_RESPONSE_BYTES must be a decimal integer from 1 through 268435456.",
            )? as usize,
            response_buffer_bytes: read(
                Self::NAMES[10],
                134_217_728,
                4096,
                1_073_741_824,
                "PUBLIC_MAX_RESPONSE_BUFFER_BYTES must be a decimal integer from 4096 through 1073741824.",
            )? as usize,
        })
    }

    pub fn active_requests(self) -> usize {
        self.active_requests
    }
    pub fn connections(self) -> usize {
        self.connections
    }
    pub fn hash_operations(self) -> usize {
        self.hash_operations
    }
    pub fn uploads(self) -> usize {
        self.uploads
    }
    pub fn writes_per_minute(self) -> u32 {
        self.writes_per_minute
    }
    pub fn tracked_peers(self) -> usize {
        self.tracked_peers
    }
    pub fn handler_timeout(self) -> Duration {
        self.handler_timeout
    }
    pub fn header_timeout(self) -> Duration {
        self.header_timeout
    }
    pub fn connection_timeout(self) -> Duration {
        self.connection_timeout
    }
    pub fn response_bytes(self) -> usize {
        self.response_bytes
    }
    pub fn response_buffer_bytes(self) -> usize {
        self.response_buffer_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_preserve_existing_budgets() {
        let limits = PublicRequestLimits::default();
        assert_eq!(limits.active_requests(), 32);
        assert_eq!(limits.connections(), 128);
        assert_eq!(limits.hash_operations(), 4);
        assert_eq!(limits.uploads(), 4);
        assert_eq!(limits.writes_per_minute(), 30);
        assert_eq!(limits.tracked_peers(), 10_000);
        assert_eq!(limits.handler_timeout(), Duration::from_secs(10));
        assert_eq!(limits.header_timeout(), Duration::from_secs(10));
        assert_eq!(limits.connection_timeout(), Duration::from_secs(120));
        assert_eq!(limits.response_bytes(), 33_554_432);
        assert_eq!(limits.response_buffer_bytes(), 134_217_728);
    }

    #[test]
    fn every_budget_rejects_malformed_and_out_of_range_values_without_echoing_them() {
        for (name, (minimum, maximum)) in PublicRequestLimits::NAMES.into_iter().zip([
            (1, 1024),
            (1, 4096),
            (1, 32),
            (1, 32),
            (1, 10_000),
            (1, 100_000),
            (1, 120_000),
            (1, 120_000),
            (1, 600_000),
            (1, 268_435_456),
            (4096, 1_073_741_824),
        ]) {
            for value in [
                "",
                "0",
                "-1",
                "+1",
                " 1",
                "1 ",
                "1.0",
                "one",
                "secret-marker",
                "18446744073709551616",
                "999999999999999999999999",
            ] {
                let error =
                    PublicRequestLimits::from_lookup(|key| (key == name).then(|| value.into()))
                        .unwrap_err();
                assert!(error.0.contains(name));
                assert!(!error.0.contains("secret-marker"));
            }
            assert!(
                PublicRequestLimits::from_lookup(
                    |key| (key == name).then(|| (maximum + 1).to_string())
                )
                .is_err()
            );
            if minimum > 1 {
                assert!(
                    PublicRequestLimits::from_lookup(
                        |key| (key == name).then(|| (minimum - 1).to_string())
                    )
                    .is_err()
                );
            }
            for value in [minimum, maximum] {
                assert!(PublicRequestLimits::from_lookup(
                    |key| (key == name).then(|| value.to_string())
                )
                .is_ok());
            }
        }
    }

    #[test]
    fn response_ceiling_and_aggregate_pool_are_independent() {
        let limits = PublicRequestLimits::from_lookup(|name| match name {
            "PUBLIC_MAX_RESPONSE_BYTES" => Some("268435456".into()),
            "PUBLIC_MAX_RESPONSE_BUFFER_BYTES" => Some("4096".into()),
            _ => None,
        })
        .unwrap();
        assert_eq!(limits.response_bytes(), 268_435_456);
        assert_eq!(limits.response_buffer_bytes(), 4096);
    }
}
