-- M4: Build Cache and Observability Schema

CREATE TABLE IF NOT EXISTS build_cache (
    id UUID PRIMARY KEY,
    cache_key TEXT UNIQUE NOT NULL,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    artifact_checksum TEXT NOT NULL,
    size_bytes BIGINT NOT NULL,
    storage_path TEXT NOT NULL,
    toolchain TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL,
    last_used_at TIMESTAMPTZ NOT NULL,
    is_invalidated BOOLEAN NOT NULL DEFAULT FALSE
);

CREATE INDEX IF NOT EXISTS idx_build_cache_project_id ON build_cache(project_id);
CREATE INDEX IF NOT EXISTS idx_build_cache_key ON build_cache(cache_key);
