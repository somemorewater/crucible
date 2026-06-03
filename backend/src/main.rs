use apalis::prelude::*;
use apalis_redis::RedisStorage;
use axum::{
    middleware,
    routing::{get, post},
    Router,
    middleware,
};
use backend::api::handlers::dashboard::{get_dashboard, DashboardState, get_dashboard_metrics, get_contract_stats};
use backend::{
    api::handlers::{profiling, stellar, dashboard},
    api::middleware::logging::logging_middleware,
    config::{Config, AppConfig, reload::{ConfigManager, handle_reload, handle_get_config}},
    jobs::{monitor_transaction, TransactionMonitorJob},
    services::{
        error_recovery::ErrorManager, log_aggregator::LogAggregator, log_alerts::AlertManager,
        audit::AuditService,
        error_recovery::ErrorManager,
        log_aggregator::LogAggregator,
        log_alerts::AlertManager,
        sys_metrics::MetricsExporter,
        tracing::{TracingService, TracingConfig},
    },
};
use backend::services::audit;
use profiling::AppState;
use redis::aio::ConnectionManager;
use sqlx::postgres::PgPoolOptions;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::signal;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;
use tracing::info_span;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    // Load layered configuration
    let env = Environment::from_env();
    let config = AppConfig::load(env).expect("Failed to load configuration");

    // Initialize observability using the new config system
    config.observability.init_tracing(env);

    // Initialize OpenTelemetry tracing FIRST - before any other services
    let tracing_config = TracingConfig::new(
        "crucible-backend".to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
    )
    .with_environment(env.as_str().to_string())
    .with_otlp_endpoint(
        config
            .observability
            .tracing_endpoint
            .clone()
            .unwrap_or_else(|| "http://localhost:4317".to_string()),
    );

    TracingService::init(tracing_config)?;

    let span = info_span!("app.startup");
    let _enter = span.enter();

    // Database setup & migrations
    let db_span = TracingService::db_query_span("CONNECT postgresql", "postgres", "CONNECT");
    let _db_enter = db_span.enter();

    let db_pool = config
        .database
        .to_sqlx_pool_options()
        .connect(&config.database.url)
        .await?;

    tracing::info!("Database connection established");
    drop(_db_enter);

    let redis_client = redis::Client::open(config.redis_url.clone())?;

    // Initialize services
    let metrics_exporter = Arc::new(MetricsExporter::new());
    let error_manager = Arc::new(ErrorManager::new());
    let alert_manager = Arc::new(AlertManager::new());
    let (log_aggregator, log_receiver) = LogAggregator::new();
    let log_aggregator = Arc::new(log_aggregator);

    tokio::spawn(MetricsExporter::run_collector(metrics_exporter.clone()));
    tokio::spawn(LogAggregator::run_worker(log_receiver));

    // Initialize config manager
    let app_config = AppConfig::default();
    let config_manager = Arc::new(ConfigManager::new(app_config));

    // Redis + job queue setup
    let conn = ConnectionManager::new(redis_client.clone()).await?;
    let redis_span = TracingService::redis_command_span("CONNECT", None);
    let _redis_enter = redis_span.enter();
    
    let redis_conn_dashboard = ConnectionManager::new(redis_client.clone()).await?;
    let storage: RedisStorage<TransactionMonitorJob> = RedisStorage::new(conn);

    tracing::info!("Redis connection established");
    drop(_redis_enter);

    let worker = WorkerBuilder::new("monitor-worker")
        .backend(storage)
        .build_fn(monitor_transaction);

    // Shared state for profiling/status routes
    let profiling_state = Arc::new(AppState {
        db: Some(db_pool.clone()),
        metrics_exporter: metrics_exporter.clone(),
        error_manager: error_manager.clone(),
        config_manager: config_manager.clone(),
        log_aggregator: log_aggregator.clone(),
        redis: redis_client.clone(),
    });

    // Shared state for dashboard routes
    let dashboard_state = Arc::new(DashboardState {
        db: db_pool,
        redis_conn: redis_conn_dashboard,
        metrics_exporter,
        error_manager,
        alert_manager,
        redis_client,
        log_aggregator,
        redis: redis_client.clone(),
        db: db_pool.clone(),
        redis_conn: redis_conn_dashboard, // Depending on what DashboardState actually expects
    });

    let audit_service = Arc::new(AuditService::new(db_pool.clone(), Arc::new(redis_client.clone())));

    // OpenAPI docs
    #[derive(OpenApi)]
    #[openapi(
        paths(
            profiling::get_metrics,
            profiling::get_health,
            dashboard::get_dashboard_metrics,
            dashboard::get_contract_stats,
            audit::list_audit_reports,
            audit::get_audit_report,
        ),
        components(
            schemas(
                profiling::MetricsReport,
                profiling::HealthResponse,
                dashboard::DashboardMetrics,
                dashboard::ContractStats
            )
        ),
        components(schemas(
            profiling::MetricsReport,
            profiling::HealthResponse,
            dashboard::DashboardMetrics,
            dashboard::ContractStats,
            audit::AuditEventRecord,
            audit::AuditEventRequest,
        )),
        tags(
            (name = "profiling", description = "Performance and health monitoring endpoints"),
            (name = "dashboard", description = "Dashboard metrics and analytics endpoints")
        )
    )]
    struct ApiDoc;

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        .route("/", get(|| async { "Crucible Backend API" }))
        .route("/.well-known/stellar.toml", get(stellar::get_stellar_toml))
        .merge(
            Router::new()
                .route("/api/config", get(handle_get_config))
                .route("/api/config/reload", post(handle_reload))
                .with_state(config_manager.clone()),
        )
        .nest(
            "/api/v1/profiling",
            Router::new()
                .route("/metrics", get(profiling::get_metrics))
                .route("/health", get(profiling::get_health))
                .route("/prometheus", get(profiling::get_prometheus_metrics))
                .route("/status", get(profiling::get_system_status))
                .route("/profile", post(profiling::trigger_profile_collection))
                .with_state(state.clone()),
        )
        .route("/api/status", get(profiling::get_system_status))
        .route("/api/profile", post(profiling::trigger_profile_collection))
        .route("/api/config", get(handle_get_config))
        .route("/api/config/reload", post(handle_reload))
        .with_state(profiling_state.clone())
        .nest(
            "/api/v1/dashboard",
            Router::new()
                .route("/metrics", get(get_dashboard_metrics))
                .route("/contracts/:contract_id/stats", get(get_contract_stats)),
                .route("/", get(get_dashboard))
                .route("/metrics", get(dashboard::get_dashboard_metrics))
                .route("/contracts/:contract_id/stats", get(dashboard::get_contract_stats))
                .with_state(dashboard_state),
        )
        .nest(
            "/api/v1/audit",
            audit::routes(audit_service.clone()),
        )
            "/api/v1/errors",
            errors::error_analytics_routes(db_pool.clone(), redis_conn_dashboard.clone())
        )
        .route("/api/dashboard", get(get_dashboard))
        .with_state(dashboard_state)
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .layer(middleware::from_fn_with_state(profiling_state.clone(), logging_middleware))
        .layer(TraceLayer::new_for_http())
        .layer(cors);

    let addr: SocketAddr = format!("{}:{}", config.server.host, config.server.port).parse()?;

    let listener = tokio::net::TcpListener::bind(&addr).await?;

    // Graceful shutdown handling
    let shutdown_timeout = std::time::Duration::from_secs(
        std::env::var("SHUTDOWN_TIMEOUT_SECS")
            .unwrap_or_else(|_| "30".into())
            .parse()
            .unwrap_or(30),
    );

    let server = axum::serve(listener, app);

    let result = tokio::select! {
        res = server.with_graceful_shutdown(shutdown_signal()) => {
            tracing::info!("Signal received, stopping acceptance of new requests");

            // Wait for server to finish shutting down (stops accepting new connections)
            match res {
                Ok(()) => tracing::info!("Server stopped accepting new connections"),
                Err(ref e) => tracing::error!("Server error during shutdown: {e}"),
            }

            // Wait for in-flight requests to complete
            tracing::info!("Waiting for in-flight requests to complete (timeout: {shutdown_timeout_secs}s)",
                         shutdown_timeout_secs = shutdown_timeout.as_secs());
            match tokio::time::timeout(shutdown_timeout, async {
                // Give time for existing requests to complete
                // Note: Axum's with_graceful_shutdown already waits for connections to close,
                // but we add an additional timeout as safety
                tokio::time::sleep(shutdown_timeout).await;
            }).await {
                Ok(()) => tracing::info!("In-flight requests completed"),
                Err(_) => tracing::warn!("Timeout waiting for in-flight requests to complete"),
            }

            // Flush tracing and logging
            tracing::info!("Flushing tracing and logging subscribers");
            // In practice, we'd use a tracing subscriber guard to flush
            // For now, we note that tracing is flushed when the subscriber is dropped
            // which happens naturally at the end of the program

            // Close database connection pool
            tracing::info!("Closing database connection pool");
            drop(state.db.clone()); // Drop this handle; other shared handles close when released.

            // Close Redis connection
            tracing::info!("Closing Redis connection");
            drop(state.redis.clone()); // Drop this handle; other shared handles close when released.

            tracing::info!("Graceful shutdown completed successfully");

            res
        },
        _ = worker.run() => {
            tracing::info!("Worker stopped");
            Ok(())
        }
    };

    // Handle the result from either branch
    if let Err(e) = &result {
        tracing::error!("Application error: {e}");
    }

    result?;

    Ok(())
}

/// Listens for shutdown signals (SIGTERM, SIGINT, Ctrl+C).
///
/// This function waits for either a SIGTERM signal (on Unix platforms) or
/// a Ctrl+C signal, then returns when one is received. It is used with
/// Axum's `with_graceful_shutdown` method to initiate graceful shutdown
/// of the HTTP server.
///
/// # Returns
///
/// This function resolves when a shutdown signal is received.
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("Received Ctrl+C, initiating graceful shutdown");
        },
        _ = terminate => {
            tracing::info!("Received SIGTERM, initiating graceful shutdown");
        },
    }
}
