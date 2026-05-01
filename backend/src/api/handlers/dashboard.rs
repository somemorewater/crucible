use axum::{Json, response::IntoResponse, extract::{State, Path}};
use serde::{Serialize, Deserialize};
use tracing::{info, instrument, error};
use chrono::{DateTime, Utc};
use crate::error::AppError;
use utoipa::ToSchema;
use std::sync::Arc;
use sqlx::PgPool;
use redis::AsyncCommands;

/// Shared application state for dashboard handlers
pub struct DashboardState {
    pub db: PgPool,
    pub redis: redis::aio::ConnectionManager,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct DashboardMetrics {
    /// Total number of active contracts
    pub total_contracts: i64,
    /// Total number of transactions processed
    pub total_transactions: i64,
    /// Average transaction processing time in milliseconds
    pub avg_processing_time_ms: f64,
    /// Number of failed transactions in the last 24 hours
    pub failed_transactions_24h: i64,
    /// Timestamp of the metrics snapshot
    pub timestamp: DateTime<Utc>,
}

#[derive(Debug, Serialize, Deserialize, ToSchema)]
pub struct ContractStats {
    /// Contract identifier
    pub contract_id: String,
    /// Number of invocations
    pub invocation_count: i64,
    /// Last invocation timestamp
    pub last_invoked: Option<DateTime<Utc>>,
    /// Average gas cost
    pub avg_gas_cost: f64,
}

/// Retrieves aggregated dashboard metrics with Redis caching
#[utoipa::path(
    get,
    path = "/api/v1/dashboard/metrics",
    responses(
        (status = 200, description = "Dashboard metrics retrieved successfully", body = DashboardMetrics),
        (status = 500, description = "Internal server error")
    ),
    tag = "dashboard"
)]
#[instrument(skip(state))]
pub async fn get_dashboard_metrics(
    State(state): State<Arc<DashboardState>>,
) -> Result<impl IntoResponse, AppError> {
    info!("Fetching dashboard metrics");

    // Try cache first
    let cache_key = "dashboard:metrics";
    let mut redis_conn = state.redis.clone();
    
    if let Ok(cached) = redis_conn.get::<_, String>(cache_key).await {
        if let Ok(metrics) = serde_json::from_str::<DashboardMetrics>(&cached) {
            info!("Returning cached dashboard metrics");
            return Ok(Json(metrics));
        }
    }

    // Fetch from database
    let total_contracts = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM contracts"
    )
    .fetch_optional(&state.db)
    .await?
    .unwrap_or(0);

    let total_transactions = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM transactions"
    )
    .fetch_optional(&state.db)
    .await?
    .unwrap_or(0);

    let avg_processing_time = sqlx::query_scalar::<_, Option<f64>>(
        "SELECT AVG(processing_time_ms) FROM transactions WHERE processing_time_ms IS NOT NULL"
    )
    .fetch_one(&state.db)
    .await?
    .unwrap_or(0.0);

    let failed_24h = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM transactions 
         WHERE status = 'failed' AND created_at > NOW() - INTERVAL '24 hours'"
    )
    .fetch_optional(&state.db)
    .await?
    .unwrap_or(0);

    let metrics = DashboardMetrics {
        total_contracts,
        total_transactions,
        avg_processing_time_ms: avg_processing_time,
        failed_transactions_24h: failed_24h,
        timestamp: Utc::now(),
    };

    // Cache for 60 seconds
    if let Ok(json) = serde_json::to_string(&metrics) {
        let _: Result<(), _> = redis_conn.set_ex(cache_key, json, 60).await;
    }

    info!(
        contracts = metrics.total_contracts,
        transactions = metrics.total_transactions,
        "Dashboard metrics retrieved"
    );

    Ok(Json(metrics))
}

/// Retrieves statistics for a specific contract
#[utoipa::path(
    get,
    path = "/api/v1/dashboard/contracts/{contract_id}/stats",
    params(
        ("contract_id" = String, Path, description = "Contract identifier")
    ),
    responses(
        (status = 200, description = "Contract statistics retrieved", body = ContractStats),
        (status = 404, description = "Contract not found"),
        (status = 500, description = "Internal server error")
    ),
    tag = "dashboard"
)]
#[instrument(skip(state))]
pub async fn get_contract_stats(
    State(state): State<Arc<DashboardState>>,
    Path(contract_id): Path<String>,
) -> Result<impl IntoResponse, AppError> {
    info!(contract_id = %contract_id, "Fetching contract statistics");

    let cache_key = format!("dashboard:contract:{}:stats", contract_id);
    let mut redis_conn = state.redis.clone();

    // Check cache
    if let Ok(cached) = redis_conn.get::<_, String>(&cache_key).await {
        if let Ok(stats) = serde_json::from_str::<ContractStats>(&cached) {
            return Ok(Json(stats));
        }
    }

    // Query database
    let result = sqlx::query!(
        r#"
        SELECT 
            COUNT(*) as "invocation_count!",
            MAX(created_at) as last_invoked,
            AVG(gas_cost) as avg_gas_cost
        FROM transactions
        WHERE contract_id = $1
        "#,
        contract_id
    )
    .fetch_optional(&state.db)
    .await?;

    let stats = match result {
        Some(row) if row.invocation_count > 0 => ContractStats {
            contract_id: contract_id.clone(),
            invocation_count: row.invocation_count,
            last_invoked: row.last_invoked,
            avg_gas_cost: row.avg_gas_cost.unwrap_or(0.0),
        },
        _ => {
            error!(contract_id = %contract_id, "Contract not found");
            return Err(AppError::NotFound(format!("Contract {} not found", contract_id)));
        }
    };

    // Cache for 30 seconds
    if let Ok(json) = serde_json::to_string(&stats) {
        let _: Result<(), _> = redis_conn.set_ex(&cache_key, json, 30).await;
    }

    Ok(Json(stats))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dashboard_metrics_serialization() {
        let metrics = DashboardMetrics {
            total_contracts: 100,
            total_transactions: 5000,
            avg_processing_time_ms: 125.5,
            failed_transactions_24h: 3,
            timestamp: Utc::now(),
        };

        let json = serde_json::to_string(&metrics).unwrap();
        let deserialized: DashboardMetrics = serde_json::from_str(&json).unwrap();
        
        assert_eq!(deserialized.total_contracts, 100);
        assert_eq!(deserialized.total_transactions, 5000);
    }

    #[test]
    fn test_contract_stats_serialization() {
        let stats = ContractStats {
            contract_id: "test_contract_123".to_string(),
            invocation_count: 42,
            last_invoked: Some(Utc::now()),
            avg_gas_cost: 1500.75,
        };

        let json = serde_json::to_string(&stats).unwrap();
        let deserialized: ContractStats = serde_json::from_str(&json).unwrap();
        
        assert_eq!(deserialized.contract_id, "test_contract_123");
        assert_eq!(deserialized.invocation_count, 42);
//! Dashboard data API handler.
//!
//! Provides a single `GET /api/dashboard` endpoint that aggregates system
//! metrics, active recovery tasks, and active alerts into one response.
//! Results are cached in Redis for [`CACHE_TTL_SECS`] seconds to reduce
//! load on downstream services.
//!
//! # Example
//! ```rust,no_run
//! use std::sync::Arc;
//! use axum::{Router, routing::get};
//! use backend::api::handlers::dashboard::{DashboardState, get_dashboard};
//!
//! # async fn example() {
//! // state is constructed with your real service instances
//! # }
//! ```

use axum::{extract::State, response::IntoResponse, Json};
use redis::{AsyncCommands, Client as RedisClient};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;
use tracing::{debug, error, warn};

use crate::services::{
    error_recovery::{ErrorManager, RecoveryTask},
    log_alerts::{Alert, AlertManager},
    sys_metrics::{MetricsExporter, SystemMetrics},
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const CACHE_KEY: &str = "dashboard:summary";
const CACHE_TTL_SECS: u64 = 30;

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur while building the dashboard response.
#[derive(Debug, Error)]
pub enum DashboardError {
    /// A Redis error occurred.
    #[error("Cache error: {0}")]
    Cache(#[from] redis::RedisError),

    /// JSON serialization/deserialization failed.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
}

impl IntoResponse for DashboardError {
    fn into_response(self) -> axum::response::Response {
        error!(error = %self, "Dashboard handler error");
        let body = serde_json::json!({ "error": self.to_string() });
        (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(body)).into_response()
    }
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// Aggregated dashboard data returned by `GET /api/dashboard`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DashboardData {
    /// Current system metrics snapshot.
    pub metrics: SystemMetrics,
    /// Recovery tasks that are currently active.
    pub active_recovery_tasks: Vec<RecoveryTask>,
    /// Alerts that have fired and not yet been resolved.
    pub active_alerts: Vec<Alert>,
}

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Shared application state for the dashboard handler.
pub struct DashboardState {
    pub metrics_exporter: Arc<MetricsExporter>,
    pub error_manager: Arc<ErrorManager>,
    pub alert_manager: Arc<AlertManager>,
    pub redis: RedisClient,
}

// ---------------------------------------------------------------------------
// Handler
// ---------------------------------------------------------------------------

/// `GET /api/dashboard` — return aggregated dashboard data.
///
/// Attempts to serve a cached response from Redis. On cache miss (or cache
/// error) the data is assembled from the live services and the cache is
/// populated before responding.
#[tracing::instrument(skip(state))]
pub async fn get_dashboard(
    State(state): State<Arc<DashboardState>>,
) -> Result<impl IntoResponse, DashboardError> {
    // --- try cache ---
    match try_cache_get(&state.redis).await {
        Ok(Some(cached)) => {
            debug!("Dashboard cache hit");
            return Ok(Json(cached));
        }
        Ok(None) => debug!("Dashboard cache miss"),
        Err(e) => warn!(error = %e, "Dashboard cache read failed; falling back to live data"),
    }

    // --- assemble live data ---
    let (metrics, active_recovery_tasks, active_alerts) = tokio::join!(
        state.metrics_exporter.get_metrics(),
        state.error_manager.get_active_tasks(),
        state.alert_manager.get_active_alerts(),
    );

    let data = DashboardData {
        metrics,
        active_recovery_tasks,
        active_alerts,
    };

    // --- populate cache (best-effort) ---
    if let Err(e) = try_cache_set(&state.redis, &data).await {
        warn!(error = %e, "Failed to populate dashboard cache");
    }

    Ok(Json(data))
}

// ---------------------------------------------------------------------------
// Cache helpers
// ---------------------------------------------------------------------------

async fn try_cache_get(redis: &RedisClient) -> Result<Option<DashboardData>, DashboardError> {
    let mut conn = redis.get_multiplexed_async_connection().await?;
    let raw: Option<String> = conn.get(CACHE_KEY).await?;
    match raw {
        Some(s) => Ok(Some(serde_json::from_str(&s)?)),
        None => Ok(None),
    }
}

async fn try_cache_set(redis: &RedisClient, data: &DashboardData) -> Result<(), DashboardError> {
    let serialized = serde_json::to_string(data)?;
    let mut conn = redis.get_multiplexed_async_connection().await?;
    let _: () = conn.set_ex(CACHE_KEY, serialized, CACHE_TTL_SECS).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{body::Body, http::Request, routing::get, Router};
    use tower::ServiceExt;

    fn make_state() -> Arc<DashboardState> {
        Arc::new(DashboardState {
            metrics_exporter: Arc::new(MetricsExporter::new()),
            error_manager: Arc::new(ErrorManager::new()),
            alert_manager: Arc::new(AlertManager::new()),
            // Use a URL that will fail to connect — the handler degrades gracefully.
            redis: RedisClient::open("redis://127.0.0.1:1/").unwrap(),
        })
    }

    fn make_app(state: Arc<DashboardState>) -> Router {
        Router::new()
            .route("/api/dashboard", get(get_dashboard))
            .with_state(state)
    }

    #[tokio::test]
    async fn test_dashboard_returns_200_without_redis() {
        let app = make_app(make_state());

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/dashboard")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), axum::http::StatusCode::OK);
    }

    #[tokio::test]
    async fn test_dashboard_response_shape() {
        let app = make_app(make_state());

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/dashboard")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert!(json.get("metrics").is_some());
        assert!(json.get("active_recovery_tasks").is_some());
        assert!(json.get("active_alerts").is_some());
    }

    #[tokio::test]
    async fn test_dashboard_metrics_fields() {
        let state = make_state();
        state.metrics_exporter.update_metrics(42.0, 2048, 120).await;

        let app = make_app(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/dashboard")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        assert_eq!(json["metrics"]["cpu_usage"], 42.0);
        assert_eq!(json["metrics"]["memory_usage"], 2048);
        assert_eq!(json["metrics"]["uptime"], 120);
    }

    #[tokio::test]
    async fn test_dashboard_includes_recovery_tasks() {
        use crate::services::error_recovery::RecoveryError;

        let state = make_state();
        state
            .error_manager
            .handle_error(RecoveryError::Internal("boom".into()), "worker_a")
            .await
            .unwrap();

        let app = make_app(state);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/api/dashboard")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

        let tasks = json["active_recovery_tasks"].as_array().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0]["name"], "worker_a");
    }

    #[test]
    fn test_dashboard_error_display() {
        let err = DashboardError::Serialization(
            serde_json::from_str::<serde_json::Value>("bad json").unwrap_err(),
        );
        assert!(!err.to_string().is_empty());
    }

    #[test]
    fn test_dashboard_data_serialization_roundtrip() {
        let data = DashboardData {
            metrics: SystemMetrics::default(),
            active_recovery_tasks: vec![],
            active_alerts: vec![],
        };
        let json = serde_json::to_string(&data).unwrap();
        let back: DashboardData = serde_json::from_str(&json).unwrap();
        assert_eq!(back.active_recovery_tasks.len(), 0);
        assert_eq!(back.active_alerts.len(), 0);
    }
}
