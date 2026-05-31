//! CONFIG APPROACH: Option A — layered config crate
//! Rationale: Using the `config` crate provides a robust, layered approach where environment-specific
//! defaults are cleanly defined in TOML files, while sensitive secrets and infrastructure-specific
//! overrides are passed securely via environment variables.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Server configuration governing HTTP connections.
#[derive(Clone, Deserialize, Serialize)]
pub struct ServerConfig {
    /// The host address to bind to (e.g., "127.0.0.1" or "0.0.0.0")
    pub host: String,
    /// The port to listen on
    pub port: u16,
    /// Maximum time in milliseconds to wait for a request to complete
    pub request_timeout_ms: u64,
    /// Maximum number of concurrent connections
    pub max_connections: usize,
    /// TLS configuration (required in production, optional elsewhere)
    pub tls: Option<TlsConfig>,
}

impl fmt::Debug for ServerConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerConfig")
            .field("host", &self.host)
            .field("port", &self.port)
            .field("request_timeout_ms", &self.request_timeout_ms)
            .field("max_connections", &self.max_connections)
            .field("tls", &self.tls)
            .finish()
    }
}

/// TLS certificates configuration.
#[derive(Clone, Deserialize, Serialize)]
pub struct TlsConfig {
    /// Path to the TLS certificate chain
    pub cert_path: String,
    /// Path to the TLS private key. Marked as skip_serializing to avoid accidental leaks.
    #[serde(skip_serializing)]
    pub key_path: String,
}

impl fmt::Debug for TlsConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TlsConfig")
            .field("cert_path", &self.cert_path)
            .field("key_path", &"[REDACTED]")
            .finish()
    }
}
