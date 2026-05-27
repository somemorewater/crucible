//! CONFIG APPROACH: Option A — layered config crate
//! Rationale: Using the `config` crate provides a robust, layered approach where environment-specific
//! defaults are cleanly defined in TOML files, while sensitive secrets and infrastructure-specific
//! overrides are passed securely via environment variables.

use serde::{Deserialize, Serialize};
use std::str::FromStr;
use tracing::Level;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

/// Observability configuration (logs, tracing, metrics).
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ObservabilityConfig {
    /// Logging level (e.g., "trace", "debug", "info", "warn", "error")
    pub log_level: String,
    /// OTLP endpoint for exporting traces. Only used if provided.
    pub tracing_endpoint: Option<String>,
    /// Whether to enable metrics collection.
    pub enable_metrics: bool,
}

impl ObservabilityConfig {
    /// Parses the configured log level into a `tracing::Level`.
    pub fn parsed_log_level(&self) -> Level {
        Level::from_str(&self.log_level).unwrap_or(Level::INFO)
    }

    /// Initializes the tracing subscriber globally based on the environment.
    /// Development uses a pretty formatter, while Staging/Production use JSON.
    pub fn init_tracing(&self, env: crate::config::Environment) {
        let filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new(&self.log_level));

        match env {
            crate::config::Environment::Development => {
                let fmt_layer = tracing_subscriber::fmt::layer()
                    .pretty()
                    .with_thread_ids(true)
                    .with_target(true);

                // try_init silently returns an error if a subscriber is already set, preventing panics in tests
                let _ = tracing_subscriber::registry()
                    .with(filter)
                    .with(fmt_layer)
                    .try_init();
            }
            crate::config::Environment::Staging | crate::config::Environment::Production => {
                let fmt_layer = tracing_subscriber::fmt::layer()
                    .json()
                    .with_span_list(true)
                    .with_current_span(true);

                let _ = tracing_subscriber::registry()
                    .with(filter)
                    .with(fmt_layer)
                    .try_init();
            }
        }
    }
}
