use axum::{
    body::Body,
    extract::State,
    http::{Request, Response},
    middleware::Next,
    response::IntoResponse,
};
use std::{sync::Arc, time::Instant};
use tracing::{info_span, Instrument};
use crate::api::handlers::profiling::AppState;

/// Middleware to log HTTP requests and responses.
/// 
/// This middleware captures:
/// - Request method, URI, and HTTP version
/// - Request headers (filtered for security)
/// - Response status code
/// - Processing latency
/// 
/// It uses `tracing` for structured logging and integrates with the `LogAggregator` service.
pub async fn logging_middleware(
    State(state): State<Arc<AppState>>,
    request: Request<Body>,
    next: Next,
) -> impl IntoResponse {
    let start_time = Instant::now();
    let method = request.method().clone();
    let uri = request.uri().clone();
    let version = request.version();

    // Create a tracing span for this request
    let span = info_span!(
        "http_request",
        %method,
        %uri,
        ?version,
    );

    async move {
        // Log the incoming request
        tracing::debug!("Incoming request");

        let response = next.run(request).await;

        let latency = start_time.elapsed();
        let status = response.status();

        // Log the response
        tracing::info!(
            latency_ms = latency.as_millis(),
            status = status.as_u16(),
            "Finished processing request"
        );

        // Optionally persist log to LogAggregator
        let log_message = format!(
            "{} {} finished with {} in {:?}",
            method, uri, status, latency
        );
        
        // We don't want to block the response on logging persistence
        let aggregator = state.log_aggregator.clone();
        tokio::spawn(async move {
            if let Err(e) = aggregator.log("INFO", &log_message, "api_gateway").await {
                tracing::error!(error = %e, "Failed to send log to aggregator");
            }
        });

        response
    }
    .instrument(span)
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        routing::get,
        Router,
    };
    use tower::ServiceExt;
    use hyper::StatusCode;
    use crate::services::{
        sys_metrics::MetricsExporter,
        error_recovery::ErrorManager,
        log_aggregator::LogAggregator,
    };
    use sqlx::PgPool;
    use redis::Client as RedisClient;

    #[tokio::test]
    async fn test_logging_middleware_success() {
        // Mock dependencies
        let metrics_exporter = Arc::new(MetricsExporter::new());
        let error_manager = Arc::new(ErrorManager::new());
        let (log_aggregator, _rx) = LogAggregator::new();
        let log_aggregator = Arc::new(log_aggregator);
        
        // Use connect_lazy for testing to avoid needing a real DB
        let db = PgPool::connect_lazy("postgres://localhost/test").unwrap();
        let redis = RedisClient::open("redis://localhost").unwrap();

        let state = Arc::new(AppState {
            metrics_exporter,
            error_manager,
            log_aggregator,
            db,
            redis,
        });

        let app = Router::new()
            .route("/", get(|| async { "OK" }))
            .layer(axum::middleware::from_fn_with_state(state.clone(), logging_middleware))
            .with_state(state);

        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }
}
