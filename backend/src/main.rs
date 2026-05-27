use apalis::prelude::*;
use apalis_redis::RedisStorage;
use axum::{
    middleware,
    routing::{get, post},
    Router,
};
use backend::api::handlers::dashboard::{get_dashboard, DashboardState};
use backend::{
    api::handlers::{dashboard, profiling, stellar},
    api::middleware::logging::logging_middleware,
    config::{AppConfig, Environment, reload::{ConfigManager, handle_reload, handle_get_config}},
    jobs::{monitor_transaction, TransactionMonitorJob},
    services::{
        error_recovery::ErrorManager,
        log_aggregator::LogAggregator,
        log_alerts::AlertManager,
        sys_metrics::MetricsExporter,
        tracing::{TracingConfig, TracingService},
    },
};
use profiling::AppState;
use redis::aio::ConnectionManager;
use redis::Client as RedisClient;
use sqlx::postgres::PgPoolOptions;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::signal;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing::info_span;
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;

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
        config.observability.tracing_endpoint
            .clone()
            .unwrap_or_else(|| "http://localhost:4317".to_string()),
    );

    TracingService::init(tracing_config)?;

    let span = info_span!("app.startup");
    let _enter = span.enter();

    // Database setup & migrations
    let db_span = TracingService::db_query_span("CONNECT postgresql", "postgres", "CONNECT");
    let _db_enter = db_span.enter();

    let db_pool = config.database.to_sqlx_pool_options()
        .connect(&config.database.url)
        .await?;

    tracing::info!("Database connection established and pool initialized");
    drop(_db_enter);

    let redis_client = RedisClient::open(config.redis.url.clone())?;

    // Initialize services
    let metrics_exporter = Arc::new(MetricsExporter::new());
    let error_manager = Arc::new(ErrorManager::new());
    let alert_manager = Arc::new(AlertManager::new());
    let (log_aggregator, log_receiver) = LogAggregator::new();
    let log_aggregator = Arc::new(log_aggregator);

    tokio::spawn(MetricsExporter::run_collector(metrics_exporter.clone()));
    tokio::spawn(LogAggregator::run_worker(log_receiver));

    // Initialize config manager
    let config_manager = Arc::new(ConfigManager::new(config.clone()));

    // Redis Job Queue setup
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

    // Create shared state
    let state = Arc::new(AppState {
        db: Some(db_pool.clone()),
        metrics_exporter: metrics_exporter.clone(),
        error_manager: error_manager.clone(),
        config_manager: config_manager.clone(),
    });

    // Create dashboard state
    let dashboard_state = Arc::new(DashboardState {
        metrics_exporter,
        error_manager,
        config_manager: config_manager.clone(),
        alert_manager,
        log_aggregator,
        redis: redis_client,
        db: db_pool,
        redis_conn: redis_conn_dashboard, // Depending on what DashboardState actually expects
    });

    // OpenAPI docs
    #[derive(OpenApi)]
    #[openapi(
        paths(
            profiling::get_metrics,
            profiling::get_health,
            dashboard::get_dashboard_metrics,
            dashboard::get_contract_stats,
        ),
        components(schemas(
            profiling::MetricsReport,
            profiling::HealthResponse,
            dashboard::DashboardMetrics,
            dashboard::ContractStats
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
        .route("/api/config", get(handle_get_config))
        .route("/api/config/reload", post(handle_reload))
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
        .nest(
            "/api/v1/dashboard",
            Router::new()
                .route("/", get(get_dashboard))
                .route("/metrics", get(dashboard::get_dashboard_metrics))
                .route("/contracts/:contract_id/stats", get(dashboard::get_contract_stats))
                .with_state(dashboard_state),
        )
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            logging_middleware,
        ))
        .layer(TraceLayer::new_for_http())
        .layer(cors)
        .with_state(state); // fallback state for /api/config handlers

    let addr: SocketAddr = format!("{}:{}", config.server.host, config.server.port).parse()?;
    
    tracing::info!("Crucible backend listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;

    tokio::select! {
        res = axum::serve(listener, app).with_graceful_shutdown(shutdown_signal()) => {
            res?;
        },
        _ = worker.run() => {
            tracing::info!("Worker stopped");
        }
    }

    Ok(())
}

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
            tracing::info!("Received Ctrl+C, starting graceful shutdown");
        },
        _ = terminate => {
            tracing::info!("Received SIGTERM, starting graceful shutdown");
        },
    }
}
