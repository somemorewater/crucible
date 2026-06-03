//! Audit logging service for security events.
//!
//! This module provides async audit logging and report retrieval for security
//! events using Axum, SQLx, and Redis.

use axum::extract::State;
use axum::response::IntoResponse;
use axum::{Json, Router, routing::post};
use chrono::Utc;
use axum::{
    extract::{Path, Query, State},
    response::IntoResponse,
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
        sqlx::query!(
            r#"INSERT INTO audit_logs (event_type, user_id, details, timestamp)
               VALUES ($1, $2, $3, $4)"#,
            event.event_type,
            event.user_id,
            event.details,
            event.timestamp
        )
        .execute(&self.db)
        .await
        .map_err(AppError::Database)?;

        let mut conn = self.redis.get_async_connection().await.map_err(AppError::Redis)?;
        let event_json = serde_json::to_string(&event).map_err(AppError::Serialization)?;
        conn.lpush("audit_queue", event_json).await.map_err(AppError::Redis)?;

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
        let mut query = String::from(
            r#"SELECT event_type, user_id, details, timestamp FROM audit_logs WHERE 1=1"#
        );
        let mut params = Vec::new();
        let mut param_index = 1;
        
        if let Some(event_type) = event_type {
            query.push_str(&format!(" AND event_type = ${}::TEXT", param_index));
            params.push(event_type);
            param_index += 1;
        }
        
        if let Some(user_id) = user_id {
            query.push_str(&format!(" AND user_id = ${}", param_index));
            params.push(user_id);
            param_index += 1;
        }
        
        if let Some(start_time) = start_time {
            query.push_str(&format!(" AND timestamp >= ${}", param_index));
            params.push(start_time);
            param_index += 1;
        }
        
        if let Some(end_time) = end_time {
            query.push_str(&format!(" AND timestamp <= ${}", param_index));
            params.push(end_time);
            param_index += 1;
        }
        
        query.push_str(" ORDER BY timestamp DESC");
        
        if let Some(limit) = limit {
            query.push_str(&format!(" LIMIT {}", limit));
        }
        
        let rows = sqlx::query(&query)
            .bind_all(params)
            .fetch_all(&self.db)
            .await
            .map_err(|e| AppError::db(e))?;
        
        let mut results = Vec::new();
        for row in rows {
            let event_type_str = row.get::<&str, _>("event_type");
            let event_type = match event_type_str {
                "authentication" => AuditEventType::Authentication,
                "authorization" => AuditEventType::Authorization,
                "data_access" => AuditEventType::DataAccess,
                "configuration_change" => AuditEventType::ConfigurationChange,
                "maintenance" => AuditEventType::Maintenance,
                "security_incident" => AuditEventType::SecurityIncident,
                "api_access" => AuditEventType::ApiAccess,
                s if s.starts_with("custom:") => {
                    let custom_name = s[7..].to_string();
                    AuditEventType::Custom(custom_name)
                }
                _ => continue,
            };
            
            let event = AuditEvent {
                event_type,
                user_id: row.get::<Option<String>, _>("user_id"),
                details: serde_json::from_value(row.get::<serde_json::Value, _>("details"))
                    .map_err(|e| AppError::serialization(e))?,
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
    /// Return the most recent audit events, optionally filtered by event type.
    pub async fn list_events(
        limit: u32,
    ) -> Result<Vec<AuditEventRecord>, AppError> {
        let limit = limit.clamp(1, 200) as i64;

        let rows = if let Some(event_type) = event_type {
            sqlx::query_as::<_, AuditEventRecord>(
                "SELECT id, event_type, user_id, details, timestamp FROM audit_logs
                 WHERE event_type = $1 ORDER BY timestamp DESC LIMIT $2",
            )
            .bind(event_type)
            .bind(limit)
            .await?
        } else {
                 ORDER BY timestamp DESC LIMIT $1",
            )
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
