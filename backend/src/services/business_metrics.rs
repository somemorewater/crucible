use std::collections::HashMap;
use std::sync::Arc;
use chrono::{DateTime, Duration, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio::sync::RwLock;
use tracing::{error, info, instrument, warn};
use uuid::Uuid;

use crate::error::AppError;

// ─── Domain Types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BusinessMetric {
    pub id: Uuid,
    pub name: String,
    pub value: Decimal,
    pub unit: String,
    pub category: MetricCategory,
    pub tags: HashMap<String, String>,
    pub recorded_at: DateTime<Utc>,
    pub source: MetricSource,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MetricCategory {
    Revenue,
    Costs,
    Users,
    Transactions,
    Performance,
    Custom(String),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum MetricSource {
    OnChain,
    OffChain,
    Database,
    ExternalApi,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSnapshot {
    pub timestamp: DateTime<Utc>,
    pub metrics: Vec<BusinessMetric>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsQuery {
    pub category: Option<MetricCategory>,
    pub from: Option<DateTime<Utc>>,
    pub to: Option<DateTime<Utc>>,
    pub tags: Option<HashMap<String, String>>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsSummary {
    pub total_metrics: i64,
    pub categories: HashMap<String, i64>,
    pub latest_timestamp: Option<DateTime<Utc>>,
}

// ─── Service ─────────────────────────────────────────────────────────────────

pub struct BusinessMetricsService {
    db: PgPool,
    cache: Arc<RwLock<HashMap<String, Vec<BusinessMetric>>>>,
}

impl BusinessMetricsService {
    pub fn new(db: PgPool) -> Self {
        Self {
            db,
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Record a new business metric with the given parameters.
    #[instrument(skip(self), fields(metric_name = %name))]
    pub async fn record_metric(
        &self,
        name: impl Into<String>,
        value: Decimal,
        unit: impl Into<String>,
        category: MetricCategory,
        tags: HashMap<String, String>,
        source: MetricSource,
    ) -> Result<BusinessMetric, AppError> {
        let id = Uuid::new_v4();
        let now = Utc::now();
        let name = name.into();
        let unit = unit.into();

        sqlx::query_as!(
            BusinessMetric,
            r#"
            INSERT INTO business_metrics (id, name, value, unit, category, tags, recorded_at, source)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING id, name, value, unit, category as "category: _", tags as "tags: _", recorded_at, source as "source: _"
            "#,
            id,
            name,
            value,
            unit,
            category as MetricCategory,
            serde_json::to_value(&tags)?,
            now,
            source as MetricSource,
        )
        .fetch_one(&self.db)
        .await
        .map_err(|e| {
            error!(error = %e, "Failed to record metric");
            AppError::Database(e)
        })?;

        let metric = BusinessMetric {
            id,
            name,
            value,
            unit,
            category,
            tags,
            recorded_at: now,
            source,
        };

        // Update in-memory cache
        {
            let mut cache = self.cache.write().await;
            let entry = cache.entry(metric.name.clone()).or_default();
            entry.push(metric.clone());
            // Keep last 1000 values per metric
            if entry.len() > 1000 {
                entry.remove(0);
            }
        }

        info!(
            metric_name = %metric.name,
            value = %metric.value,
            category = ?metric.category,
            "Recorded business metric"
        );

        Ok(metric)
    }

    /// Record multiple metrics in a single transaction.
    #[instrument(skip(self, metrics))]
    pub async fn record_metrics_batch(
        &self,
        metrics: Vec<(String, Decimal, String, MetricCategory, HashMap<String, String>, MetricSource)>,
    ) -> Result<Vec<BusinessMetric>, AppError> {
        let mut tx = self.db.begin().await?;
        let mut results = Vec::with_capacity(metrics.len());
        let now = Utc::now();

        for (name, value, unit, category, tags, source) in metrics {
            let id = Uuid::new_v4();

            sqlx::query!(
                r#"
                INSERT INTO business_metrics (id, name, value, unit, category, tags, recorded_at, source)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                "#,
                id,
                name,
                value,
                unit,
                serde_json::to_value(&tags)?,
                now,
                source as MetricSource,
            )
            .execute(&mut *tx)
            .await
            .map_err(|e| {
                error!(error = %e, "Failed in batch metric insert");
                AppError::Database(e)
            })?;

            results.push(BusinessMetric {
                id,
                name,
                value,
                unit,
                category,
                tags,
                recorded_at: now,
                source,
            });
        }

        tx.commit().await.map_err(|e| {
            error!(error = %e, "Failed to commit batch metrics");
            AppError::Database(e)
        })?;

        info!(count = results.len(), "Recorded batch metrics");
        Ok(results)
    }

    /// Query metrics with optional filters.
    #[instrument(skip(self))]
    pub async fn query_metrics(
        &self,
        query: MetricsQuery,
    ) -> Result<(Vec<BusinessMetric>, i64), AppError> {
        let limit = query.limit.unwrap_or(100);
        let offset = query.offset.unwrap_or(0);

        let total = sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!" FROM business_metrics WHERE 1=1"#
        )
        .fetch_one(&self.db)
        .await
        .map_err(|e| AppError::Database(e))?
        .unwrap_or(0);

        let metrics = sqlx::query_as!(
            BusinessMetric,
            r#"
            SELECT id, name, value, unit, category as "category: _", tags as "tags: _", recorded_at, source as "source: _"
            FROM business_metrics
            ORDER BY recorded_at DESC
            LIMIT $1 OFFSET $2
            "#,
            limit,
            offset,
        )
        .fetch_all(&self.db)
        .await
        .map_err(|e| AppError::Database(e))?;

        Ok((metrics, total))
    }

    /// Get aggregated metrics summary.
    #[instrument(skip(self))]
    pub async fn get_metrics_summary(&self) -> Result<MetricsSummary, AppError> {
        let total: i64 = sqlx::query_scalar!(
            r#"SELECT COUNT(*) as "count!" FROM business_metrics"#
        )
        .fetch_one(&self.db)
        .await
        .map_err(|e| AppError::Database(e))?
        .unwrap_or(0);

        let latest: Option<DateTime<Utc>> = sqlx::query_scalar!(
            r#"SELECT MAX(recorded_at) as "max!" FROM business_metrics"#
        )
        .fetch_one(&self.db)
        .await
        .map_err(|e| AppError::Database(e))?;

        let rows = sqlx::query!(
            r#"SELECT category as "category!: MetricCategory", COUNT(*) as "count!: i64" FROM business_metrics GROUP BY category"#
        )
        .fetch_all(&self.db)
        .await
        .map_err(|e| AppError::Database(e))?;

        let mut categories = HashMap::new();
        for row in rows {
            let key = match row.category {
                MetricCategory::Custom(s) => s,
                other => format!("{:?}", other).to_lowercase(),
            };
            categories.insert(key, row.count);
        }

        Ok(MetricsSummary {
            total_metrics: total,
            categories,
            latest_timestamp: latest,
        })
    }

    /// Compute aggregated values for a metric over a time range.
    #[instrument(skip(self))]
    pub async fn aggregate_metric(
        &self,
        name: &str,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
    ) -> Result<Option<Decimal>, AppError> {
        let result = sqlx::query_scalar!(
            r#"SELECT SUM(value) as "sum!: Decimal" FROM business_metrics WHERE name = $1 AND recorded_at >= $2 AND recorded_at <= $3"#,
            name,
            from,
            to,
        )
        .fetch_one(&self.db)
        .await
        .map_err(|e| AppError::Database(e))?;

        Ok(result)
    }

    /// Get the latest value for a specific metric.
    #[instrument(skip(self))]
    pub async fn get_latest_metric(
        &self,
        name: &str,
    ) -> Result<Option<BusinessMetric>, AppError> {
        // Check cache first
        {
            let cache = self.cache.read().await;
            if let Some(values) = cache.get(name) {
                if let Some(latest) = values.last() {
                    return Ok(Some(latest.clone()));
                }
            }
        }

        // Fall back to database
        let metric = sqlx::query_as!(
            BusinessMetric,
            r#"
            SELECT id, name, value, unit, category as "category: _", tags as "tags: _", recorded_at, source as "source: _"
            FROM business_metrics
            WHERE name = $1
            ORDER BY recorded_at DESC
            LIMIT 1
            "#,
            name,
        )
        .fetch_optional(&self.db)
        .await
        .map_err(|e| AppError::Database(e))?;

        Ok(metric)
    }

    /// Remove metrics older than the retention period.
    #[instrument(skip(self))]
    pub async fn prune_old_metrics(&self, retention_days: i64) -> Result<u64, AppError> {
        let cutoff = Utc::now() - Duration::days(retention_days);

        let deleted = sqlx::query!(
            r#"DELETE FROM business_metrics WHERE recorded_at < $1"#,
            cutoff,
        )
        .execute(&self.db)
        .await
        .map_err(|e| AppError::Database(e))?
        .rows_affected();

        info!(deleted, retention_days, "Pruned old metrics");
        Ok(deleted)
    }
}

// ─── API Handlers ────────────────────────────────────────────────────────────

use axum::{extract::State, http::StatusCode, Json};

pub struct MetricsState {
    pub service: Arc<BusinessMetricsService>,
}

#[derive(Debug, Deserialize)]
pub struct RecordMetricRequest {
    pub name: String,
    pub value: Decimal,
    pub unit: String,
    pub category: MetricCategory,
    #[serde(default)]
    pub tags: HashMap<String, String>,
    #[serde(default)]
    pub source: MetricSource,
}

/// POST /api/metrics — Record a new business metric.
#[utoipa::path(
    post,
    path = "/api/metrics",
    request_body = RecordMetricRequest,
    responses(
        (status = 201, description = "Metric recorded", body = BusinessMetric),
        (status = 400, description = "Invalid request"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn record_metric(
    State(state): State<Arc<MetricsState>>,
    Json(req): Json<RecordMetricRequest>,
) -> Result<(StatusCode, Json<BusinessMetric>), AppError> {
    let metric = state
        .service
        .record_metric(
            req.name,
            req.value,
            req.unit,
            req.category,
            req.tags,
            req.source,
        )
        .await?;

    Ok((StatusCode::CREATED, Json(metric)))
}

/// GET /api/metrics — Query business metrics with filters.
#[utoipa::path(
    get,
    path = "/api/metrics",
    params(
        ("category" = Option<MetricCategory>, Query, description = "Filter by category"),
        ("from" = Option<DateTime<Utc>>, Query, description = "Start of time range"),
        ("to" = Option<DateTime<Utc>>, Query, description = "End of time range"),
        ("limit" = Option<i64>, Query, description = "Max results"),
        ("offset" = Option<i64>, Query, description = "Pagination offset")
    ),
    responses(
        (status = 200, description = "List of metrics with total count"),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn query_metrics(
    State(state): State<Arc<MetricsState>>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> Result<Json<serde_json::Value>, AppError> {
    let category = params.get("category").and_then(|c| {
        serde_json::from_str(&format!("\"{}\"", c)).ok()
    });

    let from = params
        .get("from")
        .and_then(|v| v.parse::<DateTime<Utc>>().ok());
    let to = params
        .get("to")
        .and_then(|v| v.parse::<DateTime<Utc>>().ok());
    let limit = params.get("limit").and_then(|v| v.parse::<i64>().ok());
    let offset = params.get("offset").and_then(|v| v.parse::<i64>().ok());

    let query = MetricsQuery {
        category,
        from,
        to,
        tags: None,
        limit,
        offset,
    };

    let (metrics, total) = state.service.query_metrics(query).await?;

    Ok(Json(serde_json::json!({
        "metrics": metrics,
        "total": total,
    })))
}

/// GET /api/metrics/summary — Get aggregated metrics overview.
#[utoipa::path(
    get,
    path = "/api/metrics/summary",
    responses(
        (status = 200, description = "Metrics summary", body = MetricsSummary),
        (status = 500, description = "Internal server error")
    )
)]
pub async fn get_metrics_summary(
    State(state): State<Arc<MetricsState>>,
) -> Result<Json<MetricsSummary>, AppError> {
    let summary = state.service.get_metrics_summary().await?;
    Ok(Json(summary))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::PgPool;

    async fn setup_test_db() -> PgPool {
        let pool = PgPool::connect("postgres://localhost:5432/crucible_test")
            .await
            .expect("Failed to connect to test database");

        sqlx::query!(
            r#"
            CREATE TABLE IF NOT EXISTS business_metrics (
                id UUID PRIMARY KEY,
                name TEXT NOT NULL,
                value NUMERIC NOT NULL,
                unit TEXT NOT NULL,
                category TEXT NOT NULL,
                tags JSONB DEFAULT '{}',
                recorded_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                source TEXT NOT NULL DEFAULT 'manual'
            )
            "#
        )
        .execute(&pool)
        .await
        .expect("Failed to create test table");

        pool
    }

    #[tokio::test]
    async fn test_record_and_retrieve_metric() {
        let pool = setup_test_db().await;
        let service = BusinessMetricsService::new(pool);

        let metric = service
            .record_metric(
                "test_revenue",
                Decimal::new(1000, 0),
                "USD",
                MetricCategory::Revenue,
                HashMap::from([("region".into(), "us-east".into())]),
                MetricSource::Database,
            )
            .await
            .expect("Failed to record metric");

        assert_eq!(metric.name, "test_revenue");
        assert_eq!(metric.value, Decimal::new(1000, 0));

        let latest = service
            .get_latest_metric("test_revenue")
            .await
            .expect("Failed to get metric")
            .expect("Metric not found");

        assert_eq!(latest.value, Decimal::new(1000, 0));
    }

    #[tokio::test]
    async fn test_metrics_summary() {
        let pool = setup_test_db().await;
        let service = BusinessMetricsService::new(pool);

        service
            .record_metric(
                "revenue",
                Decimal::new(500, 0),
                "USD",
                MetricCategory::Revenue,
                HashMap::new(),
                MetricSource::Database,
            )
            .await
            .expect("Failed to record");

        service
            .record_metric(
                "cost",
                Decimal::new(200, 0),
                "USD",
                MetricCategory::Costs,
                HashMap::new(),
                MetricSource::Database,
            )
            .await
            .expect("Failed to record");

        let summary = service
            .get_metrics_summary()
            .await
            .expect("Failed to get summary");

        assert!(summary.total_metrics >= 2);
    }
}