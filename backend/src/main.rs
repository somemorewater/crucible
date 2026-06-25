use apalis::prelude::*;
use apalis_redis::RedisStorage;
use axum::{
    middleware,
    routing::{get, post},
    Router,
};
use backend::{
    api::handlers::{
        coverage::CoverageState,
        dashboard::{self, get_dashboard, get_dashboard_metrics, get_contract_stats, DashboardState},
        errors, profiling, sandbox, stellar,
        ws::{self, ws_dashboard_handler},
    },
    api::middleware::logging::logging_middleware,
    config::{AppConfig, reload::{ConfigManager, handle_reload, handle_get_config}, Environment},
    jobs::{monitor_transaction, TransactionMonitorJob},
    services::{
        audit::{self, AuditService},
        contract_benchmark::ContractBenchmarkService,
        error_recovery::ErrorManager,
        log_aggregator::LogAggregator,
        log_alerts::AlertManager,
        sandbox::ContractSandboxService,
        sys_metrics::MetricsExporter,
        test_coverage::TestCoverageService,
        tracing::{TracingConfig, TracingService},
    },
};
use profiling::AppState;
use redis::aio::ConnectionManager;
use redis::Client as RedisClient;
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

    // Initialize OpenTelemetry tracing before other services so startup work is captured.
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
            .unwrap_or_else(|| "http://localhost:4318/v1/traces".to_string()),
    );

    let _tracing_guard = TracingService::init(tracing_config)?;

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

    let redis_client = redis::Client::open(config.redis.url.clone())?;

    // Coverage state (constructed early so it can be moved into the router)
    let coverage_state = Arc::new(CoverageState {
        service: TestCoverageService::new(db_pool.clone(), redis_client.clone()),
    });

    // Initialize services
    let metrics_exporter = Arc::new(MetricsExporter::new());
    let error_manager = Arc::new(ErrorManager::new());
    let alert_manager = Arc::new(AlertManager::new());
    let (log_aggregator, log_receiver) = LogAggregator::new();
    let log_aggregator = Arc::new(log_aggregator);
    let sandbox_service = Arc::new(ContractSandboxService::default());
    let contract_benchmark_service = Arc::new(ContractBenchmarkService::new());

    tokio::spawn(MetricsExporter::run_collector(metrics_exporter.clone()));
    tokio::spawn(LogAggregator::run_worker(log_receiver));

    // Initialize config manager
    let config_manager = Arc::new(ConfigManager::new(config.clone()));

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
        contract_benchmark_service: contract_benchmark_service.clone(),
        redis: redis_client.clone(),
    });

    // Create dashboard state
    let dashboard_state = Arc::new(DashboardState {
        db: db_pool.clone(),
        redis_conn: redis_conn_dashboard,
        metrics_exporter: metrics_exporter.clone(),
        error_manager: error_manager.clone(),
        alert_manager: alert_manager.clone(),
        redis_client: redis_client.clone(),
    });

    let audit_service = Arc::new(AuditService::new(db_pool.clone(), Arc::new(redis_client.clone())));

    // Create WS state
    let ws_state = Arc::new(ws::WsState {
        metrics_exporter: metrics_exporter.clone(),
        error_manager: error_manager.clone(),
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
            dashboard::ContractStats,
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
        .route(
            "/api/config",
            get(handle_get_config).with_state(profiling_state.clone()),
        )
        .route(
            "/api/config/reload",
            post(handle_reload).with_state(profiling_state.clone()),
        )
        .nest(
            "/api/v1/profiling",
            Router::new()
                .route("/metrics", get(profiling::get_metrics))
                .route("/health", get(profiling::get_health))
                .route("/prometheus", get(profiling::get_prometheus_metrics))
                .route("/status", get(profiling::get_system_status))
                .route("/profile", post(profiling::trigger_profile_collection))
                .route(
                    "/contracts/benchmark",
                    post(profiling::run_contract_benchmark),
                )
                .with_state(profiling_state.clone()),
        )
        .route("/api/status", get(profiling::get_system_status))
        .route("/api/profile", post(profiling::trigger_profile_collection))
        .with_state(profiling_state.clone())
        .nest(
            "/api/v1/dashboard",
            Router::new()
                .route("/", get(get_dashboard))
                .route("/metrics", get(get_dashboard_metrics))
                .route("/contracts/:contract_id/stats", get(get_contract_stats))
                .with_state(dashboard_state.clone()),
        )
        .nest(
            "/api/v1/audit",
            audit::routes(audit_service.clone()),
        )
        .nest(
            "/api/v1/errors",
            errors::error_analytics_routes(db_pool.clone(), redis_client.clone()),
        )
        .nest(
            "/api/v1/contracts",
            Router::new()
                .route(
                    "/compile",
                    post(backend::api::handlers::contracts::compile_contract),
                )
                .route(
                    "/analyze-dependencies",
                    post(backend::api::handlers::contracts::analyze_dependencies),
                )
                .route(
                    "/compliance-check",
                    post(backend::api::handlers::contracts::check_compliance),
                )
                .route(
                    "/logs",
                    post(backend::api::handlers::contracts::log_contract_call),
                )
                .route(
                    "/upgrade-plan",
                    post(backend::api::handlers::contracts::create_upgrade_plan),
                )
                .route(
                    "/templates",
                    get(backend::api::handlers::contracts::get_templates),
                )
                .with_state(profiling_state.clone()),
        )
        .route(
            "/api/v1/networks",
            get(backend::api::handlers::contracts::get_networks),
        )
        .nest(
            "/api/v1/admin",
            Router::new()
                .route("/system-stats", get(backend::api::handlers::admin::get_system_stats))
                .route("/maintenance", post(backend::api::handlers::admin::set_maintenance_mode))
                .route("/logs", get(backend::api::handlers::admin::get_admin_logs))
                .with_state(profiling_state.clone()),
        )
        .nest("/api/v1/sandbox", sandbox::routes(sandbox_service))
        .nest(
            "/api/v1/coverage",
            Router::new()
                .route(
                    "/",
                    post(backend::api::handlers::coverage::submit_coverage),
                )
                .route(
                    "/:project",
                    get(backend::api::handlers::coverage::get_latest_coverage),
                )
                .with_state(coverage_state),
        )
        .route(
            "/api/v1/ws/dashboard",
            get(ws_dashboard_handler).with_state(ws_state),
        )
        .nest(
            "/api/v1/dashboard/extra",
            Router::new()
                .route("/", get(get_dashboard))
                .with_state(dashboard_state.clone()),
        )
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .layer(middleware::from_fn_with_state(profiling_state.clone(), logging_middleware))
        .layer(TraceLayer::new_for_http())
        .layer(cors);

    let addr: SocketAddr = format!("{}:{}", config.server.host, config.server.port).parse()?;
    let listener = tokio::net::TcpListener::bind(&addr).await?;

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

            match res {
                Ok(()) => tracing::info!("Server stopped accepting new connections"),
                Err(ref e) => tracing::error!("Server error during shutdown: {e}"),
            }

            tracing::info!("Waiting for in-flight requests to complete (timeout: {}s)", shutdown_timeout.as_secs());
            match tokio::time::timeout(shutdown_timeout, async {
                tokio::time::sleep(shutdown_timeout).await;
            }).await {
                Ok(()) => tracing::info!("In-flight requests completed"),
                Err(_) => tracing::warn!("Timeout waiting for in-flight requests to complete"),
            }

            tracing::info!("Closing database connection pool");
            drop(db_pool);

            tracing::info!("Graceful shutdown completed successfully");
            res
        },
        _ = worker.run() => {
            tracing::info!("Worker stopped");
            Ok(())
        }
    };

    if let Err(e) = &result {
        tracing::error!("Application error: {e}");
    }

    result?;
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
            tracing::info!("Received Ctrl+C, initiating graceful shutdown");
        },
        _ = terminate => {
            tracing::info!("Received SIGTERM, initiating graceful shutdown");
        },
    }
}
