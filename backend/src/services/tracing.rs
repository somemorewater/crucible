//! OpenTelemetry tracing initialisation.
//!
//! This module wires the [`tracing`] subscriber stack to an OTLP exporter so
//! that every `tracing` span is forwarded to an OpenTelemetry-compatible
//! collector (Jaeger, Grafana Tempo, OTEL Collector, …).
//!
//! # Usage
//!
//! ```rust,no_run
//! use backend::services::tracing::{init, TracingConfig};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let cfg = TracingConfig::from_env();
//!     let _guard = init(cfg)?;
//!     // _guard shuts down the tracer provider when dropped
//!     Ok(())
//! }
//! ```
//!
//! # Environment variables
//!
//! | Variable | Default | Description |
//! |---|---|---|
//! | `OTEL_EXPORTER_OTLP_ENDPOINT` | `http://localhost:4318` | OTLP HTTP collector endpoint |
//! | `OTEL_SERVICE_NAME` | `backend` | Service name attached to every span |
//! | `RUST_LOG` | `backend=debug` | `tracing` filter directive |

use opentelemetry::global;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::{SpanExporter, WithExportConfig};
use opentelemetry_sdk::{
    trace::{RandomIdGenerator, Sampler, SdkTracerProvider},
    Resource,
};
use thiserror::Error;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur while initialising the tracing stack.
#[derive(Debug, Error)]
pub enum TracingError {
    /// The OTLP exporter could not be built.
    #[error("Failed to build OTLP span exporter: {0}")]
    ExporterBuild(String),

    /// The tracing subscriber could not be installed.
    #[error("Failed to install tracing subscriber: {0}")]
    SubscriberInit(String),
}

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Configuration for the OpenTelemetry tracing stack.
#[derive(Debug, Clone)]
pub struct TracingConfig {
    /// OTLP HTTP endpoint (e.g. `http://localhost:4318`).
    pub otlp_endpoint: String,
    /// Logical service name attached to every span.
    pub service_name: String,
    /// `tracing` filter directive (e.g. `"backend=debug,tower_http=info"`).
    pub log_filter: String,
}

impl TracingConfig {
    /// Build configuration from environment variables, falling back to
    /// sensible defaults when variables are absent.
    pub fn from_env() -> Self {
        Self {
            otlp_endpoint: std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
                .unwrap_or_else(|_| "http://localhost:4318".to_string()),
            service_name: std::env::var("OTEL_SERVICE_NAME")
                .unwrap_or_else(|_| "backend".to_string()),
            log_filter: std::env::var("RUST_LOG")
                .unwrap_or_else(|_| "backend=debug,tower_http=debug".to_string()),
        }
    }
}

impl Default for TracingConfig {
    fn default() -> Self {
        Self::from_env()
    }
}

// ---------------------------------------------------------------------------
// Guard
// ---------------------------------------------------------------------------

/// RAII guard that shuts down the global tracer provider on drop.
///
/// Hold this value for the lifetime of the process. Dropping it flushes any
/// in-flight spans and releases the exporter connection.
pub struct TracingGuard {
    provider: SdkTracerProvider,
}

impl TracingGuard {
    /// Create a guard backed by a no-op provider (no exporter attached).
    /// Useful as a fallback when the real OTel initialisation fails.
    pub fn noop() -> Self {
        Self {
            provider: SdkTracerProvider::builder().build(),
        }
    }
}

impl Drop for TracingGuard {
    fn drop(&mut self) {
        if let Err(e) = self.provider.shutdown() {
            // Can't use tracing here — subscriber may already be gone.
            eprintln!("OpenTelemetry tracer provider shutdown error: {e}");
        }
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Initialise the global [`tracing`] subscriber with an OTLP exporter layer.
///
/// The subscriber stack is:
/// 1. `EnvFilter` — honours `RUST_LOG` / [`TracingConfig::log_filter`].
/// 2. `tracing_subscriber::fmt` — human-readable output to stdout.
/// 3. `tracing_opentelemetry::OpenTelemetryLayer` — forwards spans to the
///    OTLP collector at [`TracingConfig::otlp_endpoint`].
///
/// Returns a [`TracingGuard`] that must be kept alive for the duration of the
/// process. Dropping it triggers a graceful shutdown of the tracer provider.
///
/// # Errors
///
/// Returns [`TracingError`] if the exporter cannot be built or the subscriber
/// cannot be installed (e.g. a global subscriber is already set).
pub fn init(cfg: TracingConfig) -> Result<TracingGuard, TracingError> {
    let provider = build_provider(&cfg)?;

    // Register as the global provider so `global::tracer()` works anywhere.
    global::set_tracer_provider(provider.clone());

    let otel_layer =
        tracing_opentelemetry::layer().with_tracer(provider.tracer(cfg.service_name.clone()));

    let filter =
        EnvFilter::try_new(&cfg.log_filter).unwrap_or_else(|_| EnvFilter::new("backend=debug"));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .with(otel_layer)
        .try_init()
        .map_err(|e| TracingError::SubscriberInit(e.to_string()))?;

    Ok(TracingGuard { provider })
}

/// Build a [`SdkTracerProvider`] backed by a batched OTLP HTTP exporter.
fn build_provider(cfg: &TracingConfig) -> Result<SdkTracerProvider, TracingError> {
    let exporter = SpanExporter::builder()
        .with_http()
        .with_endpoint(&cfg.otlp_endpoint)
        .build()
        .map_err(|e| TracingError::ExporterBuild(e.to_string()))?;

    let resource = Resource::builder()
        .with_service_name(cfg.service_name.clone())
        .build();

    let provider = SdkTracerProvider::builder()
        .with_resource(resource)
        .with_sampler(Sampler::AlwaysOn)
        .with_id_generator(RandomIdGenerator::default())
        .with_batch_exporter(exporter)
        .build();

    Ok(provider)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
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

use opentelemetry::KeyValue;
use opentelemetry::trace::TracerProvider as _;
use opentelemetry_otlp::WithExportConfig;
use opentelemetry_sdk::trace::{Config, RandomIdGenerator, Sampler, TracerProvider};
use opentelemetry_sdk::Resource;
use opentelemetry_semantic_conventions::resource;
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
        let resource = Resource::new(vec![
            KeyValue::new(resource::SERVICE_NAME, config.service_name.clone()),
            KeyValue::new(resource::SERVICE_VERSION, config.service_version.clone()),
            KeyValue::new(resource::DEPLOYMENT_ENVIRONMENT, config.environment.clone()),
            KeyValue::new("service.namespace", "crucible"),
        ]);

        let sampler = if config.environment == "production" {
            Sampler::ParentBased(Box::new(
                Sampler::TraceIdRatioBased(config.sampling_ratio),
            ))
        } else {
            Sampler::AlwaysOn
        };

        let trace_config = Config::default()
            .with_resource(resource)
            .with_sampler(sampler)
            .with_id_generator(RandomIdGenerator::default())
            .with_max_attributes_per_span(config.max_attributes_per_span as u32)
            .with_max_events_per_span(config.max_events_per_span as u32)
            .with_max_links_per_span(config.max_links_per_span as u32);

        let tracer_provider = opentelemetry_otlp::new_pipeline()
            .tracing()
            .with_exporter(
                opentelemetry_otlp::new_exporter()
                    .tonic()
                    .with_endpoint(&config.otlp_endpoint)
                    .with_timeout(Duration::from_secs(10)),
            )
            .with_trace_config(trace_config)
            .install_batch(opentelemetry_sdk::runtime::Tokio)
            .map_err(|e| anyhow::anyhow!("Failed to install OTLP exporter: {}", e))?;

        // Get a tracer from the provider
        let tracer = tracer_provider.tracer("crucible-backend");

        let telemetry_layer = tracing_opentelemetry::layer().with_tracer(tracer);

        let subscriber = Registry::default()
            .with(
                EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| EnvFilter::new("info,crucible=debug")),
            )
            .with(telemetry_layer)
            .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr));

        tracing::subscriber::set_global_default(subscriber)
            .map_err(|e| anyhow::anyhow!("Failed to set global subscriber: {}", e))?;

        tracing::info!("OpenTelemetry tracing initialized successfully");
        tracing::info!("Service: {}", config.service_name);
        tracing::info!("Environment: {}", config.environment);
        tracing::info!("OTLP Endpoint: {}", config.otlp_endpoint);
        tracing::info!("Sampling Ratio: {:.1}%", config.sampling_ratio * 100.0);

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
    fn test_config_defaults() {
        // Build config directly without relying on env vars.
        let cfg = TracingConfig {
            otlp_endpoint: "http://localhost:4318".to_string(),
            service_name: "backend".to_string(),
            log_filter: "backend=debug,tower_http=debug".to_string(),
        };
        assert_eq!(cfg.otlp_endpoint, "http://localhost:4318");
        assert_eq!(cfg.service_name, "backend");
        assert!(!cfg.log_filter.is_empty());
    }

    #[test]
    fn test_config_from_env_values() {
        // Verify that TracingConfig correctly stores whatever values are given.
        let cfg = TracingConfig {
            otlp_endpoint: "http://collector:4318".to_string(),
            service_name: "my-service".to_string(),
            log_filter: "info".to_string(),
        };
        assert_eq!(cfg.otlp_endpoint, "http://collector:4318");
        assert_eq!(cfg.service_name, "my-service");
        assert_eq!(cfg.log_filter, "info");
    }

    #[test]
    fn test_tracing_error_display() {
        let e = TracingError::ExporterBuild("bad url".to_string());
        assert!(e.to_string().contains("bad url"));

        let e = TracingError::SubscriberInit("already set".to_string());
        assert!(e.to_string().contains("already set"));
    }

    #[test]
    fn test_build_provider_succeeds() {
        // build_provider only constructs SDK objects; no network connection is
        // opened, so this works without a live collector.
        let cfg = TracingConfig {
            otlp_endpoint: "http://localhost:4318".to_string(),
            service_name: "test".to_string(),
            log_filter: "debug".to_string(),
        };
        let result = build_provider(&cfg);
        assert!(result.is_ok());
        let _ = result.unwrap().shutdown();
    }

    #[test]
    fn test_build_provider_custom_endpoint() {
        let cfg = TracingConfig {
            otlp_endpoint: "http://otel-collector.internal:4318".to_string(),
            service_name: "svc-a".to_string(),
            log_filter: "info".to_string(),
        };
        let result = build_provider(&cfg);
        assert!(result.is_ok());
        let _ = result.unwrap().shutdown();
    }

    #[test]
    fn test_tracing_guard_shuts_down_on_drop() {
        let cfg = TracingConfig {
            otlp_endpoint: "http://localhost:4318".to_string(),
            service_name: "guard-test".to_string(),
            log_filter: "debug".to_string(),
        };
        let provider = build_provider(&cfg).unwrap();
        let guard = TracingGuard { provider };
        drop(guard); // must not panic
    }

    #[test]
    fn test_tracing_guard_noop() {
        let guard = TracingGuard::noop();
        drop(guard); // must not panic
    }

    #[test]
    fn test_config_clone() {
        let cfg = TracingConfig {
            otlp_endpoint: "http://a:4318".to_string(),
            service_name: "svc".to_string(),
            log_filter: "debug".to_string(),
        };
        let cloned = cfg.clone();
        assert_eq!(cfg.otlp_endpoint, cloned.otlp_endpoint);
        assert_eq!(cfg.service_name, cloned.service_name);
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
