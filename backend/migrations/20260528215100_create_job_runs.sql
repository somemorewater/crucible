-- Create enum for job run status
CREATE TYPE job_run_status AS ENUM ('running', 'succeeded', 'failed', 'timed_out');

-- Create job_runs table
CREATE TABLE job_runs (
    id UUID PRIMARY KEY,
    job_name VARCHAR(255) NOT NULL,
    started_at TIMESTAMPTZ NOT NULL,
    finished_at TIMESTAMPTZ,
    status job_run_status NOT NULL,
    error_message TEXT,
    duration_ms BIGINT
);

-- Index on (job_name, started_at DESC) for efficient history queries
CREATE INDEX idx_job_runs_name_started_desc ON job_runs (job_name, started_at DESC);

-- Index on started_at for cleanup queries
CREATE INDEX idx_job_runs_started_at ON job_runs (started_at);
