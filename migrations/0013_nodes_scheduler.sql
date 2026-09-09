CREATE TABLE IF NOT EXISTS nodes (
    id UUID PRIMARY KEY,
    hostname VARCHAR(255) NOT NULL,
    endpoint VARCHAR(255) NOT NULL,
    status VARCHAR(32) NOT NULL DEFAULT 'online',
    cpu_total_millicores BIGINT NOT NULL,
    memory_total_bytes BIGINT NOT NULL,
    max_releases INTEGER NOT NULL,
    cpu_used_millicores BIGINT NOT NULL DEFAULT 0,
    memory_used_bytes BIGINT NOT NULL DEFAULT 0,
    running_releases_count INTEGER NOT NULL DEFAULT 0,
    token_hash VARCHAR(128) NOT NULL,
    epoch BIGINT NOT NULL DEFAULT 1,
    version BIGINT NOT NULL DEFAULT 1,
    is_draining BOOLEAN NOT NULL DEFAULT FALSE,
    is_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    last_heartbeat_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    registered_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS node_events (
    id UUID PRIMARY KEY,
    node_id UUID NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    from_status VARCHAR(32) NOT NULL,
    to_status VARCHAR(32) NOT NULL,
    reason TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS placements (
    id UUID PRIMARY KEY,
    release_id UUID NOT NULL,
    node_id UUID NOT NULL REFERENCES nodes(id) ON DELETE CASCADE,
    operation_lease_id UUID NOT NULL,
    status VARCHAR(32) NOT NULL DEFAULT 'placed',
    cpu_allocated_millicores BIGINT NOT NULL,
    memory_allocated_bytes BIGINT NOT NULL,
    lease_expires_at TIMESTAMPTZ NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_nodes_status_placement ON nodes(status, is_enabled, is_draining);
CREATE INDEX IF NOT EXISTS idx_nodes_last_heartbeat ON nodes(last_heartbeat_at);
CREATE INDEX IF NOT EXISTS idx_node_events_node ON node_events(node_id, created_at);
CREATE INDEX IF NOT EXISTS idx_placements_release ON placements(release_id);
CREATE INDEX IF NOT EXISTS idx_placements_node ON placements(node_id, status);
