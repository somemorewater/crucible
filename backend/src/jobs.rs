use serde::{Deserialize, Serialize};
use tracing::{info, instrument};
use crate::services::tracing::TracingService;

#[derive(Debug, Serialize, Deserialize)]
pub struct TransactionMonitorJob {
    pub tx_hash: String,
}

/// Handler for monitoring Stellar transactions.
/// Returning () since Apalis 0.6 handlers can return ().
pub async fn monitor_transaction(job: TransactionMonitorJob) {
#[instrument(skip_all, fields(job.name = "monitor_transaction", job.id = %job.tx_hash))]
pub async fn monitor_transaction(
    job: TransactionMonitorJob,
) {
    let span = TracingService::job_span("monitor_transaction", &job.tx_hash);
    let _enter = span.enter();
    
    info!("Monitoring Stellar transaction: {}", job.tx_hash);
    tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
    
    info!("Transaction monitoring completed: {}", job.tx_hash);
}
