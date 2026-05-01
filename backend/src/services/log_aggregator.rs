#![allow(dead_code)]
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tracing::{debug, info};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: DateTime<Utc>,
    pub level: String,
    pub message: String,
    pub service: String,
}

pub struct LogAggregator {
    sender: mpsc::Sender<LogEntry>,
}

impl LogAggregator {
    pub fn new() -> (Self, mpsc::Receiver<LogEntry>) {
        let (tx, rx) = mpsc::channel(100);
        (Self { sender: tx }, rx)
    }

    pub async fn log(&self, level: &str, message: &str, service: &str) -> anyhow::Result<()> {
        let entry = LogEntry {
            timestamp: Utc::now(),
            level: level.to_string(),
            message: message.to_string(),
            service: service.to_string(),
        };

        debug!(entry = ?entry, "Aggregating log entry");
        self.sender.send(entry).await?;
        Ok(())
    }

    pub async fn run_worker(mut receiver: mpsc::Receiver<LogEntry>) {
        info!("Starting log aggregator worker");
        while let Some(entry) = receiver.recv().await {
            // In a real implementation, this would write to SQLx or Redis
            // For now, we'll just log it via tracing
            info!(
                target: "log_aggregator",
                timestamp = %entry.timestamp,
                level = %entry.level,
                service = %entry.service,
                message = %entry.message,
                "Persisted log entry"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_log_aggregation() {
        let (aggregator, mut receiver) = LogAggregator::new();

        aggregator
            .log("INFO", "Test message", "test_service")
            .await
            .unwrap();

        let entry = receiver.recv().await.unwrap();
        assert_eq!(entry.level, "INFO");
        assert_eq!(entry.message, "Test message");
        assert_eq!(entry.service, "test_service");
    }
}
