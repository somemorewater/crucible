use axum::{
    async_trait,
    extract::{FromRequest, Request, Json},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{error, warn};

// ---------------------------------------------------------------------------
// Error Handling Contract
// ---------------------------------------------------------------------------

/// Standardized API error response.
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiErrorResponse {
    pub error: String,
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

/// Errors that can occur in the API.
#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum ApiError {
    #[error("Validation failed: {0}")]
    Validation(String),

    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("Redis error: {0}")]
    Redis(#[from] redis::RedisError),

    #[error("Internal server error: {0}")]
    Internal(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Unauthorized")]
    Unauthorized,
}

impl ApiError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::Validation(_) => StatusCode::BAD_REQUEST,
            Self::Database(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Redis(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
        }
    }

    pub fn error_code(&self) -> String {
        match self {
            Self::Validation(_) => "VALIDATION_ERROR".to_string(),
            Self::Database(_) => "DATABASE_ERROR".to_string(),
            Self::Redis(_) => "REDIS_ERROR".to_string(),
            Self::Internal(_) => "INTERNAL_ERROR".to_string(),
            Self::NotFound(_) => "NOT_FOUND".to_string(),
            Self::Unauthorized => "UNAUTHORIZED".to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let body = Json(ApiErrorResponse {
            error: self.to_string(),
            code: self.error_code(),
            details: None,
        });

        if status.is_server_error() {
            error!(error = %self, "API Server Error");
        } else {
            warn!(error = %self, "API Client Error");
        }

        (status, body).into_response()
    }
}

// ---------------------------------------------------------------------------
// Success Response Contract
// ---------------------------------------------------------------------------

/// Standardized API success wrapper.
#[derive(Debug, Serialize, Deserialize)]
pub struct ApiResponse<T> {
    pub data: T,
    pub status: String,
}

impl<T> ApiResponse<T> {
    pub fn new(data: T) -> Self {
        Self {
            data,
            status: "success".to_string(),
        }
    }
}

impl<T: Serialize> IntoResponse for ApiResponse<T> {
    fn into_response(self) -> Response {
        (StatusCode::OK, Json(self)).into_response()
    }
}

// ---------------------------------------------------------------------------
// Validation Contract
// ---------------------------------------------------------------------------

/// Trait for types that can be validated.
pub trait Validate {
    /// Perform validation.
    ///
    /// # Errors
    /// Returns a string describing the validation failure.
    fn validate(&self) -> Result<(), String>;
}

/// Axum extractor that performs validation after JSON deserialization.
pub struct ValidatedJson<T>(pub T);

#[async_trait]
impl<S, T> FromRequest<S> for ValidatedJson<T>
where
    T: serde::de::DeserializeOwned + Validate,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) = Json::<T>::from_request(req, state).await
            .map_err(|e: axum::extract::rejection::JsonRejection| ApiError::Validation(e.to_string()))?;
        
        value.validate().map_err(ApiError::Validation)?;
        
        Ok(ValidatedJson(value))
    }
}

// ---------------------------------------------------------------------------
// Domain Contracts
// ---------------------------------------------------------------------------

/// Contract for System Status response.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SystemStatus {
    pub status: String,
    pub uptime_secs: u64,
    pub memory_used_bytes: u64,
    pub active_recovery_tasks: usize,
}

/// Contract for Profiling trigger request.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProfileTriggerRequest {
    pub duration_secs: u32,
    pub sample_rate_hz: u32,
    pub label: String,
}

impl Validate for ProfileTriggerRequest {
    fn validate(&self) -> Result<(), String> {
        if self.duration_secs == 0 || self.duration_secs > 3600 {
            return Err("duration_secs must be between 1 and 3600".to_string());
        }
        if self.sample_rate_hz == 0 || self.sample_rate_hz > 1000 {
            return Err("sample_rate_hz must be between 1 and 1000".to_string());
        }
        if self.label.is_empty() {
            return Err("label cannot be empty".to_string());
        }
        Ok(())
    }
}

/// Contract for Profiling trigger response.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ProfileTriggerResponse {
    pub profile_id: uuid::Uuid,
    pub message: String,
    pub estimated_completion: chrono::DateTime<chrono::Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validation_success() {
        let req = ProfileTriggerRequest {
            duration_secs: 60,
            sample_rate_hz: 10,
            label: "test-run".to_string(),
        };
        assert!(req.validate().is_ok());
    }

    #[test]
    fn test_validation_failure_duration() {
        let req = ProfileTriggerRequest {
            duration_secs: 0,
            sample_rate_hz: 10,
            label: "test-run".to_string(),
        };
        assert!(req.validate().is_err());
    }

    #[test]
    fn test_validation_failure_label() {
        let req = ProfileTriggerRequest {
            duration_secs: 60,
            sample_rate_hz: 10,
            label: "".to_string(),
        };
        assert!(req.validate().is_err());
    }

    #[test]
    fn test_api_response_serialization() {
        let resp = ApiResponse::new("test data");
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"data\":\"test data\""));
        assert!(json.contains("\"status\":\"success\""));
    }

    #[test]
    fn test_api_error_status_codes() {
        let err = ApiError::Validation("invalid".to_string());
        assert_eq!(err.status_code(), StatusCode::BAD_REQUEST);
        
        let err = ApiError::Internal("oops".to_string());
        assert_eq!(err.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
