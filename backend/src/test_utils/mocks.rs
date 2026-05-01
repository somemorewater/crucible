use crate::api::handlers::profiling::MetricsReport;
use async_trait::async_trait;
use mockall::automock;

/// Mockable interface for database operations.
#[automock]
#[async_trait]
pub trait DatabaseService: Send + Sync {
    async fn get_user_count(&self) -> Result<i64, anyhow::Error>;
    async fn get_health_status(&self) -> bool;
    async fn get_transaction_by_hash(&self, hash: &str) -> Result<Option<String>, anyhow::Error>;
}

/// Mockable interface for caching operations.
#[automock]
#[async_trait]
pub trait CacheService: Send + Sync {
    async fn get_performance_metrics(&self) -> Result<MetricsReport, anyhow::Error>;
    async fn set_metric(&self, key: &str, value: f64) -> Result<(), anyhow::Error>;
    async fn invalidate_cache(&self, key: &str) -> Result<(), anyhow::Error>;
}

/// Mockable interface for Stellar-specific transaction submission.
#[automock]
#[async_trait]
pub trait StellarService: Send + Sync {
    async fn submit_transaction(&self, xdr: &str) -> Result<String, anyhow::Error>;
    async fn get_account_balance(&self, address: &str) -> Result<i128, anyhow::Error>;
}

/// Centralized Mock Context for unit testing Axum handlers.
pub struct MockContext {
    pub db: MockDatabaseService,
    pub cache: MockCacheService,
    pub stellar: MockStellarService,
}

impl MockContext {
    pub fn new() -> Self {
        Self {
            db: MockDatabaseService::new(),
            cache: MockCacheService::new(),
            stellar: MockStellarService::new(),
        }
    }
}

impl Default for MockContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_stellar_service() {
        let mut mock_stellar = MockStellarService::new();
        mock_stellar
            .expect_submit_transaction()
            .with(mockall::predicate::eq("AAAA..."))
            .times(1)
            .returning(|_| Ok("tx_hash_123".to_string()));

        let result = mock_stellar.submit_transaction("AAAA...").await.unwrap();
        assert_eq!(result, "tx_hash_123");
    }
}
