use crate::agent_protocol::{
    HeartbeatCommand, HeartbeatResponse, NodeCapacity, NodeStatus, RegisterNodeCommand,
    RegisterNodeResponse,
};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use std::time::Duration;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug)]
pub enum NodeRegistryError {
    Database(sqlx::Error),
    InvalidCapacity(String),
    NodeNotFound(Uuid),
    EpochMismatch { current: u64, provided: u64 },
    VersionConflict(Uuid),
}

impl std::fmt::Display for NodeRegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(e) => write!(f, "Database error: {}", e),
            Self::InvalidCapacity(msg) => write!(f, "Invalid capacity specified: {}", msg),
            Self::NodeNotFound(id) => write!(f, "Node not found: {}", id),
            Self::EpochMismatch { current, provided } => {
                write!(
                    f,
                    "Epoch mismatch: current {}, provided {}",
                    current, provided
                )
            }
            Self::VersionConflict(id) => write!(f, "Version conflict updating node: {}", id),
        }
    }
}

impl std::error::Error for NodeRegistryError {}

impl From<sqlx::Error> for NodeRegistryError {
    fn from(e: sqlx::Error) -> Self {
        Self::Database(e)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeRecord {
    pub id: Uuid,
    pub hostname: String,
    pub endpoint: String,
    pub status: NodeStatus,
    pub capacity: NodeCapacity,
    pub cpu_used_millicores: u64,
    pub memory_used_bytes: u64,
    pub running_releases_count: u32,
    pub token_hash: String,
    pub epoch: u64,
    pub version: u64,
    pub is_draining: bool,
    pub is_enabled: bool,
    pub last_heartbeat_at: OffsetDateTime,
    pub registered_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

impl NodeRecord {
    pub fn from_pg_row(row: &sqlx::postgres::PgRow) -> Self {
        let status_str: String = row.get("status");
        let status = match status_str.as_str() {
            "degraded" => NodeStatus::Degraded,
            "draining" => NodeStatus::Draining,
            "offline" => NodeStatus::Offline,
            _ => NodeStatus::Online,
        };
        let cpu_total: i64 = row.get("cpu_total_millicores");
        let memory_total: i64 = row.get("memory_total_bytes");
        let max_releases: i32 = row.get("max_releases");
        let cpu_used: i64 = row.get("cpu_used_millicores");
        let memory_used: i64 = row.get("memory_used_bytes");
        let running_count: i32 = row.get("running_releases_count");
        let epoch: i64 = row.get("epoch");
        let version: i64 = row.get("version");

        Self {
            id: row.get("id"),
            hostname: row.get("hostname"),
            endpoint: row.get("endpoint"),
            status,
            capacity: NodeCapacity {
                cpu_millicores: cpu_total as u64,
                memory_bytes: memory_total as u64,
                max_releases: max_releases as u32,
            },
            cpu_used_millicores: cpu_used as u64,
            memory_used_bytes: memory_used as u64,
            running_releases_count: running_count as u32,
            token_hash: row.get("token_hash"),
            epoch: epoch as u64,
            version: version as u64,
            is_draining: row.get("is_draining"),
            is_enabled: row.get("is_enabled"),
            last_heartbeat_at: row.get("last_heartbeat_at"),
            registered_at: row.get("registered_at"),
            updated_at: row.get("updated_at"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeEventRecord {
    pub id: Uuid,
    pub node_id: Uuid,
    pub from_status: String,
    pub to_status: String,
    pub reason: Option<String>,
    pub created_at: OffsetDateTime,
}

impl NodeEventRecord {
    pub fn from_pg_row(row: &sqlx::postgres::PgRow) -> Self {
        Self {
            id: row.get("id"),
            node_id: row.get("node_id"),
            from_status: row.get("from_status"),
            to_status: row.get("to_status"),
            reason: row.get("reason"),
            created_at: row.get("created_at"),
        }
    }
}

fn status_to_str(s: NodeStatus) -> &'static str {
    match s {
        NodeStatus::Online => "online",
        NodeStatus::Degraded => "degraded",
        NodeStatus::Draining => "draining",
        NodeStatus::Offline => "offline",
    }
}

#[derive(Debug, Default, Clone)]
pub struct NodeRegistry;

impl NodeRegistry {
    pub fn new() -> Self {
        Self
    }

    pub async fn register_node(
        &self,
        pool: &PgPool,
        cmd: &RegisterNodeCommand,
    ) -> Result<RegisterNodeResponse, NodeRegistryError> {
        if cmd.capacity.cpu_millicores == 0
            || cmd.capacity.memory_bytes == 0
            || cmd.capacity.max_releases == 0
        {
            return Err(NodeRegistryError::InvalidCapacity(
                "CPU, memory, and max releases must all be greater than zero".to_string(),
            ));
        }

        let epoch: i64 = sqlx::query_scalar(
            r#"
            INSERT INTO nodes (
                id, hostname, endpoint, status,
                cpu_total_millicores, memory_total_bytes, max_releases,
                token_hash, epoch, version, is_draining, is_enabled,
                last_heartbeat_at, registered_at, updated_at
            )
            VALUES ($1, $2, $3, 'online', $4, $5, $6, $7, 1, 1, FALSE, TRUE, NOW(), NOW(), NOW())
            ON CONFLICT (id) DO UPDATE SET
                hostname = EXCLUDED.hostname,
                endpoint = EXCLUDED.endpoint,
                cpu_total_millicores = EXCLUDED.cpu_total_millicores,
                memory_total_bytes = EXCLUDED.memory_total_bytes,
                max_releases = EXCLUDED.max_releases,
                token_hash = EXCLUDED.token_hash,
                status = 'online',
                is_draining = FALSE,
                is_enabled = TRUE,
                last_heartbeat_at = NOW(),
                updated_at = NOW()
            RETURNING epoch
            "#,
        )
        .bind(cmd.node_id)
        .bind(&cmd.hostname)
        .bind(&cmd.endpoint)
        .bind(cmd.capacity.cpu_millicores as i64)
        .bind(cmd.capacity.memory_bytes as i64)
        .bind(cmd.capacity.max_releases as i32)
        .bind(&cmd.token_hash)
        .fetch_one(pool)
        .await?;

        Ok(RegisterNodeResponse {
            registered: true,
            assigned_epoch: epoch as u64,
        })
    }

    pub async fn record_heartbeat(
        &self,
        pool: &PgPool,
        cmd: &HeartbeatCommand,
    ) -> Result<HeartbeatResponse, NodeRegistryError> {
        let node = self
            .get_node(pool, cmd.node_id)
            .await?
            .ok_or(NodeRegistryError::NodeNotFound(cmd.node_id))?;

        if cmd.epoch != node.epoch {
            return Err(NodeRegistryError::EpochMismatch {
                current: node.epoch,
                provided: cmd.epoch,
            });
        }

        let updated = sqlx::query(
            r#"
            UPDATE nodes
            SET cpu_used_millicores = $1,
                memory_used_bytes = $2,
                running_releases_count = $3,
                last_heartbeat_at = NOW(),
                version = version + 1,
                updated_at = NOW()
            WHERE id = $4 AND version = $5
            "#,
        )
        .bind(cmd.cpu_used_millicores as i64)
        .bind(cmd.memory_used_bytes as i64)
        .bind(cmd.running_releases.len() as i32)
        .bind(cmd.node_id)
        .bind(node.version as i64)
        .execute(pool)
        .await?;

        if updated.rows_affected() == 0 {
            return Err(NodeRegistryError::VersionConflict(cmd.node_id));
        }

        Ok(HeartbeatResponse {
            acknowledged: true,
            drain_requested: node.is_draining,
        })
    }

    pub async fn set_drain(
        &self,
        pool: &PgPool,
        node_id: Uuid,
        draining: bool,
        reason: Option<&str>,
    ) -> Result<(), NodeRegistryError> {
        let node = self
            .get_node(pool, node_id)
            .await?
            .ok_or(NodeRegistryError::NodeNotFound(node_id))?;

        let new_status = if draining {
            NodeStatus::Draining
        } else {
            NodeStatus::Online
        };

        let mut tx = pool.begin().await?;

        sqlx::query(
            r#"
            UPDATE nodes
            SET is_draining = $1,
                status = $2,
                updated_at = NOW()
            WHERE id = $3
            "#,
        )
        .bind(draining)
        .bind(status_to_str(new_status))
        .bind(node_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            INSERT INTO node_events (id, node_id, from_status, to_status, reason, created_at)
            VALUES ($1, $2, $3, $4, $5, NOW())
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(node_id)
        .bind(status_to_str(node.status))
        .bind(status_to_str(new_status))
        .bind(reason)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn set_enabled(
        &self,
        pool: &PgPool,
        node_id: Uuid,
        enabled: bool,
        reason: Option<&str>,
    ) -> Result<(), NodeRegistryError> {
        let node = self
            .get_node(pool, node_id)
            .await?
            .ok_or(NodeRegistryError::NodeNotFound(node_id))?;

        let mut tx = pool.begin().await?;

        sqlx::query(
            r#"
            UPDATE nodes
            SET is_enabled = $1,
                updated_at = NOW()
            WHERE id = $2
            "#,
        )
        .bind(enabled)
        .bind(node_id)
        .execute(&mut *tx)
        .await?;

        let reason_text = format!(
            "Node {} by operator: {}",
            if enabled { "enabled" } else { "disabled" },
            reason.unwrap_or("no reason provided")
        );

        sqlx::query(
            r#"
            INSERT INTO node_events (id, node_id, from_status, to_status, reason, created_at)
            VALUES ($1, $2, $3, $4, $5, NOW())
            "#,
        )
        .bind(Uuid::new_v4())
        .bind(node_id)
        .bind(status_to_str(node.status))
        .bind(status_to_str(node.status))
        .bind(reason_text)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn rotate_node_identity(
        &self,
        pool: &PgPool,
        node_id: Uuid,
        new_token_hash: &str,
    ) -> Result<u64, NodeRegistryError> {
        let new_epoch: i64 = sqlx::query_scalar(
            r#"
            UPDATE nodes
            SET epoch = epoch + 1,
                token_hash = $1,
                updated_at = NOW()
            WHERE id = $2
            RETURNING epoch
            "#,
        )
        .bind(new_token_hash)
        .bind(node_id)
        .fetch_optional(pool)
        .await?
        .ok_or(NodeRegistryError::NodeNotFound(node_id))?;

        Ok(new_epoch as u64)
    }

    pub async fn evaluate_health_and_expiry(
        &self,
        pool: &PgPool,
        heartbeat_timeout: Duration,
    ) -> Result<usize, NodeRegistryError> {
        let timeout_secs = heartbeat_timeout.as_secs() as i64;
        let offline_secs = timeout_secs * 2;

        let rows = sqlx::query(
            r#"
            SELECT * FROM nodes
            WHERE status != 'offline'
              AND last_heartbeat_at < NOW() - ($1 * INTERVAL '1 second')
            "#,
        )
        .bind(timeout_secs)
        .fetch_all(pool)
        .await?;

        let mut transitioned = 0;
        let now = OffsetDateTime::now_utc();

        for raw in rows {
            let node = NodeRecord::from_pg_row(&raw);
            let elapsed_secs = (now - node.last_heartbeat_at).whole_seconds();

            let target_status = if elapsed_secs >= offline_secs {
                NodeStatus::Offline
            } else {
                NodeStatus::Degraded
            };

            if target_status != node.status {
                let mut tx = pool.begin().await?;

                sqlx::query(
                    r#"
                    UPDATE nodes
                    SET status = $1, updated_at = NOW()
                    WHERE id = $2
                    "#,
                )
                .bind(status_to_str(target_status))
                .bind(node.id)
                .execute(&mut *tx)
                .await?;

                let reason = format!(
                    "Heartbeat expired: elapsed {}s exceeds threshold {}s",
                    elapsed_secs, timeout_secs
                );

                sqlx::query(
                    r#"
                    INSERT INTO node_events (id, node_id, from_status, to_status, reason, created_at)
                    VALUES ($1, $2, $3, $4, $5, NOW())
                    "#,
                )
                .bind(Uuid::new_v4())
                .bind(node.id)
                .bind(status_to_str(node.status))
                .bind(status_to_str(target_status))
                .bind(reason)
                .execute(&mut *tx)
                .await?;

                tx.commit().await?;
                transitioned += 1;
            }
        }

        Ok(transitioned)
    }

    pub async fn list_active_nodes(
        &self,
        pool: &PgPool,
        heartbeat_timeout: Duration,
    ) -> Result<Vec<NodeRecord>, NodeRegistryError> {
        let timeout_secs = heartbeat_timeout.as_secs() as i64;
        let rows = sqlx::query(
            r#"
            SELECT * FROM nodes
            WHERE status = 'online'
              AND is_enabled = TRUE
              AND is_draining = FALSE
              AND last_heartbeat_at >= NOW() - ($1 * INTERVAL '1 second')
            ORDER BY cpu_used_millicores ASC
            "#,
        )
        .bind(timeout_secs)
        .fetch_all(pool)
        .await?;

        Ok(rows.iter().map(NodeRecord::from_pg_row).collect())
    }

    pub async fn get_node(
        &self,
        pool: &PgPool,
        node_id: Uuid,
    ) -> Result<Option<NodeRecord>, NodeRegistryError> {
        let row = sqlx::query(
            r#"
            SELECT * FROM nodes WHERE id = $1
            "#,
        )
        .bind(node_id)
        .fetch_optional(pool)
        .await?;

        Ok(row.as_ref().map(NodeRecord::from_pg_row))
    }

    pub async fn list_all_nodes(
        &self,
        pool: &PgPool,
    ) -> Result<Vec<NodeRecord>, NodeRegistryError> {
        let rows = sqlx::query(
            r#"
            SELECT * FROM nodes ORDER BY registered_at DESC
            "#,
        )
        .fetch_all(pool)
        .await?;

        Ok(rows.iter().map(NodeRecord::from_pg_row).collect())
    }

    pub async fn get_node_events(
        &self,
        pool: &PgPool,
        node_id: Uuid,
    ) -> Result<Vec<NodeEventRecord>, NodeRegistryError> {
        let rows = sqlx::query(
            r#"
            SELECT * FROM node_events WHERE node_id = $1 ORDER BY created_at DESC
            "#,
        )
        .bind(node_id)
        .fetch_all(pool)
        .await?;

        Ok(rows.iter().map(NodeEventRecord::from_pg_row).collect())
    }
}
