//! SCHEDULER APPROACH: Option B — Custom Tokio interval loop with cron crate
//! Rationale: We require strict control over execution guarantees such as distributed locking, timeouts, retries, and history logging. A custom loop using `tokio::time::sleep_until` and the `cron` crate allows us to wrap the execution precisely in a Redis lock and accurately handle timeouts using `tokio::time::timeout`.

use async_trait::async_trait;
use tracing::{info, instrument};

use crate::workers::error::JobError;
use crate::workers::job_history::cleanup_old_runs;
use crate::workers::scheduler::{JobContext, JobHandler};

/// A job that tests database and Redis connectivity.
pub struct HealthCheckJob;

#[async_trait]
impl JobHandler for HealthCheckJob {
    #[instrument(skip_all, fields(job_name = ctx.job_name))]
    async fn run(&self, ctx: JobContext) -> Result<(), JobError> {
        info!("Running health check job");

        // Ping Database
        sqlx::query("SELECT 1")
            .execute(&ctx.pool)
            .await
            .map_err(JobError::DatabaseError)?;
        info!("PostgreSQL connection is healthy");

        // Ping Redis
        let mut redis_conn = ctx.redis.get_async_connection().await.map_err(JobError::RedisError)?;
        redis::cmd("PING")
            .query_async::<_, String>(&mut redis_conn)
            .await
            .map_err(JobError::RedisError)?;
        info!("Redis connection is healthy");

        Ok(())
    }
}

/// A job that cleans up old job history runs.
pub struct CleanupJob {
    pub retain_days: i64,
}

#[async_trait]
impl JobHandler for CleanupJob {
    #[instrument(skip_all, fields(job_name = ctx.job_name))]
    async fn run(&self, ctx: JobContext) -> Result<(), JobError> {
        info!("Running job history cleanup job (retention: {} days)", self.retain_days);

        let deleted_count = cleanup_old_runs(&ctx.pool, self.retain_days).await?;
        info!("Cleaned up {} old job run records", deleted_count);

        Ok(())
    }
}
