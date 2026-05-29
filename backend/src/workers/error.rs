//! SCHEDULER APPROACH: Option B — Custom Tokio interval loop with cron crate
//! Rationale: We require strict control over execution guarantees such as distributed locking, timeouts, retries, and history logging. A custom loop using `tokio::time::sleep_until` and the `cron` crate allows us to wrap the execution precisely in a Redis lock and accurately handle timeouts using `tokio::time::timeout`.

use thiserror::Error;

#[derive(Debug, Error)]
pub enum JobError {
    #[error("Database error: {0}")]
    DatabaseError(#[from] sqlx::Error),

    #[error("Redis error: {0}")]
    RedisError(#[from] redis::RedisError),

    #[error("Job execution timed out")]
    Timeout,

    #[error("Job handler panicked: {0}")]
    HandlerPanic(String),

    #[error("Failed to acquire distributed lock")]
    LockAcquisitionFailed,
}

#[derive(Debug, Error)]
pub enum SchedulerError {
    #[error("Scheduler has already been started")]
    AlreadyStarted,

    #[error("Scheduler shutdown timed out")]
    ShutdownTimeout,

    #[error("Failed to register job: {0}")]
    JobRegistrationFailed(String),
}
