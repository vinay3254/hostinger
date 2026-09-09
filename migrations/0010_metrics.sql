-- M4: Metric samples and rollups schema

CREATE TABLE IF NOT EXISTS metric_samples (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    deployment_id UUID REFERENCES deployments(id) ON DELETE SET NULL,
    environment VARCHAR(32) NOT NULL DEFAULT 'production',
    metric_name VARCHAR(64) NOT NULL,
    value DOUBLE PRECISION NOT NULL,
    unit VARCHAR(32) NOT NULL,
    recorded_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_metric_samples_query 
    ON metric_samples (project_id, metric_name, environment, recorded_at ASC);

CREATE INDEX IF NOT EXISTS idx_metric_samples_deployment 
    ON metric_samples (deployment_id, recorded_at ASC);

CREATE TABLE IF NOT EXISTS metric_rollups (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    environment VARCHAR(32) NOT NULL DEFAULT 'production',
    metric_name VARCHAR(64) NOT NULL,
    resolution VARCHAR(16) NOT NULL,
    bucket_start TIMESTAMPTZ NOT NULL,
    sample_count BIGINT NOT NULL,
    sample_sum DOUBLE PRECISION NOT NULL,
    sample_min DOUBLE PRECISION NOT NULL,
    sample_max DOUBLE PRECISION NOT NULL,
    p50 DOUBLE PRECISION,
    p95 DOUBLE PRECISION,
    p99 DOUBLE PRECISION,
    is_partial BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT uq_metric_rollups UNIQUE (project_id, environment, metric_name, resolution, bucket_start)
);

CREATE INDEX IF NOT EXISTS idx_metric_rollups_query 
    ON metric_rollups (project_id, metric_name, environment, resolution, bucket_start ASC);
