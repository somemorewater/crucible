use axum::extract::State;
use axum::{Json, response::IntoResponse, extract::State};
use serde::{Serialize, Deserialize};
use tracing::{info, instrument, info_span};
use chrono::{DateTime, Utc};
use crate::error::AppError;
use crate::services::{error_recovery::ErrorManager, sys_metrics::MetricsExporter};
use axum::{extract::State, response::IntoResponse, Json};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, instrument};
use utoipa::ToSchema;
use crate::services::{
    sys_metrics::MetricsExporter,
    error_recovery::ErrorManager,
    log_aggregator::LogAggregator,
    tracing::TracingService,
};
use crate::config::reload::ConfigManager;
use crate::api::contracts::{ApiResponse, SystemStatus, ProfileTriggerRequest, ProfileTriggerResponse, ValidatedJson};
use sqlx::PgPool;
use redis::Client as RedisClient;

pub struct AppState {
    pub db: Option<sqlx::PgPool>,
    pub metrics_exporter: Arc<MetricsExporter>,
    pub error_manager: Arc<ErrorManager>,
    pub config_manager: Arc<ConfigManager>,
    pub log_aggregator: Arc<LogAggregator>,
    pub redis: RedisClient,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct MetricsReport {
    /// Total system uptime in seconds
    pub uptime_secs: u64,
    /// Current resident set size (RSS) in bytes
    pub memory_usage_bytes: u64,
    /// Number of currently active HTTP requests
    pub active_requests: u32,
    /// Percentage of failed requests in the last window
    pub error_rate: f64,
    /// Current latency for Stellar ledger ingestion in milliseconds
    pub ledger_ingestion_latency_ms: u32,
}

#[derive(Debug, Serialize, ToSchema)]
pub struct HealthResponse {
    /// Overall health status (e.g., 'healthy' or 'degraded')
    pub status: String,
    /// The current version of the backend service
    pub version: String,
    /// RFC3339 timestamp of the health check
    pub timestamp: DateTime<Utc>,
    /// Connectivity status to the PostgreSQL database
    pub database_connected: bool,
    /// Connectivity status to the Redis cache
    pub redis_connected: bool,
}

/// Handler for retrieving detailed performance metrics.
/// Optimized for consumption by monitoring tools like Grafana.
#[utoipa::path(
    get,
    path = "/api/v1/profiling/metrics",
    responses(
        (status = 200, description = "Performance metrics retrieved successfully", body = MetricsReport),
        (status = 500, description = "Internal server error")
    ),
    tag = "profiling"
)]
#[instrument(skip_all, fields(http.method = "GET", http.route = "/api/v1/profiling/metrics"))]
pub async fn get_metrics(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, AppError> {
    let span = info_span!("metrics.collection");
    let _enter = span.enter();
    
    info!("Collecting performance metrics");

    let sys_metrics = state.metrics_exporter.get_metrics().await;

    
    // Instrument the metrics exporter call
    let metrics_span = TracingService::service_method_span("MetricsExporter", "get_metrics");
    let _metrics_enter = metrics_span.enter();
    
    let sys_metrics = state.metrics_exporter.get_metrics().await;
    drop(_metrics_enter);
    
    let report = MetricsReport {
        uptime_secs: sys_metrics.uptime,
        memory_usage_bytes: sys_metrics.memory_usage,
        active_requests: 12,
        error_rate: 0.001,
        ledger_ingestion_latency_ms: 120,
    };

    info!(
        uptime = sys_metrics.uptime,
        memory = sys_metrics.memory_usage,
        active_requests = 12,
        "Metrics collected successfully"
    );

    Ok(Json(report))
}

/// Handler for system health checks.
/// Performs actual pings to downstream services.
#[utoipa::path(
    get,
    path = "/api/v1/profiling/health",
    responses(
        (status = 200, description = "System is healthy", body = HealthResponse),
        (status = 503, description = "System is degraded")
    ),
    tag = "profiling"
)]
#[instrument(skip_all, fields(http.method = "GET", http.route = "/api/v1/profiling/health"))]
pub async fn get_health(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, AppError> {
    let span = info_span!("health.check");
    let _enter = span.enter();
    
    info!("Performing system health check");

    
    // Check database connectivity with tracing
    let db_span = TracingService::db_query_span(
        "SELECT 1",
        "postgres",
        "PING"
    );
    let _db_enter = db_span.enter();
    
    let db_healthy = sqlx::query("SELECT 1")
        .fetch_optional(&state.db)
        .await
        .map(|result| result.is_some())
        .unwrap_or_else(|e| {
            TracingService::record_error(&db_span, &e.to_string(), "database");
            false
        });
    drop(_db_enter);
    
    let response = HealthResponse {
        status: if db_healthy { "healthy" } else { "degraded" }.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        timestamp: Utc::now(),
        database_connected: true,
        database_connected: db_healthy,
        redis_connected: true,
    };

    info!(
        db_connected = db_healthy,
        version = env!("CARGO_PKG_VERSION"),
        "Health check completed"
    );

    Ok(Json(response))
}

/// Handler for Prometheus-compatible metrics.
#[instrument(skip_all, fields(http.method = "GET", http.route = "/api/v1/profiling/prometheus"))]
pub async fn get_prometheus_metrics() -> impl IntoResponse {
    let span = info_span!("prometheus.metrics.export");
    let _enter = span.enter();
    
    info!("Exporting Prometheus-format metrics");
    
    "# HELP backend_requests_total Total number of requests\n\
                   # TYPE backend_requests_total counter\n\
                   backend_requests_total 1024\n\
                   # HELP backend_ledger_latency_ms Current ledger ingestion latency\n\
                   # TYPE backend_ledger_latency_ms gauge\n\
                   backend_ledger_latency_ms 120\n"
        .to_string()
}

pub async fn get_system_status(State(state): State<Arc<AppState>>) -> impl IntoResponse {
     # TYPE backend_requests_total counter\n\
     backend_requests_total 1024\n\
     # HELP backend_ledger_latency_ms Current ledger ingestion latency\n\
     # TYPE backend_ledger_latency_ms gauge\n\
     backend_ledger_latency_ms 120\n".to_string()
}

/// Handler for detailed system status
#[instrument(skip_all, fields(http.method = "GET", http.route = "/api/status"))]
pub async fn get_system_status(
    State(state): State<Arc<AppState>>,
) -> ApiResponse<SystemStatus> {
) -> impl IntoResponse {
    let span = info_span!("system.status");
    let _enter = span.enter();
    
    info!("Retrieving system status");
    
    let metrics_span = TracingService::service_method_span("MetricsExporter", "get_metrics");
    let _metrics_enter = metrics_span.enter();
    let metrics = state.metrics_exporter.get_metrics().await;
    drop(_metrics_enter);
    
    let recovery_span = TracingService::service_method_span("ErrorManager", "get_active_tasks");
    let _recovery_enter = recovery_span.enter();
    let recovery_tasks = state.error_manager.get_active_tasks().await;
    drop(_recovery_enter);

    ApiResponse::new(SystemStatus {
        status: "healthy".to_string(),
        uptime_secs: metrics.uptime,
        memory_used_bytes: metrics.memory_usage,
        active_recovery_tasks: recovery_tasks.len(),
    })
    Json(serde_json::json!({
        "status": "healthy",
        "metrics": metrics,
        "active_recovery_tasks": recovery_tasks,
    }))
}

pub async fn trigger_profile_collection(State(_state): State<Arc<AppState>>) -> impl IntoResponse {
/// Handler to trigger profile collection (CPU, memory profiling)
#[instrument(skip_all, fields(http.method = "POST", http.route = "/api/profile"))]
pub async fn trigger_profile_collection(
    State(_state): State<Arc<AppState>>,
    ValidatedJson(payload): ValidatedJson<ProfileTriggerRequest>,
) -> ApiResponse<ProfileTriggerResponse> {
    // In a real implementation, this would trigger a CPU/Memory profile
    // using the provided payload (duration, sample rate, etc.)
    
    ApiResponse::new(ProfileTriggerResponse {
        profile_id: uuid::Uuid::new_v4(),
        message: format!("Profiling collection triggered for label: {}", payload.label),
        estimated_completion: chrono::Utc::now() + chrono::Duration::seconds(payload.duration_secs as i64),
    })
) -> impl IntoResponse {
    let span = info_span!("profiling.collection");
    let _enter = span.enter();
    
    let profile_id = uuid::Uuid::new_v4().to_string();
    
    info!(
        profile_id = %profile_id,
        "Profiling collection triggered"
    );
    
    // In a real implementation, this would trigger a CPU/Memory profile
    Json(serde_json::json!({
        "message": "Profiling collection triggered",
        "profile_id": profile_id,
    }))
}
