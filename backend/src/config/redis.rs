//! CONFIG APPROACH: Option A — layered config crate
//! Rationale: Using the `config` crate provides a robust, layered approach where environment-specific
//! defaults are cleanly defined in TOML files, while sensitive secrets and infrastructure-specific
//! overrides are passed securely via environment variables.

use redis::IntoConnectionInfo;
use serde::{Deserialize, Serialize};
use std::fmt;

/// Redis configuration for caching and optional job queues.
#[derive(Clone, Deserialize, Serialize)]
pub struct RedisConfig {
    /// The primary Redis connection URL (used for cache). Never logged or serialized.
    #[serde(skip_serializing)]
    pub url: String,
    /// Optional separate Redis URL for a job queue. Falls back to `url` if not provided.
    #[serde(skip_serializing)]
    pub job_queue_url: Option<String>,
    /// Number of connections in the Redis pool.
    pub pool_size: u32,
    /// Connection timeout in milliseconds.
    pub connection_timeout_ms: u64,
    /// Maximum number of connection retries.
    pub max_retries: u32,
}

impl fmt::Debug for RedisConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RedisConfig")
            .field("url", &"[REDACTED]")
            .field(
                "job_queue_url",
                if self.job_queue_url.is_some() {
                    &Some("[REDACTED]")
                } else {
                    &None::<&str>
                },
            )
            .field("pool_size", &self.pool_size)
            .field("connection_timeout_ms", &self.connection_timeout_ms)
            .field("max_retries", &self.max_retries)
            .finish()
    }
}

impl RedisConfig {
    /// Converts the primary config to Redis connection info.
    pub fn to_connection_info(
        &self,
    ) -> Result<redis::ConnectionInfo, crate::config::error::ConfigError> {
        self.url.as_str().into_connection_info().map_err(|e| {
            crate::config::error::ConfigError::InvalidValue {
                field: "redis.url".to_string(),
                reason: e.to_string(),
            }
        })
    }

    /// Converts the job queue config to Redis connection info.
    pub fn to_job_queue_connection_info(
        &self,
    ) -> Result<redis::ConnectionInfo, crate::config::error::ConfigError> {
        let url = self.job_queue_url.as_ref().unwrap_or(&self.url);
        url.as_str().into_connection_info().map_err(|e| {
            crate::config::error::ConfigError::InvalidValue {
                field: "redis.job_queue_url".to_string(),
                reason: e.to_string(),
            }
        })
    }
}
