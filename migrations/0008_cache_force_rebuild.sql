-- M4: Scope cache to project and add force_rebuild column
ALTER TABLE build_cache DROP CONSTRAINT IF EXISTS build_cache_cache_key_key;
ALTER TABLE build_cache ADD CONSTRAINT build_cache_project_cache_key_key UNIQUE (project_id, cache_key);
ALTER TABLE build_jobs ADD COLUMN IF NOT EXISTS force_rebuild BOOLEAN NOT NULL DEFAULT FALSE;
