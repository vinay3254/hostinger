-- M5: Releases, active routes, and transition records schema

CREATE TABLE IF NOT EXISTS releases (
    id UUID PRIMARY KEY,
    deployment_id UUID NOT NULL REFERENCES deployments(id) ON DELETE CASCADE,
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    environment VARCHAR(32) NOT NULL DEFAULT 'production',
    status VARCHAR(32) NOT NULL DEFAULT 'starting',
    version INT NOT NULL DEFAULT 1,
    container_id VARCHAR(64),
    port INT,
    url TEXT,
    created_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_releases_project_env 
    ON releases (project_id, environment, status);

CREATE INDEX IF NOT EXISTS idx_releases_deployment 
    ON releases (deployment_id);

CREATE TABLE IF NOT EXISTS active_routes (
    project_id UUID NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    environment VARCHAR(32) NOT NULL DEFAULT 'production',
    active_release_id UUID NOT NULL REFERENCES releases(id) ON DELETE RESTRICT,
    route_target TEXT NOT NULL,
    activated_at TIMESTAMPTZ NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL,
    PRIMARY KEY (project_id, environment)
);

CREATE TABLE IF NOT EXISTS release_transitions (
    id UUID PRIMARY KEY,
    release_id UUID NOT NULL REFERENCES releases(id) ON DELETE CASCADE,
    from_status VARCHAR(32) NOT NULL,
    to_status VARCHAR(32) NOT NULL,
    reason TEXT,
    created_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_release_transitions_release 
    ON release_transitions (release_id, created_at ASC);
