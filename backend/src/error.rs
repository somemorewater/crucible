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
use serde_json::json;
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
/// # Examples
/// ```rust,no_run
/// use backend::error::AppError;
/// async fn handler() -> Result<String, AppError> {
///     Err(AppError::NotFound("Contract not found".into()))
/// }
/// ```
#[derive(Debug, Error)]
pub enum AppError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] sqlx::Error),

    /// 500 — An internal Redis error occurred.
    #[error("Redis error: {0}")]
    RedisError(#[from] redis::RedisError),

    #[error("Internal server error: {0}")]
    Internal(String),

    /// 502 — Stellar network communication failure.
    /// 500 — A catch-all for unexpected internal errors.
    #[error("Internal error: {0}")]
    InternalError(String),

    /// 502 — A Stellar network operation failed.
    /// 404  The requested resource was not found.
    #[error("Not found: {0}")]
    NotFound(String),

    /// 400  The request was malformed or contained invalid data.
    #[error("Bad request: {0}")]
    BadRequest(String),

    /// 401  Authentication is required or failed.
    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    /// 403  The authenticated user lacks permission.
    #[error("Forbidden: {0}")]
    Forbidden(String),

    /// 409  The request conflicts with the current state.
    #[error("Conflict: {0}")]
    Conflict(String),

    /// 422  The request body was well-formed but semantically invalid.
    #[error("Validation error: {0}")]
    ValidationError(String),

    /// 500  An internal database error occurred.
    Database(#[from] sqlx::Error),

    /// 500  An internal Redis error occurred.
    Redis(#[from] redis::RedisError),

    /// 500  A serialization error occurred.
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// 500  A catch-all for unexpected internal errors.

    /// 502  External Stellar integration failed.
    #[error("Stellar operation failed: {0}")]
    StellarError(String),
}

    #[error("Validation error: {0}")]
    ValidationError(String),

    #[error("Invalid request: {0}")]
    BadRequest(String),

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
        let (status, message) = match self {
            AppError::Database(ref e) => {
                error!("Database error occurred: {:?}", e);
        let (status, code, message) = match &self {
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, "not_found", msg.clone()),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, "bad_request", msg.clone()),
            AppError::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, "unauthorized", msg.clone()),
            AppError::Forbidden(msg) => (StatusCode::FORBIDDEN, "forbidden", msg.clone()),
            AppError::Conflict(msg) => (StatusCode::CONFLICT, "conflict", msg.clone()),
            AppError::ValidationError(msg) => {
                (StatusCode::UNPROCESSABLE_ENTITY, "validation_error", msg.clone())
            }
            AppError::DatabaseError(e) => {
                error!("Database error: {e:?}");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "A database error occurred".to_string(),
                )
            }
            AppError::Redis(ref e) => {
                error!("Redis error occurred: {:?}", e);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "A cache error occurred".to_string(),
                )
            }
            AppError::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            AppError::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            AppError::Unauthorized => (StatusCode::UNAUTHORIZED, "Unauthorized access".to_string()),
            AppError::RedisError(e) => {
                error!("Redis error: {e:?}");
                tracing::error!("Database error: {e:?}");
            AppError::ValidationError(msg) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "validation_error",
                msg.clone(),
            ),
            AppError::Database(e) => {
                error!(error = ?e, "Database error");
                    "database_error",
                    "An internal database error occurred".to_string(),
                )
            }
                tracing::error!("Redis error: {e:?}");
                    "redis_error",
                    "An internal cache error occurred".to_string(),
                )
            }
            AppError::Serialization(e) => {
                error!("Serialization error: {e:?}");
            AppError::Redis(e) => {
                error!(error = ?e, "Redis error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "A cache error occurred".to_string(),
                )
            }
                error!(error = ?e, "Serialization error");
                    "serialization_error",
                    "A serialization error occurred".to_string(),
                )
            }
            AppError::InternalError(msg) => {
                error!("Internal error: {msg}");
                tracing::error!("Internal error: {msg}");
                error!(error = %msg, "Internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "An internal error occurred".to_string(),
                )
            }
            AppError::StellarError(msg) => {
                error!("Stellar error: {}", msg);
                (
                    StatusCode::BAD_GATEWAY,
                    "Failed to communicate with Stellar network".to_string(),
                )
            }
            _ => {
                error!("Internal error: {:?}", self);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "An internal server error occurred".to_string(),
                )
            }
        };

        let body = Json(json!({
            "error": message,
            "code": status.as_u16(),
        }));

        (status, body).into_response()
                tracing::error!("Stellar error: {msg}");
                    "stellar_error",
                error!(error = %msg, "Stellar integration error");
                    "A Stellar network error occurred".to_string(),
                )
            }
        };

        (
            status,
            Json(ErrorResponse {
                code: code.to_string(),
                message,
            }),
        )
            .into_response()
        let body = Json(ErrorResponse { code: code.to_string(), message });
        (status, body).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_not_found_display() {
    fn test_not_found_error_display() {
        let err = AppError::NotFound("Contract not found".into());
        assert_eq!(err.to_string(), "Not found: Contract not found");
    }

    fn test_bad_request_display() {
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
