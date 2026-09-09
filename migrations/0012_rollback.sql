-- M5: Rollback tracking on deployments

ALTER TABLE deployments ADD COLUMN IF NOT EXISTS cause TEXT NOT NULL DEFAULT 'deploy';
ALTER TABLE deployments ADD COLUMN IF NOT EXISTS rollback_from_deployment_id UUID REFERENCES deployments(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_deployments_cause ON deployments(cause);

