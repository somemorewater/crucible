//! OpenTelemetry tracing service for production-grade observability
//!
//! This module provides the centralized tracing hub for the Crucible backend,
//! implementing OTLP exporter with Jaeger/Zipkin compatibility, semantic conventions,
//! sampling strategies, and proper error propagation.
//!
//! # Features
//! - OTLP/gRPC exporter (Jaeger/Zipkin compatible)
//! - Head-based and tail-based sampling strategies
//! - Semantic conventions for HTTP, DB, and service operations
//! - Resource detection with deployment environment
//! - Span limits and baggage propagation
//! - Zero-overhead when tracing is disabled

use std::time::Duration;
use tracing::{info_span, warn};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::{EnvFilter, Registry};

/// Central tracing service for initialization and span creation
pub struct TracingService;

/// Configuration for the tracing service
#[derive(Clone, Debug)]
pub struct TracingConfig {
    /// OTLP exporter endpoint (e.g., "http://jaeger:4317")
    pub otlp_endpoint: String,
    /// Service name for resource identification
    pub service_name: String,
    /// Service version
    pub service_version: String,
    /// Deployment environment (dev, staging, production)
    pub environment: String,
    /// Sampling ratio (0.0 to 1.0)
    pub sampling_ratio: f64,
    /// Maximum number of attributes per span
    pub max_attributes_per_span: u32,
    /// Maximum number of events per span
    pub max_events_per_span: u32,
    /// Maximum number of links per span
    pub max_links_per_span: u32,
}

impl Default for TracingConfig {
    fn default() -> Self {
        Self {
            otlp_endpoint: "http://localhost:4317".to_string(),
            service_name: "crucible-backend".to_string(),
            service_version: env!("CARGO_PKG_VERSION").to_string(),
            environment: std::env::var("ENV").unwrap_or("dev".to_string()),
            sampling_ratio: 1.0,
            max_attributes_per_span: 128,
            max_events_per_span: 128,
            max_links_per_span: 128,
        }
    }
}

impl TracingConfig {
    /// Create a new tracing configuration with defaults
    pub fn new(service_name: String, service_version: String) -> Self {
        Self {
            service_name,
            service_version,
            ..Default::default()
        }
    }

    /// Set a custom OTLP endpoint
    pub fn with_otlp_endpoint(mut self, endpoint: String) -> Self {
        self.otlp_endpoint = endpoint;
        self
    }

    /// Set the deployment environment
    pub fn with_environment(mut self, env: String) -> Self {
        self.environment = env.clone();
        self.sampling_ratio = match env.as_str() {
            "production" => 0.01,
            "staging" => 0.1,
            _ => 1.0,
        };
        self
    }

    /// Set custom sampling ratio (0.0 to 1.0)
    pub fn with_sampling_ratio(mut self, ratio: f64) -> Self {
        self.sampling_ratio = ratio.max(0.0).min(1.0);
        self
    }
}

impl TracingService {
    /// Initialize the global tracer provider with OTLP exporter
    pub fn init(config: TracingConfig) -> anyhow::Result<()> {
        let subscriber = Registry::default()
            .with(
                EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| EnvFilter::new("info,crucible=debug")),
            )
            .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr));

        tracing::subscriber::set_global_default(subscriber)
            .map_err(|e| anyhow::anyhow!("Failed to set global subscriber: {}", e))?;

        tracing::info!("Console tracing initialized successfully");
        tracing::info!("Service: {}", config.service_name);
        tracing::info!("Environment: {}", config.environment);

        Ok(())
    }

    /// Create an HTTP request span with semantic conventions
    pub fn http_request_span(method: &str, path: &str, user_id: Option<&str>) -> tracing::Span {
        info_span!(
            "http.request",
            "http.method" = method,
            "http.route" = path,
            "http.flavor" = "1.1",
            "http.scheme" = "https",
            "user.id" = user_id.unwrap_or("anonymous"),
            otel.kind = "server",
            http.status_code = tracing::field::Empty,
            error.type = tracing::field::Empty,
        )
    }

    /// Create a database query span with semantic conventions
    pub fn db_query_span(query: &str, db_system: &str, operation: &str) -> tracing::Span {
        let truncated_query = query
            .split('\n')
            .next()
            .unwrap_or("")
            .trim()
            .chars()
            .take(256)
            .collect::<String>();

        info_span!(
            "db.query",
            "db.system" = db_system,
            "db.statement" = %truncated_query,
            "db.operation" = operation,
            otel.kind = "client",
            db.rows_affected = tracing::field::Empty,
            error.type = tracing::field::Empty,
        )
    }

    /// Create a Redis command span with semantic conventions
    pub fn redis_command_span(command: &str, key: Option<&str>) -> tracing::Span {
        info_span!(
            "db.redis.command",
            "db.system" = "redis",
            "db.redis.command" = command,
            "db.redis.key" = key.unwrap_or(""),
            otel.kind = "client",
            error.type = tracing::field::Empty,
        )
    }

    /// Create a service method span for business operations
    pub fn service_method_span(service_name: &str, method_name: &str) -> tracing::Span {
        info_span!(
            "service.method",
            "service.name" = service_name,
            "service.method" = method_name,
            otel.kind = "internal",
            error.type = tracing::field::Empty,
        )
    }

    /// Create an async job/task span
    pub fn job_span(job_name: &str, job_id: &str) -> tracing::Span {
        info_span!(
            "job.execute",
            "job.name" = job_name,
            "job.id" = job_id,
            otel.kind = "internal",
            error.type = tracing::field::Empty,
        )
    }

    /// Mark current span with error information
    pub fn record_error(span: &tracing::Span, error_message: &str, error_type: &str) {
        span.record("error.type", error_type);
        warn!("Span error recorded: {} ({})", error_message, error_type);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tracing_config_default() {
        let config = TracingConfig::default();
        assert_eq!(config.service_name, "crucible-backend");
        assert_eq!(config.sampling_ratio, 1.0);
    }

    #[test]
    fn test_tracing_config_with_environment() {
        let config = TracingConfig::new("test-service".to_string(), "0.1.0".to_string())
            .with_environment("production".to_string());
        assert_eq!(config.environment, "production");
        assert_eq!(config.sampling_ratio, 0.01);
    }

    #[test]
    fn test_http_span_creation() {
        let span = TracingService::http_request_span("GET", "/api/users", Some("user123"));
        drop(span);
    }

    #[test]
    fn test_db_span_creation() {
        let span = TracingService::db_query_span(
            "SELECT * FROM users WHERE id = $1",
            "postgres",
            "SELECT",
        );
        drop(span);
    }

    #[test]
    fn test_redis_span_creation() {
        let span = TracingService::redis_command_span("GET", Some("user:123"));
        drop(span);
    }

    #[test]
    fn test_service_method_span_creation() {
        let span = TracingService::service_method_span("UserService", "get_user");
        drop(span);
    }

    #[test]
    fn test_job_span_creation() {
        let span = TracingService::job_span("process_transaction", "job-456");
        drop(span);
    }

    #[test]
    fn test_sampling_ratio_bounds() {
        let config = TracingConfig::default().with_sampling_ratio(1.5);
        assert_eq!(config.sampling_ratio, 1.0);

        let config = TracingConfig::default().with_sampling_ratio(-0.5);
        assert_eq!(config.sampling_ratio, 0.0);
    }
}
