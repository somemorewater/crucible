use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tower::ServiceExt;
use hyper::{Request, StatusCode};
use serde_json::json;
use backend::api::handlers::profiling::{AppState, get_system_status, trigger_profile_collection};
use backend::services::{sys_metrics::MetricsExporter, error_recovery::ErrorManager};
use backend::config::reload::ConfigManager;
use backend::config::AppConfig;

#[tokio::test]
async fn test_system_status_contract() {
    let state = Arc::new(AppState {
        metrics_exporter: Arc::new(MetricsExporter::new()),
        error_manager: Arc::new(ErrorManager::new()),
        config_manager: Arc::new(ConfigManager::new(AppConfig::default())),
    });

    let app = Router::new()
        .route("/api/status", get(get_system_status))
        .with_state(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/status")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    
    assert_eq!(json["status"], "success");
    assert!(json["data"]["status"].is_string());
    assert!(json["data"]["uptime_secs"].is_number());
}

#[tokio::test]
async fn test_profile_trigger_validation_success() {
    let state = Arc::new(AppState {
        metrics_exporter: Arc::new(MetricsExporter::new()),
        error_manager: Arc::new(ErrorManager::new()),
        config_manager: Arc::new(ConfigManager::new(AppConfig::default())),
    });

    let app = Router::new()
        .route("/api/profile", post(trigger_profile_collection))
        .with_state(state);

    let payload = json!({
        "duration_secs": 30,
        "sample_rate_hz": 100,
        "label": "load-test"
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/profile")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_profile_trigger_validation_failure() {
    let state = Arc::new(AppState {
        metrics_exporter: Arc::new(MetricsExporter::new()),
        error_manager: Arc::new(ErrorManager::new()),
        config_manager: Arc::new(ConfigManager::new(AppConfig::default())),
    });

    let app = Router::new()
        .route("/api/profile", post(trigger_profile_collection))
        .with_state(state);

    let payload = json!({
        "duration_secs": 0, // Invalid: must be > 0
        "sample_rate_hz": 100,
        "label": "" // Invalid: cannot be empty
    });

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/profile")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(serde_json::to_vec(&payload).unwrap()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    
    let body = axum::body::to_bytes(response.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    
    assert_eq!(json["code"], "VALIDATION_ERROR");
    assert!(json["error"].as_str().unwrap().contains("Validation failed"));
}
