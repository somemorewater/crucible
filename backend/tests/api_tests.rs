use axum::{
    body::Body,
    http::{Request, StatusCode},
    response::IntoResponse,
    routing::get,
    Router,
};
use backend::api::handlers::profiling::{get_system_status, AppState};
use backend::services::{error_recovery::ErrorManager, sys_metrics::MetricsExporter};
use std::sync::Arc;
use tower::ServiceExt;

#[tokio::test]
async fn test_health_check_integration() {
    // Placeholder — full integration test requires a live DB.
}

#[tokio::test]
async fn test_stellar_toml_headers() {
    use backend::api::handlers::stellar::get_stellar_toml;
    let response = get_stellar_toml().await.into_response();

    assert_eq!(response.status(), StatusCode::OK);
    let cors = response
        .headers()
        .get("access-control-allow-origin")
        .unwrap();
    assert_eq!(cors, "*");
}

    let config_manager = Arc::new(backend::config::reload::ConfigManager::new(backend::config::AppConfig::default()));
    let state = Arc::new(AppState {
        metrics_exporter,
        error_manager,
        config_manager,
#[tokio::test]
async fn test_get_status_endpoint() {
    let state = Arc::new(AppState {
        db: None,
        metrics_exporter: Arc::new(MetricsExporter::new()),
        error_manager: Arc::new(ErrorManager::new()),
    });

    let app = Router::new()
        .route("/api/status", get(get_system_status))
        .with_state(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}
