-- Build system metrics table
CREATE TABLE IF NOT EXISTS build_metrics (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    project_name TEXT NOT NULL,
    build_id TEXT NOT NULL,
    build_status TEXT NOT NULL,
    compilation_time_ms BIGINT NOT NULL,
    dependency_count INTEGER NOT NULL,
    cache_hit_rate DECIMAL(5,2),
    cpu_usage DECIMAL(5,2),
    memory_usage_mb BIGINT,
    build_timestamp TIMESTAMP WITH TIME ZONE NOT NULL,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_build_metrics_project ON build_metrics(project_name);
CREATE INDEX IF NOT EXISTS idx_build_metrics_timestamp ON build_metrics(build_timestamp);
CREATE INDEX IF NOT EXISTS idx_build_metrics_status ON build_metrics(build_status);
