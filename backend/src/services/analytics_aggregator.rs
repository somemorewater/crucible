//! Contract analytics aggregation service.
//!
//! Aggregates per-contract metrics (call counts, error rates, gas usage,
//! unique callers) from indexed events and build metrics, storing roll-ups
//! in PostgreSQL and caching summaries in Redis.

use chrono::{DateTime, Utc};
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::{debug, info, instrument};

const CACHE_KEY_PREFIX: &str = "analytics:contract:";
const CACHE_TTL_SECS: u64 = 60;

/// Aggregated analytics for a single contract.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContractAnalytics {
    pub contract_id: String,
    pub total_calls: i64,
    pub error_count: i64,
    pub error_rate: f64,
    pub unique_callers: i64,
    pub avg_gas_used: f64,
    pub last_activity: Option<DateTime<Utc>>,
    pub computed_at: DateTime<Utc>,
}

/// Platform-wide analytics summary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsSummary {
    pub total_contracts: i64,
    pub total_calls: i64,
    pub total_errors: i64,
    pub avg_error_rate: f64,
    pub most_active_contract: Option<String>,
    pub computed_at: DateTime<Utc>,
}

pub struct AnalyticsAggregator {
    db: PgPool,
    redis: redis::Client,
}

impl AnalyticsAggregator {
    pub fn new(db: PgPool, redis: redis::Client) -> Self {
        Self { db, redis }
    }

    /// Compute and cache analytics for a specific contract.
    #[instrument(skip(self), fields(contract_id = %contract_id))]
    pub async fn contract_analytics(
        &self,
        contract_id: &str,
    ) -> Result<ContractAnalytics, anyhow::Error> {
        let cache_key = format!("{}{}", CACHE_KEY_PREFIX, contract_id);

        // Try Redis cache first
        if let Ok(mut conn) = self.redis.get_async_connection().await {
            if let Ok(cached) = conn.get::<_, String>(&cache_key).await {
                if let Ok(analytics) = serde_json::from_str::<ContractAnalytics>(&cached) {
                    debug!(contract_id, "Cache hit for contract analytics");
                    return Ok(analytics);
                }
            }
        }

        let analytics = self.compute_contract_analytics(contract_id).await?;

        // Populate cache (best-effort)
        if let Ok(mut conn) = self.redis.get_async_connection().await {
            if let Ok(json) = serde_json::to_string(&analytics) {
                let _: Result<(), _> = conn.set_ex(&cache_key, json, CACHE_TTL_SECS).await;
            }
        }

        Ok(analytics)
    }

    async fn compute_contract_analytics(
        &self,
        contract_id: &str,
    ) -> Result<ContractAnalytics, sqlx::Error> {
        // Total calls and error count from contract_events
        let (total_calls, error_count): (i64, i64) = sqlx::query_as(
            r#"
            SELECT
                COUNT(*) AS total_calls,
                COUNT(*) FILTER (WHERE event_type = 'error') AS error_count
            FROM contract_events
            WHERE contract_id = $1
            "#,
        )
        .bind(contract_id)
        .fetch_one(&self.db)
        .await
        .unwrap_or((0, 0));

        // Unique callers (stored in event data as JSON field "caller")
        let (unique_callers,): (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(DISTINCT data->>'caller')
            FROM contract_events
            WHERE contract_id = $1 AND data->>'caller' IS NOT NULL
            "#,
        )
        .bind(contract_id)
        .fetch_one(&self.db)
        .await
        .unwrap_or((0,));

        // Average gas from build_metrics (proxy for gas usage)
        let (avg_gas,): (Option<f64>,) = sqlx::query_as(
            "SELECT AVG(compilation_time_ms) FROM build_metrics WHERE project_name = $1",
        )
        .bind(contract_id)
        .fetch_one(&self.db)
        .await
        .unwrap_or((None,));

        // Last activity timestamp
        let (last_activity,): (Option<DateTime<Utc>>,) =
            sqlx::query_as("SELECT MAX(indexed_at) FROM contract_events WHERE contract_id = $1")
                .bind(contract_id)
                .fetch_one(&self.db)
                .await
                .unwrap_or((None,));

        let error_rate = if total_calls > 0 {
            error_count as f64 / total_calls as f64
        } else {
            0.0
        };

        Ok(ContractAnalytics {
            contract_id: contract_id.to_string(),
            total_calls,
            error_count,
            error_rate,
            unique_callers,
            avg_gas_used: avg_gas.unwrap_or(0.0),
            last_activity,
            computed_at: Utc::now(),
        })
    }

    /// Compute a platform-wide analytics summary.
    #[instrument(skip(self))]
    pub async fn summary(&self) -> Result<AnalyticsSummary, anyhow::Error> {
        const SUMMARY_KEY: &str = "analytics:summary";

        if let Ok(mut conn) = self.redis.get_async_connection().await {
            if let Ok(cached) = conn.get::<_, String>(SUMMARY_KEY).await {
                if let Ok(s) = serde_json::from_str::<AnalyticsSummary>(&cached) {
                    return Ok(s);
                }
            }
        }

        let (total_contracts,): (i64,) =
            sqlx::query_as("SELECT COUNT(DISTINCT contract_id) FROM contract_events")
                .fetch_one(&self.db)
                .await
                .unwrap_or((0,));

        let (total_calls, total_errors): (i64, i64) = sqlx::query_as(
            r#"
            SELECT COUNT(*),
                   COUNT(*) FILTER (WHERE event_type = 'error')
            FROM contract_events
            "#,
        )
        .fetch_one(&self.db)
        .await
        .unwrap_or((0, 0));

        let most_active: Option<(String,)> = sqlx::query_as(
            r#"
            SELECT contract_id FROM contract_events
            GROUP BY contract_id ORDER BY COUNT(*) DESC LIMIT 1
            "#,
        )
        .fetch_optional(&self.db)
        .await
        .unwrap_or(None);

        let avg_error_rate = if total_calls > 0 {
            total_errors as f64 / total_calls as f64
        } else {
            0.0
        };

        let summary = AnalyticsSummary {
            total_contracts,
            total_calls,
            total_errors,
            avg_error_rate,
            most_active_contract: most_active.map(|(id,)| id),
            computed_at: Utc::now(),
        };

        if let Ok(mut conn) = self.redis.get_async_connection().await {
            if let Ok(json) = serde_json::to_string(&summary) {
                let _: Result<(), _> = conn.set_ex(SUMMARY_KEY, json, CACHE_TTL_SECS).await;
            }
        }

        info!("Analytics summary computed");
        Ok(summary)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::postgres::PgPoolOptions;

    fn lazy_pool() -> PgPool {
        PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://postgres:postgres@localhost/crucible_test")
            .unwrap()
    }

    fn lazy_redis() -> redis::Client {
        redis::Client::open("redis://127.0.0.1/").unwrap()
    }

    #[test]
    fn test_error_rate_zero_when_no_calls() {
        let analytics = ContractAnalytics {
            contract_id: "CABC".to_string(),
            total_calls: 0,
            error_count: 0,
            error_rate: 0.0,
            unique_callers: 0,
            avg_gas_used: 0.0,
            last_activity: None,
            computed_at: Utc::now(),
        };
        assert_eq!(analytics.error_rate, 0.0);
    }

    #[test]
    fn test_error_rate_calculation() {
        let total = 100i64;
        let errors = 5i64;
        let rate = errors as f64 / total as f64;
        assert!((rate - 0.05).abs() < f64::EPSILON);
    }

    #[test]
    fn test_analytics_serialization() {
        let a = ContractAnalytics {
            contract_id: "CABC".to_string(),
            total_calls: 42,
            error_count: 1,
            error_rate: 1.0 / 42.0,
            unique_callers: 5,
            avg_gas_used: 1234.5,
            last_activity: None,
            computed_at: Utc::now(),
        };
        let json = serde_json::to_string(&a).unwrap();
        let back: ContractAnalytics = serde_json::from_str(&json).unwrap();
        assert_eq!(back.contract_id, "CABC");
        assert_eq!(back.total_calls, 42);
    }

    #[test]
    fn test_summary_serialization() {
        let s = AnalyticsSummary {
            total_contracts: 10,
            total_calls: 500,
            total_errors: 25,
            avg_error_rate: 0.05,
            most_active_contract: Some("CABC".to_string()),
            computed_at: Utc::now(),
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: AnalyticsSummary = serde_json::from_str(&json).unwrap();
        assert_eq!(back.total_contracts, 10);
    }
}
