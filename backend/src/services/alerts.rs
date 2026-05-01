//! Critical error alerting service.
//!
//! This module sits on top of [`log_alerts`] and [`error_recovery`] and is
//! responsible for *dispatching* notifications when a critical condition is
//! detected. It supports two notification channels:
//!
//! - **In-memory** — a bounded queue that callers can drain (useful for tests
//!   and for feeding a WebSocket push layer).
//! - **Redis pub/sub** — publishes a JSON payload to a configurable channel so
//!   that any subscriber (e.g. a separate alerting micro-service) is notified.
//!
//! Alerts are deduplicated within a configurable cooldown window: if the same
//! `alert_key` fires again before the cooldown expires the notification is
//! silently dropped.
//!
//! # Example
//!
//! ```rust,no_run
//! use std::sync::Arc;
//! use backend::services::alerts::{AlertDispatcher, AlertNotification, NotificationLevel};
//!
//! # async fn example() -> anyhow::Result<()> {
//! let dispatcher = Arc::new(AlertDispatcher::new(None, 60));
//!
//! dispatcher.dispatch(AlertNotification {
//!     alert_key: "db_connection_lost".to_string(),
//!     level: NotificationLevel::Critical,
//!     title: "Database unreachable".to_string(),
//!     message: "Connection pool exhausted after 3 retries".to_string(),
//!     metadata: Default::default(),
//! }).await?;
//!
//! let pending = dispatcher.drain_notifications().await;
//! assert_eq!(pending.len(), 1);
//! # Ok(())
//! # }
//! ```

#![allow(dead_code)]

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use redis::{AsyncCommands, Client as RedisClient};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::services::log_alerts::{Alert, AlertSeverity};

// ---------------------------------------------------------------------------
// Error type
// ---------------------------------------------------------------------------

/// Errors that can occur while dispatching alert notifications.
#[derive(Debug, Error)]
pub enum AlertDispatchError {
    /// A Redis error occurred while publishing.
    #[error("Redis publish error: {0}")]
    Redis(#[from] redis::RedisError),

    /// JSON serialisation failed.
    #[error("Serialisation error: {0}")]
    Serialisation(#[from] serde_json::Error),
}

// ---------------------------------------------------------------------------
// Domain types
// ---------------------------------------------------------------------------

/// Severity level of a dispatched notification.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NotificationLevel {
    /// Informational notification.
    Info,
    /// Warning — should be investigated.
    Warning,
    /// Critical — requires immediate attention.
    Critical,
}

impl From<&AlertSeverity> for NotificationLevel {
    fn from(s: &AlertSeverity) -> Self {
        match s {
            AlertSeverity::Info => NotificationLevel::Info,
            AlertSeverity::Warning => NotificationLevel::Warning,
            AlertSeverity::Critical => NotificationLevel::Critical,
        }
    }
}

impl std::fmt::Display for NotificationLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotificationLevel::Info => write!(f, "info"),
            NotificationLevel::Warning => write!(f, "warning"),
            NotificationLevel::Critical => write!(f, "critical"),
        }
    }
}

/// A notification that has been dispatched.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertNotification {
    /// Stable key used for deduplication (e.g. `"db_connection_lost"`).
    pub alert_key: String,
    /// Severity of the notification.
    pub level: NotificationLevel,
    /// Short human-readable title.
    pub title: String,
    /// Detailed message.
    pub message: String,
    /// Arbitrary key-value metadata (rule name, service, etc.).
    pub metadata: HashMap<String, String>,
}

/// An envelope wrapping a notification with dispatch metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchedNotification {
    /// Unique ID for this dispatch event.
    pub id: Uuid,
    /// The notification payload.
    pub notification: AlertNotification,
    /// When this notification was dispatched.
    pub dispatched_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// AlertDispatcher
// ---------------------------------------------------------------------------

/// Dispatches critical-error notifications to in-memory and Redis channels.
///
/// Construct with [`AlertDispatcher::new`], optionally providing a Redis
/// client for pub/sub publishing.
pub struct AlertDispatcher {
    redis: Option<RedisClient>,
    /// Redis pub/sub channel name.
    redis_channel: String,
    /// Cooldown in seconds — duplicate `alert_key`s within this window are dropped.
    cooldown_secs: i64,
    /// In-memory queue of dispatched notifications.
    queue: Arc<RwLock<Vec<DispatchedNotification>>>,
    /// Tracks the last dispatch time per `alert_key` for deduplication.
    last_dispatched: Arc<RwLock<HashMap<String, DateTime<Utc>>>>,
}

impl AlertDispatcher {
    /// Create a new dispatcher.
    ///
    /// # Arguments
    /// - `redis` — optional Redis client for pub/sub publishing.
    /// - `cooldown_secs` — deduplication window; the same `alert_key` will not
    ///   be dispatched again until this many seconds have elapsed.
    pub fn new(redis: Option<RedisClient>, cooldown_secs: i64) -> Self {
        Self {
            redis,
            redis_channel: "alerts:critical".to_string(),
            cooldown_secs,
            queue: Arc::new(RwLock::new(Vec::new())),
            last_dispatched: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Override the Redis pub/sub channel name (default: `"alerts:critical"`).
    pub fn with_channel(mut self, channel: impl Into<String>) -> Self {
        self.redis_channel = channel.into();
        self
    }

    /// Dispatch a notification.
    ///
    /// If the same `alert_key` was dispatched within the cooldown window the
    /// call is a no-op and returns `Ok(false)`. Otherwise the notification is
    /// appended to the in-memory queue and, if a Redis client is configured,
    /// published to the pub/sub channel. Returns `Ok(true)` when dispatched.
    ///
    /// # Errors
    /// Returns [`AlertDispatchError`] only on Redis or serialisation failures.
    pub async fn dispatch(
        &self,
        notification: AlertNotification,
    ) -> Result<bool, AlertDispatchError> {
        // --- deduplication check ---
        {
            let last = self.last_dispatched.read().await;
            if let Some(&ts) = last.get(&notification.alert_key) {
                let elapsed = (Utc::now() - ts).num_seconds();
                if elapsed < self.cooldown_secs {
                    debug!(
                        key = %notification.alert_key,
                        elapsed_secs = elapsed,
                        cooldown_secs = self.cooldown_secs,
                        "Alert suppressed by cooldown"
                    );
                    return Ok(false);
                }
            }
        }

        let envelope = DispatchedNotification {
            id: Uuid::new_v4(),
            notification,
            dispatched_at: Utc::now(),
        };

        // --- update deduplication timestamp ---
        self.last_dispatched.write().await.insert(
            envelope.notification.alert_key.clone(),
            envelope.dispatched_at,
        );

        // --- in-memory queue ---
        self.queue.write().await.push(envelope.clone());

        info!(
            id = %envelope.id,
            key = %envelope.notification.alert_key,
            level = %envelope.notification.level,
            title = %envelope.notification.title,
            "Alert notification dispatched"
        );

        // --- Redis pub/sub (best-effort) ---
        if let Some(redis) = &self.redis {
            let payload = serde_json::to_string(&envelope)?;
            match redis.get_multiplexed_async_connection().await {
                Ok(mut conn) => {
                    let result: Result<i64, _> = conn.publish(&self.redis_channel, &payload).await;
                    match result {
                        Ok(receivers) => {
                            debug!(
                                channel = %self.redis_channel,
                                receivers = receivers,
                                "Published alert to Redis"
                            );
                        }
                        Err(e) => {
                            warn!(error = %e, "Failed to publish alert to Redis");
                        }
                    }
                }
                Err(e) => {
                    warn!(error = %e, "Failed to connect to Redis for alert publish");
                }
            }
        }

        Ok(true)
    }

    /// Dispatch a notification derived from a fired [`Alert`].
    ///
    /// Only dispatches if the alert's severity is [`AlertSeverity::Critical`].
    /// Returns `Ok(false)` for non-critical alerts or when suppressed by
    /// cooldown.
    pub async fn dispatch_alert(&self, alert: &Alert) -> Result<bool, AlertDispatchError> {
        if alert.severity != AlertSeverity::Critical {
            debug!(
                alert_id = %alert.id,
                severity = %alert.severity,
                "Skipping non-critical alert"
            );
            return Ok(false);
        }

        error!(
            alert_id = %alert.id,
            rule_name = %alert.rule_name,
            match_count = alert.match_count,
            "Critical alert detected — dispatching notification"
        );

        let mut metadata = HashMap::new();
        metadata.insert("rule_id".to_string(), alert.rule_id.to_string());
        metadata.insert("rule_name".to_string(), alert.rule_name.clone());
        metadata.insert("match_count".to_string(), alert.match_count.to_string());

        self.dispatch(AlertNotification {
            alert_key: format!("rule:{}", alert.rule_id),
            level: NotificationLevel::from(&alert.severity),
            title: format!("Critical alert: {}", alert.rule_name),
            message: format!(
                "Rule '{}' fired {} times within the evaluation window.",
                alert.rule_name, alert.match_count
            ),
            metadata,
        })
        .await
    }

    /// Drain and return all pending in-memory notifications, clearing the queue.
    pub async fn drain_notifications(&self) -> Vec<DispatchedNotification> {
        let mut queue = self.queue.write().await;
        std::mem::take(&mut *queue)
    }

    /// Peek at pending notifications without clearing the queue.
    pub async fn pending_count(&self) -> usize {
        self.queue.read().await.len()
    }

    /// Reset the deduplication state (useful for testing).
    pub async fn reset_cooldowns(&self) {
        self.last_dispatched.write().await.clear();
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::log_alerts::AlertSeverity;

    fn make_notification(key: &str, level: NotificationLevel) -> AlertNotification {
        AlertNotification {
            alert_key: key.to_string(),
            level,
            title: format!("Test alert: {key}"),
            message: "Something went wrong".to_string(),
            metadata: HashMap::new(),
        }
    }

    fn make_alert(severity: AlertSeverity) -> Alert {
        Alert {
            id: Uuid::new_v4(),
            rule_id: Uuid::new_v4(),
            rule_name: "test-rule".to_string(),
            severity,
            match_count: 5,
            fired_at: Utc::now(),
            acknowledged: false,
        }
    }

    fn dispatcher() -> AlertDispatcher {
        // No Redis — tests run without a live server.
        AlertDispatcher::new(None, 60)
    }

    // --- dispatch ---

    #[tokio::test]
    async fn test_dispatch_adds_to_queue() {
        let d = dispatcher();
        let dispatched = d
            .dispatch(make_notification("key1", NotificationLevel::Critical))
            .await
            .unwrap();
        assert!(dispatched);
        assert_eq!(d.pending_count().await, 1);
    }

    #[tokio::test]
    async fn test_dispatch_deduplication_within_cooldown() {
        let d = dispatcher();
        d.dispatch(make_notification("key1", NotificationLevel::Critical))
            .await
            .unwrap();
        let second = d
            .dispatch(make_notification("key1", NotificationLevel::Critical))
            .await
            .unwrap();
        assert!(!second, "second dispatch should be suppressed by cooldown");
        assert_eq!(d.pending_count().await, 1);
    }

    #[tokio::test]
    async fn test_dispatch_different_keys_not_deduplicated() {
        let d = dispatcher();
        d.dispatch(make_notification("key1", NotificationLevel::Critical))
            .await
            .unwrap();
        d.dispatch(make_notification("key2", NotificationLevel::Critical))
            .await
            .unwrap();
        assert_eq!(d.pending_count().await, 2);
    }

    #[tokio::test]
    async fn test_dispatch_after_cooldown_reset() {
        let d = AlertDispatcher::new(None, 60);
        d.dispatch(make_notification("key1", NotificationLevel::Warning))
            .await
            .unwrap();
        d.reset_cooldowns().await;
        let second = d
            .dispatch(make_notification("key1", NotificationLevel::Warning))
            .await
            .unwrap();
        assert!(second, "should dispatch after cooldown reset");
        assert_eq!(d.pending_count().await, 2);
    }

    #[tokio::test]
    async fn test_drain_clears_queue() {
        let d = dispatcher();
        d.dispatch(make_notification("k1", NotificationLevel::Info))
            .await
            .unwrap();
        d.dispatch(make_notification("k2", NotificationLevel::Warning))
            .await
            .unwrap();

        let drained = d.drain_notifications().await;
        assert_eq!(drained.len(), 2);
        assert_eq!(d.pending_count().await, 0);
    }

    #[tokio::test]
    async fn test_drain_returns_correct_payload() {
        let d = dispatcher();
        d.dispatch(make_notification("my-key", NotificationLevel::Critical))
            .await
            .unwrap();

        let mut drained = d.drain_notifications().await;
        assert_eq!(drained.len(), 1);
        let n = drained.remove(0);
        assert_eq!(n.notification.alert_key, "my-key");
        assert_eq!(n.notification.level, NotificationLevel::Critical);
        assert!(!n.id.is_nil());
    }

    // --- dispatch_alert ---

    #[tokio::test]
    async fn test_dispatch_alert_critical_is_dispatched() {
        let d = dispatcher();
        let alert = make_alert(AlertSeverity::Critical);
        let dispatched = d.dispatch_alert(&alert).await.unwrap();
        assert!(dispatched);
        assert_eq!(d.pending_count().await, 1);
    }

    #[tokio::test]
    async fn test_dispatch_alert_warning_is_skipped() {
        let d = dispatcher();
        let alert = make_alert(AlertSeverity::Warning);
        let dispatched = d.dispatch_alert(&alert).await.unwrap();
        assert!(!dispatched);
        assert_eq!(d.pending_count().await, 0);
    }

    #[tokio::test]
    async fn test_dispatch_alert_info_is_skipped() {
        let d = dispatcher();
        let alert = make_alert(AlertSeverity::Info);
        let dispatched = d.dispatch_alert(&alert).await.unwrap();
        assert!(!dispatched);
        assert_eq!(d.pending_count().await, 0);
    }

    #[tokio::test]
    async fn test_dispatch_alert_metadata_populated() {
        let d = dispatcher();
        let alert = make_alert(AlertSeverity::Critical);
        d.dispatch_alert(&alert).await.unwrap();

        let mut drained = d.drain_notifications().await;
        let n = drained.remove(0);
        assert!(n.notification.metadata.contains_key("rule_name"));
        assert!(n.notification.metadata.contains_key("match_count"));
        assert_eq!(
            n.notification.metadata["match_count"],
            alert.match_count.to_string()
        );
    }

    #[tokio::test]
    async fn test_dispatch_alert_deduplication_by_rule_id() {
        let d = dispatcher();
        let alert = make_alert(AlertSeverity::Critical);
        d.dispatch_alert(&alert).await.unwrap();
        // Same alert (same rule_id) — should be suppressed.
        let second = d.dispatch_alert(&alert).await.unwrap();
        assert!(!second);
        assert_eq!(d.pending_count().await, 1);
    }

    // --- NotificationLevel ---

    #[test]
    fn test_notification_level_from_severity() {
        assert_eq!(
            NotificationLevel::from(&AlertSeverity::Critical),
            NotificationLevel::Critical
        );
        assert_eq!(
            NotificationLevel::from(&AlertSeverity::Warning),
            NotificationLevel::Warning
        );
        assert_eq!(
            NotificationLevel::from(&AlertSeverity::Info),
            NotificationLevel::Info
        );
    }

    #[test]
    fn test_notification_level_display() {
        assert_eq!(NotificationLevel::Critical.to_string(), "critical");
        assert_eq!(NotificationLevel::Warning.to_string(), "warning");
        assert_eq!(NotificationLevel::Info.to_string(), "info");
    }

    // --- error display ---

    #[test]
    fn test_dispatch_error_display() {
        let e = AlertDispatchError::Serialisation(
            serde_json::from_str::<serde_json::Value>("bad").unwrap_err(),
        );
        assert!(!e.to_string().is_empty());
    }

    // --- zero cooldown ---

    #[tokio::test]
    async fn test_zero_cooldown_never_deduplicates() {
        let d = AlertDispatcher::new(None, 0);
        d.dispatch(make_notification("k", NotificationLevel::Critical))
            .await
            .unwrap();
        let second = d
            .dispatch(make_notification("k", NotificationLevel::Critical))
            .await
            .unwrap();
        assert!(second, "zero cooldown should never suppress");
        assert_eq!(d.pending_count().await, 2);
    }

    // --- with_channel ---

    #[test]
    fn test_with_channel() {
        let d = AlertDispatcher::new(None, 30).with_channel("my-alerts");
        assert_eq!(d.redis_channel, "my-alerts");
    }

    // --- serialisation roundtrip ---

    #[test]
    fn test_dispatched_notification_roundtrip() {
        let n = DispatchedNotification {
            id: Uuid::new_v4(),
            notification: make_notification("k", NotificationLevel::Critical),
            dispatched_at: Utc::now(),
        };
        let json = serde_json::to_string(&n).unwrap();
        let back: DispatchedNotification = serde_json::from_str(&json).unwrap();
        assert_eq!(back.notification.alert_key, "k");
        assert_eq!(back.notification.level, NotificationLevel::Critical);
    }
}
