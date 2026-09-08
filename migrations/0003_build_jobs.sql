-- Update deployments status constraint to include queued and cancelled
ALTER TABLE deployments DROP CONSTRAINT IF EXISTS deployments_status_check;
ALTER TABLE deployments ADD CONSTRAINT deployments_status_check CHECK (status IN ('pending', 'queued', 'building', 'running', 'failed', 'stopped', 'cancelled'));

-- Build Jobs
CREATE TABLE IF NOT EXISTS build_jobs (
    id UUID PRIMARY KEY,
    deployment_id UUID NOT NULL REFERENCES deployments(id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    queue_name TEXT NOT NULL DEFAULT 'build_jobs',
    priority TEXT NOT NULL CHECK (priority IN ('production', 'preview')),
    status TEXT NOT NULL CHECK (status IN ('queued', 'running', 'completed', 'failed', 'cancelled')),
    attempt INT NOT NULL DEFAULT 0,
    max_attempts INT NOT NULL DEFAULT 3,
    lease_owner TEXT,
    lease_expires_at TIMESTAMPTZ,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    UNIQUE(deployment_id)
);
ALTER TABLE build_jobs ADD COLUMN IF NOT EXISTS queue_name TEXT NOT NULL DEFAULT 'build_jobs';
CREATE INDEX IF NOT EXISTS idx_build_jobs_queue_status ON build_jobs(queue_name, status, priority, created_at);
CREATE INDEX IF NOT EXISTS idx_build_jobs_status_priority ON build_jobs(status, priority, created_at);
CREATE INDEX IF NOT EXISTS idx_build_jobs_lease_expires ON build_jobs(lease_expires_at) WHERE status = 'running';

-- Build Attempts
CREATE TABLE IF NOT EXISTS build_attempts (
    id UUID PRIMARY KEY,
    job_id UUID NOT NULL REFERENCES build_jobs(id) ON DELETE CASCADE,
    attempt_number INT NOT NULL,
    worker_id TEXT NOT NULL,
    status TEXT NOT NULL CHECK (status IN ('running', 'succeeded', 'failed', 'cancelled', 'timed_out')),
    error_message TEXT,
    started_at TIMESTAMPTZ NOT NULL,
    finished_at TIMESTAMPTZ,
    UNIQUE(job_id, attempt_number)
);
CREATE INDEX IF NOT EXISTS idx_build_attempts_job ON build_attempts(job_id);

-- Build Events
CREATE TABLE IF NOT EXISTS build_events (
    id UUID PRIMARY KEY,
    job_id UUID NOT NULL REFERENCES build_jobs(id) ON DELETE CASCADE,
    deployment_id UUID NOT NULL REFERENCES deployments(id) ON DELETE CASCADE,
    event_type TEXT NOT NULL,
    payload JSONB NOT NULL DEFAULT '{}'::jsonb,
    created_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_build_events_job_created ON build_events(job_id, created_at);
