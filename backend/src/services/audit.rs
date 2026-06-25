//! Audit logging service for security events.
//!
//! This module provides async audit logging and report retrieval for security
//! events using Axum, SQLx, and Redis.

use axum::extract::State;
use axum::response::IntoResponse;
use axum::{
    extract::{Path, Query},
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::{info, instrument};
use redis::AsyncCommands;
use std::sync::Arc;
use utoipa::ToSchema;

use crate::error::AppError;

#[derive(Debug, Serialize, Deserialize, Clone, sqlx::FromRow, ToSchema)]
pub struct AuditEventRecord {
    pub id: i64,
    pub event_type: String,
    pub user_id: Option<String>,
    pub details: serde_json::Value,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Serialize, Deserialize, Clone, ToSchema)]
pub struct AuditEvent {
    pub event_type: String,
    pub user_id: Option<String>,
    pub details: serde_json::Value,
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

#[derive(Clone)]
pub struct AuditService {
    pub db: PgPool,
    pub redis: Arc<redis::Client>,
}

impl AuditService {
    pub fn new(db: PgPool, redis: Arc<redis::Client>) -> Self {
        Self { db, redis }
    }

    /// Log an audit event to the database and enqueue it for async processing.
    #[instrument(skip(self))]
    pub async fn log_event(&self, event: AuditEvent) -> Result<(), AppError> {
        sqlx::query(
            r#"INSERT INTO audit_logs (event_type, user_id, details, timestamp)
               VALUES ($1, $2, $3, $4)"#,
        )
        .bind(&event.event_type)
        .bind(&event.user_id)
        .bind(&event.details)
        .bind(event.timestamp)
        .execute(&self.db)
        .await
        .map_err(AppError::Database)?;

        let mut conn = self.redis.get_async_connection().await.map_err(AppError::Redis)?;
        let event_json = serde_json::to_string(&event).map_err(AppError::Serialization)?;
        conn.lpush::<_, _, ()>("audit_queue", event_json).await.map_err(AppError::Redis)?;

        info!(event_type = %event.event_type, "Audit event logged");
        Ok(())
    }

    /// Search audit logs with optional filters.
    #[instrument(skip(self))]
    pub async fn search_audit_logs(
        &self,
        event_type: Option<String>,
        user_id: Option<String>,
        start_time: Option<chrono::DateTime<chrono::Utc>>,
        end_time: Option<chrono::DateTime<chrono::Utc>>,
        limit: Option<i64>,
    ) -> Result<Vec<AuditEvent>, AppError> {
        let mut query_builder = sqlx::QueryBuilder::new(
            "SELECT event_type, user_id, details, timestamp FROM audit_logs WHERE 1=1"
        );

        if let Some(event_type) = event_type {
            query_builder.push(" AND event_type = ");
            query_builder.push_bind(event_type);
        }

        if let Some(user_id) = user_id {
            query_builder.push(" AND user_id = ");
            query_builder.push_bind(user_id);
        }

        if let Some(start_time) = start_time {
            query_builder.push(" AND timestamp >= ");
            query_builder.push_bind(start_time);
        }

        if let Some(end_time) = end_time {
            query_builder.push(" AND timestamp <= ");
            query_builder.push_bind(end_time);
        }

        query_builder.push(" ORDER BY timestamp DESC");

        if let Some(limit) = limit {
            query_builder.push(" LIMIT ");
            query_builder.push_bind(limit);
        }

        let query = query_builder.build();
        let rows = query.fetch_all(&self.db).await.map_err(AppError::Database)?;

        let mut results = Vec::new();
        for row in rows {
            use sqlx::Row;
            let event = AuditEvent {
                event_type: row.get::<String, _>("event_type"),
                user_id: row.get::<Option<String>, _>("user_id"),
                details: row.get::<serde_json::Value, _>("details"),
                timestamp: row.get::<chrono::DateTime<chrono::Utc>, _>("timestamp"),
            };
            results.push(event);
        }

        Ok(results)
    }

    /// Export audit logs as JSON for external processing.
    #[instrument(skip(self))]
    pub async fn export_audit_logs(
        &self,
        event_type: Option<String>,
        user_id: Option<String>,
        start_time: Option<chrono::DateTime<chrono::Utc>>,
        end_time: Option<chrono::DateTime<chrono::Utc>>,
        limit: Option<i64>,
    ) -> Result<Vec<AuditEvent>, AppError> {
        self.search_audit_logs(event_type, user_id, start_time, end_time, limit).await
    }

    /// Return the most recent audit events, optionally filtered by event type.
    pub async fn list_events(
        &self,
        event_type: Option<String>,
        limit: u32,
    ) -> Result<Vec<AuditEventRecord>, AppError> {
        let limit = limit.clamp(1, 200) as i64;

        let rows = if let Some(event_type) = event_type {
            sqlx::query_as::<_, AuditEventRecord>(
                "SELECT id, event_type, user_id, details, timestamp FROM audit_logs WHERE event_type = $1 ORDER BY timestamp DESC LIMIT $2",
            )
            .bind(event_type)
            .bind(limit)
            .fetch_all(&self.db)
            .await?
        } else {
            sqlx::query_as::<_, AuditEventRecord>(
                "SELECT id, event_type, user_id, details, timestamp FROM audit_logs ORDER BY timestamp DESC LIMIT $1",
            )
            .bind(limit)
            .fetch_all(&self.db)
            .await?
        };

        Ok(rows)
    }

    /// Retrieve a single audit event report by ID.
    pub async fn get_event(&self, id: i64) -> Result<AuditEventRecord, AppError> {
        let event = sqlx::query_as::<_, AuditEventRecord>(
            "SELECT id, event_type, user_id, details, timestamp FROM audit_logs WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.db)
        .await?;

        event.ok_or_else(|| AppError::NotFound(format!("Audit report {} not found", id)))
    }
}

#[derive(Debug, Deserialize, ToSchema)]
pub struct AuditEventRequest {
    pub event_type: String,
    pub user_id: Option<String>,
    pub details: serde_json::Value,
}

#[derive(Debug, Deserialize)]
pub struct AuditReportQuery {
    pub event_type: Option<String>,
    #[serde(default = "default_limit")]
    pub limit: u32,
}

fn default_limit() -> u32 {
    50
}

/// Axum handler for logging audit events.
#[instrument(skip(service))]
pub async fn log_audit_event(
    State(service): State<Arc<AuditService>>,
    Json(payload): Json<AuditEventRequest>,
) -> Result<impl IntoResponse, AppError> {
    let event = AuditEvent {
        event_type: payload.event_type,
        user_id: payload.user_id,
        details: payload.details,
        timestamp: chrono::Utc::now(),
    };
    service.log_event(event).await?;
    Ok(axum::http::StatusCode::CREATED)
}

/// Axum handler for listing audit reports.
#[instrument(skip(service))]
pub async fn list_audit_reports(
    State(service): State<Arc<AuditService>>,
    Query(query): Query<AuditReportQuery>,
) -> Result<impl IntoResponse, AppError> {
    let events = service.list_events(query.event_type, query.limit).await?;
    Ok(Json(events))
}

/// Axum handler for retrieving a single audit report.
#[instrument(skip(service))]
pub async fn get_audit_report(
    State(service): State<Arc<AuditService>>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, AppError> {
    let event = service.get_event(id).await?;
    Ok(Json(event))
}

/// Add audit routes to the Axum router.
pub fn routes(service: Arc<AuditService>) -> Router {
    Router::new()
        .route("/log", post(log_audit_event))
        .route("/reports", get(list_audit_reports))
        .route("/reports/:id", get(get_audit_report))
        .with_state(service)
}
