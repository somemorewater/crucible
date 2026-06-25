//! Custom error types for the Crucible backend.
//!
//! Provides a unified [`AppError`] type that maps internal errors into
//! appropriate HTTP status codes and JSON error responses.

use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use thiserror::Error;
use tracing::error;

/// Structured error response returned to API clients.
#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    /// Machine-readable error code (e.g., `"database_error"`, `"not_found"`).
    pub code: String,
    /// Human-readable error message.
    pub message: String,
}

/// Application-level error type that unifies all possible error sources.
///
/// Each variant maps to an HTTP status code and produces a consistent
/// JSON error response via the [`IntoResponse`] implementation.
#[derive(Debug, Error)]
pub enum AppError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Database error: {0}")]
    DatabaseError(sqlx::Error),

    #[error("Redis error: {0}")]
    Redis(#[from] redis::RedisError),

    #[error("Redis error: {0}")]
    RedisError(redis::RedisError),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Internal error: {0}")]
    Internal(String),

    #[error("Internal error: {0}")]
    InternalError(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Forbidden: {0}")]
    Forbidden(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Validation error: {0}")]
    ValidationError(String),

    #[error("Stellar operation failed: {0}")]
    StellarError(String),
}

impl AppError {
    /// Wrap a Database error.
    pub fn db(e: sqlx::Error) -> Self {
        AppError::Database(e)
    }

    /// Wrap a Redis error.
    pub fn redis(e: redis::RedisError) -> Self {
        AppError::Redis(e)
    }

    /// Wrap a serialization error.
    pub fn serialization(e: serde_json::Error) -> Self {
        AppError::Serialization(e)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code, message) = match &self {
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, "not_found", msg.clone()),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, "bad_request", msg.clone()),
            AppError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, "unauthorized", msg.clone()),
            AppError::Forbidden(msg) => (StatusCode::FORBIDDEN, "forbidden", msg.clone()),
            AppError::Conflict(msg) => (StatusCode::CONFLICT, "conflict", msg.clone()),
            AppError::ValidationError(msg) => (StatusCode::UNPROCESSABLE_ENTITY, "validation_error", msg.clone()),
            AppError::Database(e) => {
                error!("Database error: {e:?}");
                (StatusCode::INTERNAL_SERVER_ERROR, "database_error", "An internal database error occurred".to_string())
            }
            AppError::DatabaseError(e) => {
                error!("Database error: {e:?}");
                (StatusCode::INTERNAL_SERVER_ERROR, "database_error", "An internal database error occurred".to_string())
            }
            AppError::Redis(e) => {
                error!("Redis error: {e:?}");
                (StatusCode::INTERNAL_SERVER_ERROR, "redis_error", "An internal cache error occurred".to_string())
            }
            AppError::RedisError(e) => {
                error!("Redis error: {e:?}");
                (StatusCode::INTERNAL_SERVER_ERROR, "redis_error", "An internal cache error occurred".to_string())
            }
            AppError::Serialization(e) => {
                error!("Serialization error: {e:?}");
                (StatusCode::INTERNAL_SERVER_ERROR, "serialization_error", "A serialization error occurred".to_string())
            }
            AppError::Internal(msg) => {
                error!("Internal error: {msg}");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal_error", msg.clone())
            }
            AppError::InternalError(msg) => {
                error!("Internal error: {msg}");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal_error", msg.clone())
            }
            AppError::StellarError(msg) => {
                error!("Stellar error: {msg}");
                (StatusCode::BAD_GATEWAY, "stellar_error", msg.clone())
            }
        };

        let body = Json(ErrorResponse {
            code: code.to_string(),
            message,
        });

        (status, body).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_not_found_error_display() {
        let err = AppError::NotFound("Contract not found".into());
        assert_eq!(err.to_string(), "Not found: Contract not found");
    }

    #[test]
    fn test_bad_request_error_display() {
        let err = AppError::BadRequest("Invalid address format".into());
        assert_eq!(err.to_string(), "Bad request: Invalid address format");
    }

    #[test]
    fn test_validation_error_display() {
        let err = AppError::ValidationError("name is required".into());
        assert_eq!(err.to_string(), "Validation error: name is required");
    }

    #[test]
    fn test_internal_error_display() {
        let err = AppError::InternalError("unexpected state".into());
        assert_eq!(err.to_string(), "Internal error: unexpected state");
    }

    #[test]
    fn test_error_response_serialization() {
        let resp = ErrorResponse {
            code: "not_found".into(),
            message: "Resource not found".into(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"code\":\"not_found\""));
        assert!(json.contains("\"message\":\"Resource not found\""));
    }
}
