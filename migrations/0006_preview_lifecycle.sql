-- Add target and env_scope to build_jobs and deployments
ALTER TABLE build_jobs ADD COLUMN IF NOT EXISTS target TEXT NOT NULL DEFAULT 'production';
ALTER TABLE build_jobs ADD COLUMN IF NOT EXISTS env_scope TEXT NOT NULL DEFAULT 'production';
ALTER TABLE deployments ADD COLUMN IF NOT EXISTS target TEXT NOT NULL DEFAULT 'production';
