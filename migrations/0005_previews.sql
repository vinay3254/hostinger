-- Previews
CREATE TABLE IF NOT EXISTS previews (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    provider TEXT NOT NULL CHECK (provider IN ('github', 'gitlab')),
    pr_number BIGINT NOT NULL,
    head_sha TEXT NOT NULL,
    base_branch TEXT NOT NULL,
    head_branch TEXT NOT NULL,
    deployment_id UUID REFERENCES deployments(id) ON DELETE SET NULL,
    hostname TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('building', 'ready', 'failed', 'closed')),
    closed_at TIMESTAMPTZ,
    cleanup_attempt INT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    UNIQUE(project_id, provider, pr_number)
);

CREATE INDEX IF NOT EXISTS idx_previews_project ON previews(project_id);
CREATE INDEX IF NOT EXISTS idx_previews_status ON previews(status);
CREATE INDEX IF NOT EXISTS idx_previews_hostname ON previews(hostname);
