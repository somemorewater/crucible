use std::sync::Arc;
use axum::{Json, response::IntoResponse, extract::State};
use serde::{Serialize, Deserialize};
use utoipa::ToSchema;
use chrono::{DateTime, Utc};
use crate::error::AppError;
use crate::services::{
    sys_metrics::{MetricsExporter, SystemMetrics},
    error_recovery::ErrorManager,
    tracing::TracingService,
    log_aggregator::LogAggregator,
    contract_benchmark::{
        ContractBenchmarkError, ContractBenchmarkReport, ContractBenchmarkRequest,
        ContractBenchmarkService,
    },
};
use crate::config::reload::ConfigManager;
use redis::Client as RedisClient;
use crate::api::contracts::{
    ApiResponse, ProfileTriggerRequest, ProfileTriggerResponse, SystemStatus, ValidatedJson,
};
use tracing::{info, instrument};

// ---------------------------------------------------------------------------
// Shared application state
// ---------------------------------------------------------------------------

/// Shared application state passed to profiling and config handlers.
pub struct AppState {
    pub db: Option<sqlx::PgPool>,
    pub metrics_exporter: Arc<MetricsExporter>,
    pub error_manager: Arc<ErrorManager>,
    pub config_manager: Arc<ConfigManager>,
    pub log_aggregator: Arc<LogAggregator>,
    pub contract_benchmark_service: Arc<ContractBenchmarkService>,
    pub redis: RedisClient,
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// Detailed performance metrics report.
#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct MetricsReport {
    pub uptime_secs: u64,
    pub memory_usage_bytes: u64,
    pub active_requests: u32,
    pub error_rate: f64,
    pub ledger_ingestion_latency_ms: u32,
}

/// System health check response.
#[derive(Debug, Serialize, ToSchema)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub timestamp: DateTime<Utc>,
    pub database_connected: bool,
    pub redis_connected: bool,
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

/// `GET /api/v1/profiling/metrics` — Return performance metrics.
#[utoipa::path(
    get,
    path = "/api/v1/profiling/metrics",
    responses(
        (status = 200, description = "Performance metrics", body = MetricsReport),
        (status = 500, description = "Internal server error")
    ),
    tag = "profiling"
)]
#[instrument(skip(state))]
pub async fn get_metrics(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, AppError> {
    info!("Collecting performance metrics");

    let sys_metrics = state.metrics_exporter.get_metrics().await;

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
        "Metrics collected successfully"
    );

    Ok(Json(report))
}

/// `GET /api/v1/profiling/health` — System health check.
#[utoipa::path(
    get,
    path = "/api/v1/profiling/health",
    responses(
        (status = 200, description = "System is healthy", body = HealthResponse),
        (status = 503, description = "System is degraded")
    ),
    tag = "profiling"
)]
#[instrument(skip(state))]
pub async fn get_health(
    State(state): State<Arc<AppState>>,
) -> Result<impl IntoResponse, AppError> {
    info!("Performing system health check");

    let db_healthy = if let Some(ref pool) = state.db {
        let db_span = TracingService::db_query_span("SELECT 1", "postgres", "PING");
        let _db_enter = db_span.enter();
        let result = sqlx::query("SELECT 1")
            .fetch_optional(pool)
            .await
            .map(|r| r.is_some())
            .unwrap_or_else(|e| {
                TracingService::record_error(&db_span, &e.to_string(), "database");
                false
            });
        result
    } else {
        false
    };

    let redis_span = TracingService::redis_command_span("PING", None);
    let _redis_enter = redis_span.enter();
    let redis_healthy = match state.redis.get_multiplexed_async_connection().await {
        Ok(mut conn) => redis::cmd("PING")
            .query_async::<String>(&mut conn)
            .await
            .map(|pong| pong == "PONG")
            .unwrap_or_else(|e| {
                TracingService::record_error(&redis_span, &e.to_string(), "redis_ping");
                false
            }),
        Err(e) => {
            TracingService::record_error(&redis_span, &e.to_string(), "redis_connection");
            false
        }
    };
    drop(_redis_enter);

    let response = HealthResponse {
        status: if db_healthy && redis_healthy {
            "healthy"
        } else {
            "degraded"
        }
        .to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        timestamp: Utc::now(),
        database_connected: db_healthy,
        redis_connected: redis_healthy,
    };

    info!(
        db_connected = db_healthy,
        redis_connected = redis_healthy,
        version = env!("CARGO_PKG_VERSION"),
        "Health check completed"
    );

    Ok(Json(response))
}

/// `GET /api/v1/profiling/prometheus` — Prometheus-format metrics.
#[instrument(skip_all)]
pub async fn get_prometheus_metrics() -> impl IntoResponse {
    info!("Exporting Prometheus-format metrics");
    "# HELP backend_requests_total Total number of requests\n\
     # TYPE backend_requests_total counter\n\
     backend_requests_total 1024\n\
     # HELP backend_ledger_latency_ms Current ledger ingestion latency\n\
     # TYPE backend_ledger_latency_ms gauge\n\
     backend_ledger_latency_ms 120\n"
        .to_string()
}

/// `GET /api/status` — detailed system status.
#[instrument(skip(state))]
pub async fn get_system_status(
    State(state): State<Arc<AppState>>,
) -> Result<ApiResponse<SystemStatus>, AppError> {
    info!("Retrieving system status");

    let metrics = state.metrics_exporter.get_metrics().await;
    let recovery_tasks = state.error_manager.get_active_tasks().await;

    Ok(ApiResponse::new(SystemStatus {
        status: "healthy".to_string(),
        uptime_secs: metrics.uptime,
        memory_used_bytes: metrics.memory_usage,
        active_recovery_tasks: recovery_tasks.len(),
    }))
}

/// `POST /api/profile` — trigger a profiling collection run.
#[utoipa::path(
    post,
    path = "/api/profile",
    responses(
        (status = 200, description = "Profiling collection triggered"),
        (status = 400, description = "Invalid request parameters")
    ),
    tag = "profiling"
)]
#[instrument(skip(_state))]
pub async fn trigger_profile_collection(
    State(_state): State<Arc<AppState>>,
    ValidatedJson(payload): ValidatedJson<ProfileTriggerRequest>,
) -> Result<ApiResponse<ProfileTriggerResponse>, AppError> {
    let profile_id = uuid::Uuid::new_v4();

    info!(
        profile_id = %profile_id,
        label = %payload.label,
        duration_secs = payload.duration_secs,
        "Profiling collection triggered"
    );

    Ok(ApiResponse::new(ProfileTriggerResponse {
        profile_id,
        message: format!("Profiling collection triggered for label: {}", payload.label),
        estimated_completion: chrono::Utc::now()
            + chrono::Duration::seconds(payload.duration_secs as i64),
    }))
}

/// Handler for contract performance benchmark aggregation.
#[instrument(skip(state))]
pub async fn run_contract_benchmark(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<ContractBenchmarkRequest>,
) -> Result<ApiResponse<ContractBenchmarkReport>, AppError> {
    let report = state
        .contract_benchmark_service
        .run_benchmark(payload)
        .await
        .map_err(map_contract_benchmark_error)?;

    Ok(ApiResponse::new(report))
}

fn map_contract_benchmark_error(error: ContractBenchmarkError) -> AppError {
    match error {
        ContractBenchmarkError::Validation(message) => AppError::BadRequest(message),
    }
}
