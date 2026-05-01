//! Concurrent load tests for the `GET /api/status` endpoint.

use axum::{routing::get, Router};
use hyper::{Request, StatusCode};
use std::sync::Arc;
use tower::ServiceExt;

use backend::api::handlers::profiling::{get_system_status, AppState};
use backend::services::{error_recovery::ErrorManager, sys_metrics::MetricsExporter};
use backend::config::{AppConfig, reload::ConfigManager};

/// Build a test router with the status endpoint.
fn build_app() -> Router {
    let state = Arc::new(AppState {
        db: None,
        metrics_exporter: Arc::new(MetricsExporter::new()),
        error_manager: Arc::new(ErrorManager::new()),
        config_manager: Arc::new(ConfigManager::new(AppConfig::default())),
    });
    Router::new()
        .route("/api/status", get(get_system_status))
        .with_state(state)
}

/// Fire `n` concurrent requests and assert all return 200.
async fn run_concurrent(n: usize) {
    let handles: Vec<_> = (0..n)
        .map(|_| {
            let app = build_app();
            tokio::spawn(async move {
                let resp = app
                    .oneshot(
                        Request::builder()
                            .uri("/api/status")
                            .body(axum::body::Body::empty())
                            .unwrap(),
                    )
                    .await
                    .unwrap();
                resp.status()
            })
        })
        .collect();

    for handle in handles {
        let status = handle.await.unwrap();
        assert_eq!(status, StatusCode::OK);
    }
}

#[tokio::test]
async fn test_status_10_concurrent() {
    run_concurrent(10).await;
}

#[tokio::test]
async fn test_status_50_concurrent() {
    run_concurrent(50).await;
}

#[tokio::test]
async fn test_status_100_concurrent() {
    run_concurrent(100).await;
}

/// Verify that repeated sequential requests all succeed.
#[tokio::test]
async fn test_status_sequential_stability() {
    let app = build_app();
    for _ in 0..20 {
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/status")
                    .body(axum::body::Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}

/// Verify response body contains expected JSON keys.
#[tokio::test]
async fn test_status_response_shape() {
    use axum::body::to_bytes;

    let app = build_app();
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/status")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(json["status"], "success");
    assert!(json.get("data").is_some());
    assert!(json["data"].get("status").is_some());
    assert!(json["data"].get("uptime_secs").is_some());
    assert!(json["data"].get("active_recovery_tasks").is_some());
}
