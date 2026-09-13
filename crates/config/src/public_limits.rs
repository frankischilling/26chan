use crate::ConfigError;
use std::{collections::BTreeMap, time::Duration};

/// Per-process budgets. Private fields keep unchecked values out of runtimes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PublicRequestLimits {
    active_requests: usize,
    hash_operations: usize,
    uploads: usize,
    writes_per_minute: u32,
    tracked_peers: usize,
    handler_timeout: Duration,
}

impl Default for PublicRequestLimits {
    fn default() -> Self {
        Self::from_lookup(|_| None).expect("fixed public request defaults")
    }
}

impl PublicRequestLimits {
    pub const NAMES: [&'static str; 6] = [
        "PUBLIC_MAX_ACTIVE_REQUESTS",
        "PUBLIC_MAX_HASH_OPERATIONS",
        "PUBLIC_MAX_UPLOADS",
        "PUBLIC_WRITES_PER_MINUTE",
        "PUBLIC_MAX_TRACKED_PEERS",
        "PUBLIC_HANDLER_TIMEOUT_MS",
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
        let mut read = |name, default, maximum, error| {
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
            if value == 0 || value > maximum {
                return Err(ConfigError(error));
            }
            Ok(value)
        };
        Ok(Self {
            active_requests: read(
                Self::NAMES[0],
                32,
                1024,
                "PUBLIC_MAX_ACTIVE_REQUESTS must be a decimal integer from 1 through 1024.",
            )? as usize,
            hash_operations: read(
                Self::NAMES[1],
                4,
                32,
                "PUBLIC_MAX_HASH_OPERATIONS must be a decimal integer from 1 through 32.",
            )? as usize,
            uploads: read(
                Self::NAMES[2],
                4,
                32,
                "PUBLIC_MAX_UPLOADS must be a decimal integer from 1 through 32.",
            )? as usize,
            writes_per_minute: read(
                Self::NAMES[3],
                30,
                10_000,
                "PUBLIC_WRITES_PER_MINUTE must be a decimal integer from 1 through 10000.",
            )? as u32,
            tracked_peers: read(
                Self::NAMES[4],
                10_000,
                100_000,
                "PUBLIC_MAX_TRACKED_PEERS must be a decimal integer from 1 through 100000.",
            )? as usize,
            handler_timeout: Duration::from_millis(read(
                Self::NAMES[5],
                10_000,
                120_000,
                "PUBLIC_HANDLER_TIMEOUT_MS must be a decimal integer from 1 through 120000.",
            )?),
        })
    }

    pub fn active_requests(self) -> usize {
        self.active_requests
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_preserve_existing_budgets() {
        let limits = PublicRequestLimits::default();
        assert_eq!(limits.active_requests(), 32);
        assert_eq!(limits.hash_operations(), 4);
        assert_eq!(limits.uploads(), 4);
        assert_eq!(limits.writes_per_minute(), 30);
        assert_eq!(limits.tracked_peers(), 10_000);
        assert_eq!(limits.handler_timeout(), Duration::from_secs(10));
    }

    #[test]
    fn every_budget_rejects_malformed_and_out_of_range_values_without_echoing_them() {
        for (name, maximum) in PublicRequestLimits::NAMES
            .into_iter()
            .zip([1024, 32, 32, 10_000, 100_000, 120_000])
        {
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
            for value in [1, maximum] {
                assert!(PublicRequestLimits::from_lookup(|key| (key == name).then(|| value.to_string())).is_ok());
            }
        }
    }
}
