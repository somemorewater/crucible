//! Concurrent load tests for the `POST /api/profile` endpoint.

use axum::{routing::post, Router};
use hyper::{Request, StatusCode};
use std::sync::Arc;
use tower::ServiceExt;

use backend::api::handlers::profiling::{trigger_profile_collection, AppState};
use backend::services::{error_recovery::ErrorManager, sys_metrics::MetricsExporter};
use backend::config::{AppConfig, reload::ConfigManager};

fn build_app() -> Router {
    let state = Arc::new(AppState {
        db: None,
        metrics_exporter: Arc::new(MetricsExporter::new()),
        error_manager: Arc::new(ErrorManager::new()),
        config_manager: Arc::new(ConfigManager::new(AppConfig::default())),
    });
    Router::new()
        .route("/api/profile", post(trigger_profile_collection))
        .with_state(state)
}

async fn run_concurrent(n: usize) {
    let handles: Vec<_> = (0..n)
        .map(|_| {
            let app = build_app();
            tokio::spawn(async move {
                let resp = app
                    .oneshot(
                        Request::builder()
                            .method("POST")
                            .uri("/api/profile")
                            .header("content-type", "application/json")
                            .body(axum::body::Body::from(serde_json::json!({
                                "duration_secs": 10,
                                "sample_rate_hz": 100,
                                "label": "load-test"
                            }).to_string()))
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
async fn test_profile_10_concurrent() {
    run_concurrent(10).await;
}

#[tokio::test]
async fn test_profile_50_concurrent() {
    run_concurrent(50).await;
}

/// Verify each response contains a unique profile_id.
#[tokio::test]
async fn test_profile_unique_ids() {
    use axum::body::to_bytes;
    use std::collections::HashSet;

    let mut ids = HashSet::new();
    for _ in 0..10 {
        let app = build_app();
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/profile")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(serde_json::json!({
                        "duration_secs": 10,
                        "sample_rate_hz": 100,
                        "label": "load-test-id"
                    }).to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();

        let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).expect("Valid JSON");
        let id = json["data"]["profile_id"].as_str().expect("profile_id in data").to_string();
        ids.insert(id);
    }

    // All 10 profile IDs should be unique
    assert_eq!(ids.len(), 10);
}

/// Verify response body shape.
#[tokio::test]
async fn test_profile_response_shape() {
    use axum::body::to_bytes;

    let app = build_app();
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/profile")
                .header("content-type", "application/json")
                .body(axum::body::Body::from(serde_json::json!({
                    "duration_secs": 10,
                    "sample_rate_hz": 100,
                    "label": "load-test-shape"
                }).to_string()))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);

    let bytes = to_bytes(resp.into_body(), usize::MAX).await.unwrap();
    let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert!(json.get("data").is_some());
    assert!(json["data"].get("message").is_some());
    assert!(json["data"].get("profile_id").is_some());
}
