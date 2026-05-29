//! SCHEDULER APPROACH: Option B — Custom Tokio interval loop with cron crate
//! Rationale: We require strict control over execution guarantees such as distributed locking, timeouts, retries, and history logging. A custom loop using `tokio::time::sleep_until` and the `cron` crate allows us to wrap the execution precisely in a Redis lock and accurately handle timeouts using `tokio::time::timeout`.

pub mod error;
pub mod job_history;
pub mod jobs;
pub mod scheduler;

pub mod cache_warm;
pub mod progress;
pub mod health;
pub mod executor;

#[cfg(test)]
mod tests;

pub use cache_warm::CacheWarmWorker;
pub use progress::JobProgressTracker;
pub use health::WorkerHealthMonitor;
pub use executor::TaskExecutor;
