//! Integration test framework for the crucible backend.
//!
//! Provides shared helpers for spinning up an in-process Axum router and
//! issuing HTTP requests without binding a real TCP socket.

pub mod api_profile_test;
pub mod api_status_test;
pub mod services_test;

use axum::{routing::{get, post}, Router};
use backend::{
    api::handlers::profiling::{AppState, get_system_status, trigger_profile_collection},
    services::{error_recovery::ErrorManager, sys_metrics::MetricsExporter},
};
use std::sync::Arc;

/// Build a test [`Router`] backed by fresh service instances.
pub fn test_app() -> Router {
    let state = Arc::new(AppState {
        metrics_exporter: Arc::new(MetricsExporter::new()),
        error_manager: Arc::new(ErrorManager::new()),
    });

    Router::new()
        .route("/api/status", get(get_system_status))
        .route("/api/profile", post(trigger_profile_collection))
        .with_state(state)
}
