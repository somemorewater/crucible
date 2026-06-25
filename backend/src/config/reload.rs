//! Configuration hot-reload.
//!
//! This module provides two complementary configuration management types:
//!
//! - [`ConfigManager`] — a simple `ArcSwap`-backed manager used by the
//!   profiling handlers. Supports file-based and patch-based reloads.
//! - [`ConfigWatcher`] — a richer watcher that subscribes to a Redis pub/sub
//!   channel and atomically swaps the live config on every reload signal.
//!
//! # Redis protocol (ConfigWatcher)
//!
//! ```text
//! SET config:current '{\"log_level\":\"info\",\"max_connections\":50,...}'
//! PUBLISH config:reload "reload"
//! ```

#![allow(dead_code)]

use std::sync::Arc;

use arc_swap::ArcSwap;
use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use redis::{AsyncCommands, Client as RedisClient};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::sync::{watch, RwLock};
use tracing::{error, info, instrument, warn};

use crate::config::AppConfig;

// ---------------------------------------------------------------------------
// ReloadError
// ---------------------------------------------------------------------------

/// Errors that can occur during configuration reload.
#[derive(Debug, Error)]
pub enum ReloadError {
    /// A Redis error occurred.
    #[error("Redis error: {0}")]
    Redis(#[from] redis::RedisError),

    /// The configuration value could not be deserialised.
    #[error("Config deserialisation error: {0}")]
    Deserialise(#[from] serde_json::Error),

    /// The configuration key was not found in Redis.
    #[error("Config key not found in Redis")]
    NotFound,

    /// An I/O error occurred (e.g. reading config.json).
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// The configuration value was semantically invalid.
    #[error("Invalid configuration: {0}")]
    Invalid(String),
}

impl IntoResponse for ReloadError {
    fn into_response(self) -> axum::response::Response {
        let status = match self {
            ReloadError::Invalid(_) | ReloadError::Deserialise(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(serde_json::json!({ "error": self.to_string() }))).into_response()
    }
}

// ---------------------------------------------------------------------------
// ConfigManager — ArcSwap-based, patch-capable
// ---------------------------------------------------------------------------

/// Manages hot-reloadable application configuration via lock-free reads.
///
/// Wrap in an [`Arc`] and share across Axum handlers via application state.
pub struct ConfigManager {
    current: ArcSwap<AppConfig>,
}

impl ConfigManager {
    /// Create a new manager with the given initial configuration.
    pub fn new(initial: AppConfig) -> Self {
        Self {
            current: ArcSwap::from(Arc::new(initial)),
        }
    }

    /// Return a snapshot of the current configuration.
    ///
    /// This is a lock-free read — safe to call from hot paths.
    pub fn load(&self) -> Arc<AppConfig> {
        self.current.load_full()
    }

    /// Atomically replace the current configuration.
    /// Reads the JSON value from `config.json` in the current directory,
    /// validates it, and swaps it in.
    #[instrument(skip(self))]
    pub async fn reload(&self) -> Result<(), ReloadError> {
        info!("Starting configuration reload from config.json");

        let path = "config.json";
        if !std::path::Path::new(path).exists() {
            warn!("config.json not found, aborting reload");
            return Err(ReloadError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "config.json not found",
            )));
        }

        let content = tokio::fs::read_to_string(path).await?;
        let new_config: AppConfig = serde_json::from_str(&content)?;

        if new_config.database.url.is_empty() {
            return Err(ReloadError::Invalid("database.url cannot be empty".into()));
        }

        self.current.store(Arc::new(new_config));
        info!("Configuration reloaded successfully");
        Ok(())
    }

    /// Apply a partial JSON patch to the current configuration.
    /// Top-level and one-level-deep object keys are merged; all other values
    /// are replaced. Returns an error if the result cannot be deserialised
    /// into [`AppConfig`].
    #[instrument(skip(self, patch))]
    pub fn update_from_patch(&self, patch: Value) -> Result<(), ReloadError> {
        let current = self.load();
        let mut current_json = serde_json::to_value(&*current)?;

        if let (Some(patch_obj), Some(current_obj)) =
            (patch.as_object(), current_json.as_object_mut())
        {
            for (k, v) in patch_obj {
                if v.is_object() {
                    if let Some(sub) = current_obj.get_mut(k).and_then(|s| s.as_object_mut()) {
                        for (sk, sv) in v.as_object().unwrap() {
                            sub.insert(sk.clone(), sv.clone());
                        }
                        continue;
                    }
                }
                current_obj.insert(k.clone(), v.clone());
            }
        }

        let new_config: AppConfig = serde_json::from_value(current_json)?;
        self.current.store(Arc::new(new_config));
        info!("Configuration updated via patch");
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// HotAppConfig (used by ConfigWatcher)
// ---------------------------------------------------------------------------

/// Live application configuration that can be hot-reloaded at runtime.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HotAppConfig {
    /// Tracing / log filter directive (e.g. `"backend=debug"`).
    pub log_level: String,
    /// Maximum number of database connections in the pool.
    pub max_connections: u32,
    /// Request timeout in seconds.
    pub request_timeout_secs: u64,
    /// Whether the maintenance mode banner is shown.
    pub maintenance_mode: bool,
    /// Redis key that stores the serialised [`HotAppConfig`] JSON.
    pub redis_config_key: String,
}

impl Default for HotAppConfig {
    fn default() -> Self {
        Self {
            log_level: "backend=debug,tower_http=debug".to_string(),
            max_connections: 10,
            request_timeout_secs: 30,
            maintenance_mode: false,
            redis_config_key: "config:current".to_string(),
        }
    }
}

// ---------------------------------------------------------------------------
// ConfigHandle — cheap clone, shared reader with change notification
// ---------------------------------------------------------------------------

/// A cheap-to-clone handle to the live configuration.
#[derive(Clone)]
pub struct ConfigHandle {
    inner: Arc<RwLock<HotAppConfig>>,
    changed: watch::Receiver<()>,
}

impl ConfigHandle {
    /// Return a snapshot of the current configuration.
    pub async fn get(&self) -> HotAppConfig {
        self.inner.read().await.clone()
    }

    /// Wait until the configuration changes, then return the new snapshot.
    pub async fn wait_for_change(&mut self) -> HotAppConfig {
        let _ = self.changed.changed().await;
        self.get().await
    }
}

// ---------------------------------------------------------------------------
// ConfigWatcher — Redis pub/sub driven reload
// ---------------------------------------------------------------------------

/// Owns the live [`HotAppConfig`] and drives hot-reload via Redis pub/sub.
///
/// Wrap in an [`Arc`] to share across tasks.
pub struct ConfigWatcher {
    inner: Arc<RwLock<HotAppConfig>>,
    notify_tx: watch::Sender<()>,
    notify_rx: watch::Receiver<()>,
}

impl ConfigWatcher {
    /// Create a new watcher with the given initial configuration.
    pub fn new(initial: HotAppConfig) -> Self {
        let (tx, rx) = watch::channel(());
        Self {
            inner: Arc::new(RwLock::new(initial)),
            notify_tx: tx,
            notify_rx: rx,
        }
    }

    /// Return a [`ConfigHandle`] that can be cloned and shared freely.
    pub fn handle(&self) -> ConfigHandle {
        ConfigHandle {
            inner: Arc::clone(&self.inner),
            changed: self.notify_rx.clone(),
        }
    }

    /// Atomically replace the current configuration and notify all handles.
    ///
    /// If the new config is identical to the current one, no notification is sent.
    pub async fn reload(&self, new_config: HotAppConfig) {
        let old = {
            let mut guard = self.inner.write().await;
            let old = guard.clone();
            *guard = new_config.clone();
            old
        };
        if old != new_config {
            info!(
                log_level = %new_config.log_level,
                max_connections = new_config.max_connections,
                maintenance_mode = new_config.maintenance_mode,
                "Configuration reloaded"
            );
            let _ = self.notify_tx.send(());
        } else {
            info!("Configuration reload requested but values unchanged");
        }
    }

    /// Fetch the current configuration from Redis and apply it.
    ///
    /// Reads the JSON value stored at the key `config:current`, deserialises
    /// it, and calls [`Self::reload`].
    /// # Errors
    /// Returns [`ReloadError`] if the Redis key is absent, the connection
    /// fails, or the JSON cannot be deserialised.
    pub async fn reload_from_redis(&self, redis: &RedisClient) -> Result<(), ReloadError> {
        const KEY: &str = "config:current";
        let mut conn = redis.get_multiplexed_async_connection().await?;
        let raw: Option<String> = conn.get(KEY).await?;
        let json = raw.ok_or(ReloadError::NotFound)?;
        let new_config: HotAppConfig = serde_json::from_str(&json)?;
        self.reload(new_config).await;
        Ok(())
    }

    /// Spawn a background task that subscribes to `config:reload` on Redis
    /// and calls [`Self::reload_from_redis`] on every message.
    ///
    /// The task runs until the Redis pub/sub stream ends or the process exits.
    /// Connection errors are logged and the task exits — callers may restart
    /// it if desired.
    pub fn watch(self: Arc<Self>, redis: RedisClient) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            const CHANNEL: &str = "config:reload";

            #[allow(deprecated)]
            let conn = match redis.get_async_connection().await {
                Ok(c) => c,
                Err(e) => {
                    error!(error = %e, "Config watcher: failed to connect to Redis");
                    return;
                }
            };

            let mut pubsub = conn.into_pubsub();
            if let Err(e) = pubsub.subscribe(CHANNEL).await {
                error!(error = %e, channel = CHANNEL, "Config watcher: subscribe failed");
                return;
            }

            info!(channel = CHANNEL, "Config watcher: listening for reload signals");

            use futures_util::StreamExt;
            let mut stream = pubsub.into_on_message();

            loop {
                match stream.next().await {
                    Some(msg) => {
                        let payload: String = msg.get_payload().unwrap_or_default();
                        info!(payload = %payload, "Config reload signal received");
                        if let Err(e) = self.reload_from_redis(&redis).await {
                            warn!(
                                error = %e,
                                "Config reload from Redis failed; keeping current config"
                            );
                        }
                    }
                    None => {
                        warn!("Config watcher: Redis pub/sub stream ended");
                        break;
                    }
                }
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Axum handlers
// ---------------------------------------------------------------------------

/// `POST /api/config/reload` — Reload configuration from `config.json`.
///
/// Returns `200 OK` on success or an error response if the file is missing
/// or the JSON is invalid.
pub async fn handle_reload(
    State(state): State<Arc<crate::api::handlers::profiling::AppState>>,
) -> Result<impl IntoResponse, ReloadError> {
    state.config_manager.reload().await?;
    Ok((StatusCode::OK, Json(serde_json::json!({ "status": "reloaded" }))))
}

/// `GET /api/config` — Return the current configuration as JSON.
/// Sensitive fields (e.g. database passwords embedded in URLs) are returned
/// as-is; callers should restrict access to this endpoint appropriately.
pub async fn handle_get_config(
    State(state): State<Arc<crate::api::handlers::profiling::AppState>>,
) -> impl IntoResponse {
    let config = state.config_manager.load();
    Json(config.as_ref().clone())
}
