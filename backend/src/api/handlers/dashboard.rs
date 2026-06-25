use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use chrono::{DateTime, Utc};
use redis::{AsyncCommands, Client as RedisClient};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use std::sync::Arc;
use tracing::{debug, error, info, instrument, warn};
use utoipa::ToSchema;

use crate::error::AppError;
use crate::services::{
    error_recovery::{ErrorManager, RecoveryTask},
    log_alerts::{Alert, AlertManager},
    sys_metrics::{MetricsExporter, SystemMetrics},
};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------
const CACHE_TTL_SECS: u64 = 30;

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

/// Shared application state for dashboard handlers.
pub struct DashboardState {
    pub db: PgPool,
    pub redis_conn: redis::aio::ConnectionManager,
    pub metrics_exporter: Arc<MetricsExporter>,
    pub error_manager: Arc<ErrorManager>,
    pub alert_manager: Arc<AlertManager>,
    pub redis_client: RedisClient,
}

// ---------------------------------------------------------------------------
// Response types
// ---------------------------------------------------------------------------

/// Aggregated dashboard metrics.
#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct DashboardMetrics {
    pub total_contracts: i64,
    pub total_transactions: i64,
    pub avg_processing_time_ms: f64,
    pub failed_transactions_24h: i64,
    pub timestamp: DateTime<Utc>,
}

/// Per-contract statistics.
#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct ContractStats {
    pub contract_id: String,
    pub invocation_count: i64,
    pub last_invoked: Option<DateTime<Utc>>,
    pub avg_gas_cost: f64,
}

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
// Handlers
// ---------------------------------------------------------------------------

/// `GET /api/v1/dashboard/metrics` — Aggregated dashboard metrics with Redis caching.
#[utoipa::path(
    get,
    path = "/api/v1/dashboard/metrics",
    responses(
        (status = 200, description = "Dashboard metrics", body = DashboardMetrics),
        (status = 500, description = "Internal server error")
    ),
    tag = "dashboard"
)]
#[instrument(skip(state))]
pub async fn get_dashboard_metrics(
    State(state): State<Arc<DashboardState>>,
) -> Result<impl IntoResponse, AppError> {
    info!("Fetching dashboard metrics");

    let cache_key = "dashboard:metrics";
    let mut redis_conn = state.redis_conn.clone();
    
    if let Ok(cached) = redis_conn.get::<_, String>(cache_key).await {
        if let Ok(metrics) = serde_json::from_str::<DashboardMetrics>(&cached) {
            info!("Returning cached dashboard metrics");
            return Ok(Json(metrics));
        }
    }

    let total_contracts = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM contracts")
        .fetch_one(&state.db)
        .await?;

    let total_transactions = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM transactions")
        .fetch_one(&state.db)
        .await?;

    let avg_processing_time = sqlx::query_scalar::<_, Option<f64>>(
        "SELECT AVG(processing_time_ms) FROM transactions WHERE processing_time_ms IS NOT NULL",
    )
    .fetch_one(&state.db)
    .await?
    .unwrap_or(0.0);

    let failed_24h = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM transactions \
         WHERE status = 'failed' AND created_at > NOW() - INTERVAL '24 hours'",
    )
    .fetch_one(&state.db)
    .await?;

    let metrics = DashboardMetrics {
        total_contracts,
        total_transactions,
        avg_processing_time_ms: avg_processing_time,
        failed_transactions_24h: failed_24h,
        timestamp: Utc::now(),
    };

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

/// `GET /api/v1/dashboard/contracts/:contract_id/stats` — Per-contract statistics.
#[utoipa::path(
    get,
    path = "/api/v1/dashboard/contracts/{contract_id}/stats",
    params(("contract_id" = String, Path, description = "Contract identifier")),
    responses(
        (status = 200, description = "Contract statistics", body = ContractStats),
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
    let mut redis_conn = state.redis_conn.clone();

    if let Ok(cached) = redis_conn.get::<_, String>(&cache_key).await {
        if let Ok(stats) = serde_json::from_str::<ContractStats>(&cached) {
            return Ok(Json(stats));
        }
    }

    // Verify contract exists
    let exists = sqlx::query_scalar::<_, i32>("SELECT 1 FROM contracts WHERE contract_id = $1")
        .bind(&contract_id)
        .fetch_optional(&state.db)
        .await?
        .is_some();

    if !exists {
        error!(contract_id = %contract_id, "Contract not found");
        return Err(AppError::NotFound(format!(
            "Contract {} not found",
            contract_id
        )));
    }

    let row = sqlx::query_as::<_, (i64, Option<DateTime<Utc>>, Option<f64>)>(
        r#"
        SELECT
            COUNT(*) as invocation_count,
            MAX(created_at) as last_invoked,
            AVG(gas_cost) as avg_gas_cost
        FROM transactions
        WHERE contract_id = $1
        "#
    )
    .bind(&contract_id)
    .fetch_one(&state.db)
    .await?;

    let stats = ContractStats {
        contract_id: contract_id.clone(),
        invocation_count: row.0,
        last_invoked: row.1,
        avg_gas_cost: row.2.unwrap_or(0.0),
    };

    if let Ok(json) = serde_json::to_string(&stats) {
        let _: Result<(), _> = redis_conn.set_ex(&cache_key, json, 30).await;
    }

    Ok(Json(stats))
}

/// `GET /api/dashboard` — return aggregated dashboard data.
#[instrument(skip(state))]
pub async fn get_dashboard(
    State(state): State<Arc<DashboardState>>,
) -> Result<impl IntoResponse, AppError> {
    info!("Fetching full dashboard data");
    
    let cache_key = "dashboard:summary";
    match try_cache_get::<DashboardData>(&state.redis_client, cache_key).await {
        Ok(Some(cached)) => {
            debug!("Dashboard cache hit");
            return Ok(Json(cached));
        }
        Ok(None) => debug!("Dashboard cache miss"),
        Err(e) => warn!(error = %e, "Dashboard cache read failed; falling back to live data"),
    }

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

    if let Err(e) = try_cache_set(&state.redis_client, cache_key, &data, CACHE_TTL_SECS).await {
        warn!(error = %e, "Failed to populate dashboard cache");
    }

    Ok(Json(data))
}

// ---------------------------------------------------------------------------
// Cache helpers
// ---------------------------------------------------------------------------
async fn try_cache_get<T>(redis: &RedisClient, key: &str) -> Result<Option<T>, AppError>
where
    T: for<'a> Deserialize<'a>,
{
    let mut conn = redis.get_multiplexed_async_connection().await?;
    let raw: Option<String> = conn.get(key).await?;
    match raw {
        Some(s) => Ok(Some(serde_json::from_str(&s)?)),
        None => Ok(None),
    }
}

async fn try_cache_set<T>(redis: &RedisClient, key: &str, data: &T, ttl_secs: u64) -> Result<(), AppError>
where
    T: Serialize,
{
    let serialized = serde_json::to_string(data)?;
    let mut conn = redis.get_multiplexed_async_connection().await?;
    let _: () = conn.set_ex(key, serialized, ttl_secs).await?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    fn make_state(db: PgPool, redis_conn: redis::aio::ConnectionManager) -> Arc<DashboardState> {
        Arc::new(DashboardState {
            db,
            redis_conn,
            metrics_exporter: Arc::new(MetricsExporter::new()),
            error_manager: Arc::new(ErrorManager::new()),
            alert_manager: Arc::new(AlertManager::new()),
            redis_client: RedisClient::open("redis://127.0.0.1:1/").unwrap(),
        })
    }

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
        let back: DashboardMetrics = serde_json::from_str(&json).unwrap();
        assert_eq!(back.total_contracts, 100);
        assert_eq!(back.total_transactions, 5000);
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
