use sqlx::PgPool;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use chrono::{DateTime, Utc};
use tracing::{info, debug, instrument};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CoverageError {
    #[error("Database error: {0}")]
    Database(#[from] sqlx::Error),
    #[error("Redis error: {0}")]
    Redis(#[from] redis::RedisError),
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    #[error("Project not found: {0}")]
    NotFound(String),
}

#[derive(Debug, Serialize, Deserialize, sqlx::FromRow, Clone)]
pub struct TestCoverage {
    pub id: Uuid,
    pub project_name: String,
    pub branch: String,
    pub commit_sha: String,
    pub coverage_percent: f64,
    pub total_lines: i32,
    pub covered_lines: i32,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct NewTestCoverage {
    pub project_name: String,
    pub branch: String,
    pub commit_sha: String,
    pub coverage_percent: f64,
    pub total_lines: i32,
    pub covered_lines: i32,
}

#[derive(Clone)]
pub struct TestCoverageService {
    db: PgPool,
    redis: redis::Client,
}

impl TestCoverageService {
    pub fn new(db: PgPool, redis: redis::Client) -> Self {
        Self { db, redis }
    }

    #[instrument(skip(self, data), fields(project = %data.project_name))]
    pub async fn submit_coverage(&self, data: NewTestCoverage) -> Result<TestCoverage, CoverageError> {
        info!("Persisting coverage report");

        let coverage = sqlx::query_as::<_, TestCoverage>(
            r#"
            INSERT INTO test_coverage (id, project_name, branch, commit_sha, coverage_percent, total_lines, covered_lines, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            RETURNING *
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(&data.project_name)
        .bind(&data.branch)
        .bind(&data.commit_sha)
        .bind(data.coverage_percent)
        .bind(data.total_lines)
        .bind(data.covered_lines)
        .bind(Utc::now())
        .fetch_one(&self.db)
        .await?;

        // Cache the latest report for this project in Redis (1 hour TTL)
        let cache_key = format!("coverage:latest:{}", data.project_name);
        let mut conn = self.redis.get_multiplexed_async_connection().await?;
        let serialized = serde_json::to_string(&coverage)?;
        let _: () = conn.set_ex(cache_key, serialized, 3600).await?;

        debug!("Updated latest coverage cache for {}", data.project_name);
        Ok(coverage)
    }

    #[instrument(skip(self))]
    pub async fn get_latest_coverage(&self, project_name: &str) -> Result<TestCoverage, CoverageError> {
        let cache_key = format!("coverage:latest:{}", project_name);
        let mut conn = self.redis.get_multiplexed_async_connection().await?;
        
        // Try cache
        if let Some(cached): Option<String> = conn.get(&cache_key).await? {
            debug!(project = %project_name, "Coverage cache hit");
            return Ok(serde_json::from_str(&cached)?);
        }

        // Fallback to DB
        debug!(project = %project_name, "Coverage cache miss - querying database");
        let report = sqlx::query_as::<_, TestCoverage>(
            "SELECT * FROM test_coverage WHERE project_name = $1 ORDER BY created_at DESC LIMIT 1"
        )
        .bind(project_name)
        .fetch_optional(&self.db)
        .await?;

        match report {
            Some(r) => {
                let serialized = serde_json::to_string(&r)?;
                let _: () = conn.set_ex(cache_key, serialized, 3600).await?;
                Ok(r)
            },
            None => Err(CoverageError::NotFound(project_name.to_string())),
        }
    }
}