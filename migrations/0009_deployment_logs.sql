-- M4: Deployment logs and log segments schema

CREATE TABLE IF NOT EXISTS deployment_logs (
    id UUID PRIMARY KEY,
    deployment_id UUID NOT NULL REFERENCES deployments(id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    sequence BIGINT NOT NULL,
    stream VARCHAR(16) NOT NULL,
    message TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    CONSTRAINT deployment_logs_deployment_sequence_key UNIQUE (deployment_id, sequence)
);

CREATE INDEX IF NOT EXISTS idx_deployment_logs_lookup ON deployment_logs(deployment_id, sequence ASC);
CREATE INDEX IF NOT EXISTS idx_deployment_logs_created_at ON deployment_logs(created_at);
CREATE INDEX IF NOT EXISTS idx_deployment_logs_project_id ON deployment_logs(project_id);

CREATE TABLE IF NOT EXISTS log_segments (
    id UUID PRIMARY KEY,
    deployment_id UUID NOT NULL REFERENCES deployments(id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    start_sequence BIGINT NOT NULL,
    end_sequence BIGINT NOT NULL,
    storage_path TEXT NOT NULL,
    size_bytes BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_log_segments_deployment ON log_segments(deployment_id, start_sequence ASC);
