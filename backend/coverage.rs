use axum::{
    extract::{Path, State},
    Json,
    http::StatusCode,
    response::IntoResponse,
};
use crate::services::test_coverage::{NewTestCoverage, TestCoverageService, CoverageError};

pub async fn submit_coverage(
    State(service): State<TestCoverageService>,
    Json(payload): Json<NewTestCoverage>,
) -> impl IntoResponse {
    match service.submit_coverage(payload).await {
        Ok(report) => (StatusCode::CREATED, Json(report)).into_response(),
        Err(e) => map_error(e),
    }
}

pub async fn get_latest_coverage(
    State(service): State<TestCoverageService>,
    Path(project): Path<String>,
) -> impl IntoResponse {
    match service.get_latest_coverage(&project).await {
        Ok(report) => (StatusCode::OK, Json(report)).into_response(),
        Err(e) => map_error(e),
    }
}

fn map_error(err: CoverageError) -> axum::response::Response {
    match err {
        CoverageError::NotFound(_) => (StatusCode::NOT_FOUND, err.to_string()).into_response(),
        CoverageError::Database(_) | CoverageError::Redis(_) => {
            tracing::error!(error = %err, "Internal service error");
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal server error").into_response()
        }
        CoverageError::Serialization(_) => {
            (StatusCode::UNPROCESSABLE_ENTITY, "Data format error").into_response()
        }
    }
}