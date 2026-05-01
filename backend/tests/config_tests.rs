use axum::{
    routing::{get, post},
    Router,
};
use std::sync::Arc;
use tower::ServiceExt;
use hyper::{Request, StatusCode};
use backend::config::{AppConfig, reload::{ConfigManager, handle_reload, handle_get_config}};
use backend::api::handlers::profiling::AppState;
use backend::services::{
    sys_metrics::MetricsExporter,
    error_recovery::ErrorManager,
};

#[tokio::test]
async fn test_config_get_endpoint() {
    let config = AppConfig::default();
    let config_manager = Arc::new(ConfigManager::new(config));
    let state = Arc::new(AppState {
        metrics_exporter: Arc::new(MetricsExporter::new()),
        error_manager: Arc::new(ErrorManager::new()),
        config_manager: config_manager.clone(),
    });

    let app = Router::new()
        .route("/api/config", get(handle_get_config))
        .with_state(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/config")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_config_reload_endpoint_no_file() {
    let config = AppConfig::default();
    let config_manager = Arc::new(ConfigManager::new(config));
    let state = Arc::new(AppState {
        metrics_exporter: Arc::new(MetricsExporter::new()),
        error_manager: Arc::new(ErrorManager::new()),
        config_manager: config_manager.clone(),
    });

    let app = Router::new()
        .route("/api/config/reload", post(handle_reload))
        .with_state(state);

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/config/reload")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    // Since config.json doesn't exist, it should return an error
    // In our implementation, ConfigReloadError::Io maps to INTERNAL_SERVER_ERROR
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}

#[tokio::test]
async fn test_config_manager_patch() {
    let config = AppConfig::default();
    let config_manager = ConfigManager::new(config);
    
    let patch = serde_json::json!({
        "log_level": "debug",
        "server": {
            "port": 4000
        }
    });
    
    config_manager.update_from_patch(patch).unwrap();
    
    let updated = config_manager.load();
    assert_eq!(updated.log_level, "debug");
    assert_eq!(updated.server.port, 4000);
    // Ensure other fields are preserved
    assert_eq!(updated.server.host, "0.0.0.0");
}
