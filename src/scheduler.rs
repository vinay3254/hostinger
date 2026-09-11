use crate::agent_protocol::NodeStatus;
use crate::node_registry::{NodeRecord, NodeRegistry};
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, Row};
use std::collections::HashSet;
use std::time::Duration;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug)]
pub enum SchedulerError {
    Database(sqlx::Error),
    InsufficientCapacity,
    PlacementNotFound(Uuid),
}

impl std::fmt::Display for SchedulerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Database(e) => write!(f, "Database error: {}", e),
            Self::InsufficientCapacity => {
                write!(f, "Insufficient node capacity available for placement")
            }
            Self::PlacementNotFound(id) => {
                write!(f, "Placement record not found for release: {}", id)
            }
        }
    }
}

impl std::error::Error for SchedulerError {}

impl From<sqlx::Error> for SchedulerError {
    fn from(e: sqlx::Error) -> Self {
        Self::Database(e)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlacementStatus {
    Placed,
    Active,
    Stopped,
    Failed,
}

impl PlacementStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Placed => "placed",
            Self::Active => "active",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "active" => Self::Active,
            "stopped" => Self::Stopped,
            "failed" => Self::Failed,
            _ => Self::Placed,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementRequest {
    pub release_id: Uuid,
    pub deployment_id: Uuid,
    pub project_id: Uuid,
    pub required_cpu_millicores: u64,
    pub required_memory_bytes: u64,
    pub candidate_nodes: Option<HashSet<Uuid>>,
    pub anti_affinity_nodes: HashSet<Uuid>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RescheduleRequest {
    pub release_id: Uuid,
    pub failed_node_id: Uuid,
    pub required_cpu_millicores: u64,
    pub required_memory_bytes: u64,
    pub candidate_nodes: Option<HashSet<Uuid>>,
    pub lease_duration: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementDecision {
    pub release_id: Uuid,
    pub selected_node_id: Uuid,
    pub lease_id: Uuid,
    pub allocated_cpu_millicores: u64,
    pub allocated_memory_bytes: u64,
    pub lease_expires_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlacementRecord {
    pub id: Uuid,
    pub release_id: Uuid,
    pub node_id: Uuid,
    pub operation_lease_id: Uuid,
    pub status: PlacementStatus,
    pub cpu_allocated_millicores: u64,
    pub memory_allocated_bytes: u64,
    pub lease_expires_at: OffsetDateTime,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Default, Clone)]
pub struct Scheduler {
    registry: NodeRegistry,
}

impl Scheduler {
    pub fn new() -> Self {
        Self {
            registry: NodeRegistry::new(),
        }
    }

    pub fn score_node(node: &NodeRecord, req_cpu: u64, req_mem: u64) -> Option<f64> {
        if node.status != NodeStatus::Online || !node.is_enabled || node.is_draining {
            return None;
        }

        let future_cpu = node.cpu_used_millicores + req_cpu;
        let future_mem = node.memory_used_bytes + req_mem;

        if future_cpu > node.capacity.cpu_millicores
            || future_mem > node.capacity.memory_bytes
            || node.running_releases_count >= node.capacity.max_releases
        {
            return None;
        }

        let remaining_cpu_pct = 1.0 - (future_cpu as f64 / node.capacity.cpu_millicores as f64);
        let remaining_mem_pct = 1.0 - (future_mem as f64 / node.capacity.memory_bytes as f64);

        // Balanced resource score (weighted 50/50 CPU and memory)
        Some((remaining_cpu_pct + remaining_mem_pct) / 2.0)
    }

    pub async fn schedule_release(
        &self,
        pool: &PgPool,
        req: &PlacementRequest,
        lease_duration: Duration,
    ) -> Result<PlacementDecision, SchedulerError> {
        let active_nodes = self
            .registry
            .list_active_nodes(pool, Duration::from_secs(60))
            .await
            .map_err(|e| match e {
                crate::node_registry::NodeRegistryError::Database(err) => {
                    SchedulerError::Database(err)
                }
                _ => SchedulerError::InsufficientCapacity,
            })?;

        let mut candidate_scores: Vec<(&NodeRecord, f64)> = Vec::new();

        for node in &active_nodes {
            if let Some(ref candidates) = req.candidate_nodes {
                if !candidates.contains(&node.id) {
                    continue;
                }
            }

            if req.anti_affinity_nodes.contains(&node.id) {
                continue;
            }

            if let Some(score) =
                Self::score_node(node, req.required_cpu_millicores, req.required_memory_bytes)
            {
                candidate_scores.push((node, score));
            }
        }

        if candidate_scores.is_empty() {
            return Err(SchedulerError::InsufficientCapacity);
        }

        // Sort by score descending; break ties deterministically by node UUID string
        candidate_scores.sort_by(|(a_node, a_score), (b_node, b_score)| {
            b_score
                .partial_cmp(a_score)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a_node.id.cmp(&b_node.id))
        });

        let selected_node = candidate_scores[0].0;
        let lease_id = Uuid::new_v4();
        let placement_id = Uuid::new_v4();
        let now = OffsetDateTime::now_utc();
        let lease_expires_at = now + time::Duration::seconds(lease_duration.as_secs() as i64);

        let mut tx = pool.begin().await?;

        sqlx::query(
            r#"
            INSERT INTO placements (
                id, release_id, node_id, operation_lease_id,
                status, cpu_allocated_millicores, memory_allocated_bytes,
                lease_expires_at, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, 'placed', $5, $6, $7, NOW(), NOW())
            "#,
        )
        .bind(placement_id)
        .bind(req.release_id)
        .bind(selected_node.id)
        .bind(lease_id)
        .bind(req.required_cpu_millicores as i64)
        .bind(req.required_memory_bytes as i64)
        .bind(lease_expires_at)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            UPDATE nodes
            SET cpu_used_millicores = cpu_used_millicores + $1,
                memory_used_bytes = memory_used_bytes + $2,
                running_releases_count = running_releases_count + 1,
                updated_at = NOW()
            WHERE id = $3
            "#,
        )
        .bind(req.required_cpu_millicores as i64)
        .bind(req.required_memory_bytes as i64)
        .bind(selected_node.id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        Ok(PlacementDecision {
            release_id: req.release_id,
            selected_node_id: selected_node.id,
            lease_id,
            allocated_cpu_millicores: req.required_cpu_millicores,
            allocated_memory_bytes: req.required_memory_bytes,
            lease_expires_at,
        })
    }

    pub async fn release_placement(
        &self,
        pool: &PgPool,
        release_id: Uuid,
    ) -> Result<(), SchedulerError> {
        let row = sqlx::query(
            "SELECT node_id, cpu_allocated_millicores, memory_allocated_bytes, status FROM placements WHERE release_id = $1 ORDER BY created_at DESC LIMIT 1"
        )
        .bind(release_id)
        .fetch_optional(pool)
        .await?
        .ok_or(SchedulerError::PlacementNotFound(release_id))?;

        let status_str: String = row.get("status");
        if status_str == "stopped" || status_str == "failed" {
            return Ok(());
        }

        let node_id: Uuid = row.get("node_id");
        let cpu: i64 = row.get("cpu_allocated_millicores");
        let mem: i64 = row.get("memory_allocated_bytes");

        let mut tx = pool.begin().await?;

        sqlx::query(
            "UPDATE placements SET status = 'stopped', updated_at = NOW() WHERE release_id = $1",
        )
        .bind(release_id)
        .execute(&mut *tx)
        .await?;

        sqlx::query(
            r#"
            UPDATE nodes
            SET cpu_used_millicores = GREATEST(0, cpu_used_millicores - $1),
                memory_used_bytes = GREATEST(0, memory_used_bytes - $2),
                running_releases_count = GREATEST(0, running_releases_count - 1),
                updated_at = NOW()
            WHERE id = $3
            "#,
        )
        .bind(cpu)
        .bind(mem)
        .bind(node_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;
        Ok(())
    }

    pub async fn reconcile_expired_leases(
        &self,
        pool: &PgPool,
    ) -> Result<Vec<Uuid>, SchedulerError> {
        let rows = sqlx::query(
            r#"
            SELECT release_id, node_id, cpu_allocated_millicores, memory_allocated_bytes
            FROM placements
            WHERE status = 'placed' AND lease_expires_at < NOW()
            "#,
        )
        .fetch_all(pool)
        .await?;

        let mut expired_releases = Vec::new();

        for row in rows {
            let release_id: Uuid = row.get("release_id");
            let node_id: Uuid = row.get("node_id");
            let cpu: i64 = row.get("cpu_allocated_millicores");
            let mem: i64 = row.get("memory_allocated_bytes");

            let mut tx = pool.begin().await?;

            sqlx::query(
                "UPDATE placements SET status = 'failed', updated_at = NOW() WHERE release_id = $1 AND status = 'placed'",
            )
            .bind(release_id)
            .execute(&mut *tx)
            .await?;

            sqlx::query(
                r#"
                UPDATE nodes
                SET cpu_used_millicores = GREATEST(0, cpu_used_millicores - $1),
                    memory_used_bytes = GREATEST(0, memory_used_bytes - $2),
                    running_releases_count = GREATEST(0, running_releases_count - 1),
                    updated_at = NOW()
                WHERE id = $3
                "#,
            )
            .bind(cpu)
            .bind(mem)
            .bind(node_id)
            .execute(&mut *tx)
            .await?;

            tx.commit().await?;
            expired_releases.push(release_id);
        }

        Ok(expired_releases)
    }

    pub async fn reschedule_failed_release(
        &self,
        pool: &PgPool,
        req: RescheduleRequest,
    ) -> Result<PlacementDecision, SchedulerError> {
        // Mark old placement failed
        let _ = self.release_placement(pool, req.release_id).await;

        let mut anti_affinity = HashSet::new();
        anti_affinity.insert(req.failed_node_id);

        let placement_req = PlacementRequest {
            release_id: req.release_id,
            deployment_id: Uuid::new_v4(),
            project_id: Uuid::new_v4(),
            required_cpu_millicores: req.required_cpu_millicores,
            required_memory_bytes: req.required_memory_bytes,
            candidate_nodes: req.candidate_nodes,
            anti_affinity_nodes: anti_affinity,
        };

        self.schedule_release(pool, &placement_req, req.lease_duration)
            .await
    }

    pub async fn list_placements_for_node(
        &self,
        pool: &PgPool,
        node_id: Uuid,
    ) -> Result<Vec<PlacementRecord>, SchedulerError> {
        let rows = sqlx::query(
            r#"
            SELECT id, release_id, node_id, operation_lease_id, status,
                   cpu_allocated_millicores, memory_allocated_bytes,
                   lease_expires_at, created_at, updated_at
            FROM placements
            WHERE node_id = $1
            ORDER BY created_at DESC
            "#,
        )
        .bind(node_id)
        .fetch_all(pool)
        .await?;

        let records = rows
            .iter()
            .map(|r| {
                let status_str: String = r.get("status");
                let cpu: i64 = r.get("cpu_allocated_millicores");
                let mem: i64 = r.get("memory_allocated_bytes");
                PlacementRecord {
                    id: r.get("id"),
                    release_id: r.get("release_id"),
                    node_id: r.get("node_id"),
                    operation_lease_id: r.get("operation_lease_id"),
                    status: PlacementStatus::parse(&status_str),
                    cpu_allocated_millicores: cpu as u64,
                    memory_allocated_bytes: mem as u64,
                    lease_expires_at: r.get("lease_expires_at"),
                    created_at: r.get("created_at"),
                    updated_at: r.get("updated_at"),
                }
            })
            .collect();

        Ok(records)
    }
}
