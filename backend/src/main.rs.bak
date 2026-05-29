use axum::{
    routing::{get, post},
    Router,
};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use sqlx::postgres::PgPoolOptions;

mod error;
mod services;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "backend=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Database setup
    let db_url = std::env::var("DATABASE_URL").unwrap_or_else(|_| "postgres://postgres:postgres@localhost/crucible".into());
    let db = PgPoolOptions::new()
        .max_connections(5)
        .connect(&db_url)
        .await?;

    // Redis setup
    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1/".into());
    let redis = redis::Client::open(redis_url)?;

    let state = Arc::new(services::log_alerts::ServiceState {
        db,
        redis,
    });

    // Build our application with a route
    let app = Router::new()
        .route("/", get(|| async { "Crucible Backend API" }))
        .nest("/api/alerts", services::log_alerts::router())
        .with_state(state);

    // Run it with hyper
    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::debug!("listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
//! # Crucible Backend
//!
//! Production-ready HTTP API server for the Crucible smart contract testing
//! platform. Built with [Axum](https://docs.rs/axum), [SQLx](https://docs.rs/sqlx)
//! (PostgreSQL), and [Redis](https://docs.rs/redis) for caching and job queues.
//!
//! ## Architecture
//!
//! ```text
//! ┌──────────┐     ┌──────────────┐     ┌────────────┐
//! │  Client   │────▶│  Axum Router  │────▶│ PostgreSQL │
//! └──────────┘     │  (port 8080)  │     └────────────┘
//!                  │               │     ┌────────────┐
//!                  │  Middleware:   │────▶│   Redis    │
//!                  │  - CORS       │     └────────────┘
//!                  │  - Tracing    │
//!                  │  - Compression│
//!                  └──────────────┘
//! ```

use std::net::SocketAddr;
use std::time::Duration;

use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use redis::aio::ConnectionManager;
use serde::Serialize;
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing::{info, warn};

pub mod error;

/// Shared application state passed to all handlers via Axum's state extraction.
#[derive(Clone)]
pub struct AppState {
    /// PostgreSQL connection pool managed by SQLx.
    pub db: PgPool,
    /// Redis connection manager for caching and job queues.
    pub redis: ConnectionManager,
}

/// Response returned by the `/health` endpoint.
#[derive(Serialize)]
struct HealthResponse {
    status: String,
    version: String,
    database: String,
    redis: String,
}

#[tokio::main]
async fn main() {
    // Load .env file if present (development convenience)
    dotenvy::dotenv().ok();

    // Initialize structured logging with tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "crucible_backend=debug,tower_http=debug".into()),
        )
        .with_target(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .init();

    info!("Starting Crucible Backend");

    // ----- Database connection -----
    let database_url = std::env::var("DATABASE_URL")
        .expect("DATABASE_URL must be set");

    let max_connections: u32 = std::env::var("DATABASE_MAX_CONNECTIONS")
        .unwrap_or_else(|_| "10".into())
        .parse()
        .expect("DATABASE_MAX_CONNECTIONS must be a valid u32");

    let min_connections: u32 = std::env::var("DATABASE_MIN_CONNECTIONS")
        .unwrap_or_else(|_| "2".into())
        .parse()
        .expect("DATABASE_MIN_CONNECTIONS must be a valid u32");

    let db = PgPoolOptions::new()
        .max_connections(max_connections)
        .min_connections(min_connections)
        .acquire_timeout(Duration::from_secs(30))
        .idle_timeout(Duration::from_secs(600))
        .test_before_acquire(true)
        .connect(&database_url)
        .await
        .expect("Failed to connect to PostgreSQL");

    info!("Connected to PostgreSQL (pool: {min_connections}..{max_connections})");

    // Run pending migrations
    sqlx::migrate!("./migrations")
        .run(&db)
        .await
        .expect("Failed to run database migrations");

    info!("Database migrations applied");

    // ----- Redis connection -----
    let redis_url = std::env::var("REDIS_URL")
        .unwrap_or_else(|_| "redis://127.0.0.1:6379".into());

    let redis_client = redis::Client::open(redis_url.as_str())
        .expect("Invalid REDIS_URL");

    let redis = ConnectionManager::new(redis_client)
        .await
        .expect("Failed to connect to Redis");

    info!("Connected to Redis");

    // ----- Application state -----
    let state = AppState { db, redis };

    // ----- Router -----
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/api/v1/status", get(api_status))
        .layer(TraceLayer::new_for_http())
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        .with_state(state);

    // ----- Server -----
    let host = std::env::var("APP_HOST").unwrap_or_else(|_| "0.0.0.0".into());
    let port: u16 = std::env::var("APP_PORT")
        .unwrap_or_else(|_| "8080".into())
        .parse()
        .expect("APP_PORT must be a valid u16");

    let addr: SocketAddr = format!("{host}:{port}")
        .parse()
        .expect("Invalid APP_HOST:APP_PORT combination");

    info!("Listening on {addr}");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("Failed to bind TCP listener");

    axum::serve(listener, app.into_make_service())
        .await
        .expect("Server error");
}

/// `GET /health` — Comprehensive health check for load balancers and Docker.
///
/// Verifies connectivity to both PostgreSQL and Redis, returning a JSON
/// response with individual service statuses.
async fn health_check(State(state): State<AppState>) -> impl IntoResponse {
    let db_status = match sqlx::query_scalar::<_, i32>("SELECT 1")
        .fetch_one(&state.db)
        .await
    {
        Ok(_) => "healthy".to_string(),
        Err(e) => {
            warn!("Database health check failed: {e}");
            format!("unhealthy: {e}")
        }
    };

    let redis_status = {
        let mut conn = state.redis.clone();
        match redis::cmd("PING")
            .query_async::<String>(&mut conn)
            .await
        {
            Ok(_) => "healthy".to_string(),
            Err(e) => {
                warn!("Redis health check failed: {e}");
                format!("unhealthy: {e}")
            }
        }
    };

    let all_healthy = db_status == "healthy" && redis_status == "healthy";
    let status_code = if all_healthy {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        status_code,
        Json(HealthResponse {
            status: if all_healthy {
                "ok".into()
            } else {
                "degraded".into()
            },
            version: env!("CARGO_PKG_VERSION").into(),
            database: db_status,
            redis: redis_status,
        }),
    )
}

/// `GET /api/v1/status` — Simple API status endpoint.
async fn api_status() -> impl IntoResponse {
    Json(serde_json::json!({
        "service": "crucible-backend",
        "version": env!("CARGO_PKG_VERSION"),
        "status": "running"
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_response_serialization() {
        let response = HealthResponse {
            status: "ok".into(),
            version: "0.1.0".into(),
            database: "healthy".into(),
            redis: "healthy".into(),
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"status\":\"ok\""));
        assert!(json.contains("\"database\":\"healthy\""));
        assert!(json.contains("\"redis\":\"healthy\""));
    }

    #[test]
    fn test_health_response_fields() {
        let response = HealthResponse {
            status: "degraded".into(),
            version: "0.1.0".into(),
            database: "healthy".into(),
            redis: "unhealthy: connection refused".into(),
        };
        assert_eq!(response.status, "degraded");
        assert_eq!(response.database, "healthy");
        assert!(response.redis.starts_with("unhealthy"));
mod api;
mod services;
mod config;

use std::sync::Arc;
use apalis::prelude::*;
use apalis_redis::RedisStorage;
use axum::{
    routing::{get, post},
    Router,
};
use backend::api::handlers::dashboard::{get_dashboard, DashboardState};
use backend::{
    api::handlers::{profiling, stellar},
    config::Config,
    jobs::{monitor_transaction, TransactionMonitorJob},
    config::Config,
    jobs::{monitor_transaction, TransactionMonitorJob},
    api::handlers::{profiling, stellar, dashboard},
    api::handlers::{profiling, stellar},
    api::middleware::logging::logging_middleware,
    services::{
        error_recovery::ErrorManager, log_aggregator::LogAggregator, log_alerts::AlertManager,
        sys_metrics::MetricsExporter,
        error_recovery::ErrorManager,
        log_aggregator::LogAggregator,
        tracing::{TracingService, TracingConfig},
    },
    telemetry::init_telemetry,
};
use profiling::AppState;
use redis::aio::ConnectionManager;
use sqlx::postgres::PgPoolOptions;
use axum::{routing::{get, post}, Router, middleware};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::signal;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use crate::config::{AppConfig, reload::{ConfigManager, handle_reload, handle_get_config}};
use utoipa::OpenApi;
use utoipa_swagger_ui::SwaggerUi;
use profiling::AppState;
use apalis::prelude::*;
use apalis_redis::RedisStorage;
use sqlx::postgres::PgPoolOptions;
use redis::aio::ConnectionManager;
use redis::Client as RedisClient;
use std::sync::Arc;
use tracing::info_span;

#[tokio::main]
async fn main() -> Result<(), anyhow::Error> {
    // Load configuration
    let config = Config::from_env()?;

    // Initialize observability (fmt tracing subscriber)
    init_telemetry();

    // Database setup
    // Initialize OpenTelemetry tracing FIRST - before any other services
    let tracing_config = TracingConfig::new(
        "crucible-backend".to_string(),
        env!("CARGO_PKG_VERSION").to_string(),
    )
    .with_environment(std::env::var("ENV").unwrap_or("dev".to_string()))
    .with_otlp_endpoint(
        std::env::var("OTLP_ENDPOINT")
            .unwrap_or_else(|_| "http://localhost:4317".to_string())
    );

    TracingService::init(tracing_config)?;

    let span = info_span!("app.startup");
    let _enter = span.enter();

    // Database setup & migrations
    let db_span = TracingService::db_query_span(
        "CONNECT postgresql",
        "postgres",
        "CONNECT",
    );
    let _db_enter = db_span.enter();

    let db_pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&config.database_url)
        .await?;

    tracing::info!("Database connection established");
    
    tracing::info!("Database pool initialized");
    drop(_db_enter);

    let redis_client = RedisClient::open(config.redis_url.clone())?;

    // Initialize services
    let metrics_exporter = Arc::new(MetricsExporter::new());
    let error_manager = Arc::new(ErrorManager::new());
    let alert_manager = Arc::new(AlertManager::new());
    let (_log_aggregator, log_receiver) = LogAggregator::new();
    let (log_aggregator, log_receiver) = LogAggregator::new();
    let log_aggregator = Arc::new(log_aggregator);

    tokio::spawn(MetricsExporter::run_collector(metrics_exporter.clone()));
    tokio::spawn(LogAggregator::run_worker(log_receiver));
    
    // Initialize config manager
    let config = AppConfig::default();
    let config_manager = Arc::new(ConfigManager::new(config));

    // Redis + job queue setup
    // Redis Job Queue setup
    let conn = ConnectionManager::new(redis_client.clone()).await?;
    let redis_span = TracingService::redis_command_span("CONNECT", None);
    let _redis_enter = redis_span.enter();

    let redis_client = redis::Client::open(config.redis_url.clone())?;
    let conn = ConnectionManager::new(redis_client.clone()).await?;
    let redis_conn_dashboard = ConnectionManager::new(redis_client).await?;
    let storage: RedisStorage<TransactionMonitorJob> = RedisStorage::new(conn);

    
    tracing::info!("Redis connection established");
    drop(_redis_enter);
    
    let worker = WorkerBuilder::new("monitor-worker")
        .backend(storage)
        .build_fn(monitor_transaction);

    // Create shared state
    let state = Arc::new(AppState {
        db: db_pool.clone(),
    // Shared state for profiling/status routes
    let profiling_state = Arc::new(AppState {
        db: Some(db_pool),
        metrics_exporter: metrics_exporter.clone(),
        error_manager: error_manager.clone(),
    });

    // Shared state for dashboard route
    let dashboard_state = Arc::new(DashboardState {
        metrics_exporter,
        error_manager,
        config_manager: config_manager.clone(),
        alert_manager,
        log_aggregator,
        redis: redis_client,
    });

    // Create dashboard state
    let dashboard_state = Arc::new(dashboard::DashboardState {
        db: db_pool,
        redis: redis_conn_dashboard,
    });

    // Define OpenAPI documentation
    // OpenAPI docs
    #[derive(OpenApi)]
    #[openapi(
        paths(
            profiling::get_metrics,
            profiling::get_health,
            dashboard::get_dashboard_metrics,
            dashboard::get_contract_stats,
        ),
        components(
            schemas(
                profiling::MetricsReport, 
                profiling::HealthResponse,
                dashboard::DashboardMetrics,
                dashboard::ContractStats
            )
        ),
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
        .route("/api/status", get(get_system_status))
        .route("/api/profile", post(trigger_profile_collection))
        .route("/api/config", get(handle_get_config))
        .route("/api/config/reload", post(handle_reload))
        .with_state(state);
        .route("/", get(|| async { "Crucible Backend API" }))
        .route("/.well-known/stellar.toml", get(stellar::get_stellar_toml))
        .nest(
            "/api/v1/profiling",
            Router::new()
                .route("/metrics", get(profiling::get_metrics))
                .route("/health", get(profiling::get_health))
                .route("/prometheus", get(profiling::get_prometheus_metrics)),
        )
        .nest("/api/v1/dashboard", Router::new()
            .route("/metrics", get(dashboard::get_dashboard_metrics))
            .route("/contracts/:contract_id/stats", get(dashboard::get_contract_stats))
            .with_state(dashboard_state)
        )
        .nest(
            "/api/v1/errors",
            errors::error_analytics_routes(db_pool.clone(), redis_conn_dashboard.clone())
        )
        .route("/api/status", get(profiling::get_system_status))
        .route("/api/profile", post(profiling::trigger_profile_collection))
        .with_state(profiling_state)
        .route("/api/dashboard", get(get_dashboard))
        .with_state(dashboard_state)
        .merge(SwaggerUi::new("/swagger-ui").url("/api-docs/openapi.json", ApiDoc::openapi()))
        .layer(middleware::from_fn_with_state(state.clone(), logging_middleware))
        .layer(TraceLayer::new_for_http())
        .layer(cors);

    let addr = SocketAddr::from(([0, 0, 0, 0], config.server_port));
    tracing::info!("listening on {}", addr);

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
