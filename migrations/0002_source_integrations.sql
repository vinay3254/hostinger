-- Provider Connections
CREATE TABLE IF NOT EXISTS provider_connections (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider TEXT NOT NULL CHECK (provider IN ('github', 'gitlab')),
    external_user_id TEXT NOT NULL,
    access_token_encrypted TEXT NOT NULL,
    refresh_token_encrypted TEXT,
    expires_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    UNIQUE(user_id, provider)
);
CREATE INDEX IF NOT EXISTS idx_provider_connections_user ON provider_connections(user_id);

-- Provider Repositories
CREATE TABLE IF NOT EXISTS provider_repositories (
    id UUID PRIMARY KEY,
    connection_id UUID NOT NULL REFERENCES provider_connections(id) ON DELETE CASCADE,
    external_id TEXT NOT NULL,
    full_name TEXT NOT NULL,
    clone_url TEXT NOT NULL,
    default_branch TEXT NOT NULL,
    synced_at TIMESTAMPTZ NOT NULL,
    UNIQUE(connection_id, external_id)
);
CREATE INDEX IF NOT EXISTS idx_provider_repositories_conn ON provider_repositories(connection_id);

-- OAuth States
CREATE TABLE IF NOT EXISTS oauth_states (
    state TEXT PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    redirect_url TEXT,
    expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_oauth_states_user ON oauth_states(user_id);

-- Provider Deliveries
CREATE TABLE IF NOT EXISTS provider_deliveries (
    id UUID PRIMARY KEY,
    provider TEXT NOT NULL,
    delivery_id TEXT NOT NULL,
    project_id UUID REFERENCES projects(id) ON DELETE CASCADE,
    created_at TIMESTAMPTZ NOT NULL,
    UNIQUE(provider, delivery_id)
);
CREATE INDEX IF NOT EXISTS idx_provider_deliveries_delivery ON provider_deliveries(provider, delivery_id);

-- Source Events
CREATE TABLE IF NOT EXISTS source_events (
    id UUID PRIMARY KEY,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    delivery_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    commit_sha TEXT NOT NULL,
    branch TEXT,
    pull_request JSONB,
    idempotency_key TEXT NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_source_events_project_created ON source_events(project_id, created_at);

-- Project Source Connection
ALTER TABLE projects ADD COLUMN IF NOT EXISTS repository_id UUID REFERENCES provider_repositories(id) ON DELETE SET NULL;
ALTER TABLE projects ADD COLUMN IF NOT EXISTS webhook_secret TEXT;
ALTER TABLE projects ADD COLUMN IF NOT EXISTS target_branch TEXT DEFAULT 'main';
