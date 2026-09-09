use crate::health::{HealthPolicy, HealthProbeClient};
use crate::repository::DbExecutor;
use crate::traffic::TrafficRouter;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::time::Duration;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReleaseStatus {
    Starting,
    HealthChecking,
    Ready,
    Active,
    Draining,
    Stopped,
    Failed,
}

impl ReleaseStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::HealthChecking => "health_checking",
            Self::Ready => "ready",
            Self::Active => "active",
            Self::Draining => "draining",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
        }
    }

    pub fn parse(s: &str) -> Result<Self> {
        s.parse()
    }

    pub fn can_transition_to(&self, target: ReleaseStatus) -> bool {
        matches!(
            (self, target),
            (Self::Starting, Self::HealthChecking)
                | (Self::Starting, Self::Failed)
                | (Self::HealthChecking, Self::Ready)
                | (Self::HealthChecking, Self::Failed)
                | (Self::Ready, Self::Active)
                | (Self::Ready, Self::Stopped)
                | (Self::Ready, Self::Failed)
                | (Self::Active, Self::Draining)
                | (Self::Active, Self::Stopped)
                | (Self::Active, Self::Failed)
                | (Self::Draining, Self::Stopped)
                | (Self::Draining, Self::Failed)
        )
    }
}

impl std::fmt::Display for ReleaseStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl std::str::FromStr for ReleaseStatus {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "starting" => Ok(Self::Starting),
            "health_checking" => Ok(Self::HealthChecking),
            "ready" => Ok(Self::Ready),
            "active" => Ok(Self::Active),
            "draining" => Ok(Self::Draining),
            "stopped" => Ok(Self::Stopped),
            "failed" => Ok(Self::Failed),
            other => Err(anyhow!("unknown release status: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Release {
    pub id: Uuid,
    pub deployment_id: Uuid,
    pub project_id: Uuid,
    pub environment: String,
    pub status: ReleaseStatus,
    pub version: i32,
    pub container_id: Option<String>,
    pub port: Option<i32>,
    pub url: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReleaseTransitionRecord {
    pub id: Uuid,
    pub release_id: Uuid,
    pub from_status: ReleaseStatus,
    pub to_status: ReleaseStatus,
    pub reason: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

pub trait ContainerStopper: Send + Sync {
    fn stop_container(&mut self, container_id: &str) -> Result<()>;
}

#[derive(Default, Clone)]
pub struct MockContainerStopper {
    pub stopped: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl MockContainerStopper {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ContainerStopper for MockContainerStopper {
    fn stop_container(&mut self, container_id: &str) -> Result<()> {
        self.stopped.lock().unwrap().push(container_id.to_string());
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReleaseOrchestrationPlan {
    pub deployment_id: Uuid,
    pub project_id: Uuid,
    pub environment: String,
    pub container_id: String,
    pub port: i32,
    pub url: String,
    pub health_policy: HealthPolicy,
    pub drain_timeout: Duration,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReleaseOrchestrationResult {
    pub active_release: Release,
    pub previous_release: Option<Release>,
    pub drain_result: Option<crate::traffic::DrainResult>,
}

pub struct ReleaseController<'a> {
    executor: DbExecutor<'a>,
}

impl<'a> ReleaseController<'a> {
    pub fn new(executor: DbExecutor<'a>) -> Self {
        Self { executor }
    }

    pub async fn create_release(
        &mut self,
        deployment_id: Uuid,
        project_id: Uuid,
        environment: &str,
        container_id: Option<String>,
        port: Option<i32>,
        url: Option<String>,
    ) -> Result<Release> {
        let id = Uuid::new_v4();
        let now = OffsetDateTime::now_utc();
        let initial_status = ReleaseStatus::Starting;
        let initial_version = 1;

        self.executor
            .execute(
                sqlx::query(
                    "INSERT INTO releases (id, deployment_id, project_id, environment, status, version, container_id, port, url, created_at, updated_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $10)",
                )
                .bind(id)
                .bind(deployment_id)
                .bind(project_id)
                .bind(environment)
                .bind(initial_status.as_str())
                .bind(initial_version)
                .bind(container_id.as_deref())
                .bind(port)
                .bind(url.as_deref())
                .bind(now),
            )
            .await
            .context("failed to create release")?;

        Ok(Release {
            id,
            deployment_id,
            project_id,
            environment: environment.to_string(),
            status: initial_status,
            version: initial_version,
            container_id,
            port,
            url,
            created_at: now,
            updated_at: now,
        })
    }

    pub async fn get_release(&mut self, release_id: Uuid) -> Result<Release> {
        let row = self
            .executor
            .fetch_optional(
                sqlx::query(
                    "SELECT id, deployment_id, project_id, environment, status, version, container_id, port, url, created_at, updated_at
                     FROM releases WHERE id = $1",
                )
                .bind(release_id),
            )
            .await
            .context("failed to get release")?
            .ok_or_else(|| anyhow!("release not found: {release_id}"))?;

        let status_str: String = row.get("status");
        let status = ReleaseStatus::parse(&status_str)?;

        Ok(Release {
            id: row.get("id"),
            deployment_id: row.get("deployment_id"),
            project_id: row.get("project_id"),
            environment: row.get("environment"),
            status,
            version: row.get("version"),
            container_id: row.get("container_id"),
            port: row.get("port"),
            url: row.get("url"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
    }

    pub async fn get_active_release(
        &mut self,
        project_id: Uuid,
        environment: &str,
    ) -> Result<Option<Release>> {
        let active_row = self
            .executor
            .fetch_optional(
                sqlx::query(
                    "SELECT active_release_id FROM active_routes WHERE project_id = $1 AND environment = $2",
                )
                .bind(project_id)
                .bind(environment),
            )
            .await
            .context("failed to fetch active route")?;

        let active_release_id: Option<Uuid> = active_row.map(|r| r.get("active_release_id"));
        if let Some(rel_id) = active_release_id {
            let rel = self.get_release(rel_id).await?;
            Ok(Some(rel))
        } else {
            Ok(None)
        }
    }

    pub async fn list_releases_for_deployment(
        &mut self,
        deployment_id: Uuid,
    ) -> Result<Vec<Release>> {
        let rows = self
            .executor
            .fetch_all(
                sqlx::query(
                    "SELECT id, deployment_id, project_id, environment, status, version, container_id, port, url, created_at, updated_at
                     FROM releases WHERE deployment_id = $1 ORDER BY created_at ASC",
                )
                .bind(deployment_id),
            )
            .await
            .context("failed to list releases for deployment")?;

        let mut releases = Vec::new();
        for row in rows {
            let status_str: String = row.get("status");
            let status = ReleaseStatus::parse(&status_str)?;
            releases.push(Release {
                id: row.get("id"),
                deployment_id: row.get("deployment_id"),
                project_id: row.get("project_id"),
                environment: row.get("environment"),
                status,
                version: row.get("version"),
                container_id: row.get("container_id"),
                port: row.get("port"),
                url: row.get("url"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            });
        }
        Ok(releases)
    }

    pub async fn list_transitions_for_release(
        &mut self,
        release_id: Uuid,
    ) -> Result<Vec<ReleaseTransitionRecord>> {
        let rows = self
            .executor
            .fetch_all(
                sqlx::query(
                    "SELECT id, release_id, from_status, to_status, reason, created_at
                     FROM release_transitions WHERE release_id = $1 ORDER BY created_at ASC",
                )
                .bind(release_id),
            )
            .await
            .context("failed to list release transitions")?;

        let mut transitions = Vec::new();
        for row in rows {
            let from_str: String = row.get("from_status");
            let to_str: String = row.get("to_status");
            transitions.push(ReleaseTransitionRecord {
                id: row.get("id"),
                release_id: row.get("release_id"),
                from_status: ReleaseStatus::parse(&from_str)?,
                to_status: ReleaseStatus::parse(&to_str)?,
                reason: row.get("reason"),
                created_at: row.get("created_at"),
            });
        }
        Ok(transitions)
    }

    pub async fn transition(
        &mut self,
        release_id: Uuid,
        expected_version: i32,
        target_status: ReleaseStatus,
        reason: Option<&str>,
    ) -> Result<Release> {
        let current = self.get_release(release_id).await?;

        if !current.status.can_transition_to(target_status) {
            return Err(anyhow!(
                "invalid release status transition: cannot transition from {:?} to {:?}",
                current.status,
                target_status
            ));
        }

        let now = OffsetDateTime::now_utc();
        let updated_row = self
            .executor
            .fetch_optional(
                sqlx::query(
                    "UPDATE releases
                     SET status = $1, version = version + 1, updated_at = $2
                     WHERE id = $3 AND version = $4
                     RETURNING id, deployment_id, project_id, environment, status, version, container_id, port, url, created_at, updated_at",
                )
                .bind(target_status.as_str())
                .bind(now)
                .bind(release_id)
                .bind(expected_version),
            )
            .await
            .context("failed to execute compare-and-set release transition")?;

        let row = updated_row.ok_or_else(|| {
            anyhow!(
                "stale release version or concurrent modification: expected version {}",
                expected_version
            )
        })?;

        // Append transition record
        let transition_id = Uuid::new_v4();
        self.executor
            .execute(
                sqlx::query(
                    "INSERT INTO release_transitions (id, release_id, from_status, to_status, reason, created_at)
                     VALUES ($1, $2, $3, $4, $5, $6)",
                )
                .bind(transition_id)
                .bind(release_id)
                .bind(current.status.as_str())
                .bind(target_status.as_str())
                .bind(reason)
                .bind(now),
            )
            .await
            .context("failed to record release transition")?;

        let status_str: String = row.get("status");
        let status = ReleaseStatus::parse(&status_str)?;

        Ok(Release {
            id: row.get("id"),
            deployment_id: row.get("deployment_id"),
            project_id: row.get("project_id"),
            environment: row.get("environment"),
            status,
            version: row.get("version"),
            container_id: row.get("container_id"),
            port: row.get("port"),
            url: row.get("url"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
    }

    pub async fn activate_traffic(
        &mut self,
        release: &Release,
        router: &mut dyn TrafficRouter,
    ) -> Result<Option<Release>> {
        if release.status != ReleaseStatus::Ready && release.status != ReleaseStatus::Active {
            return Err(anyhow!(
                "cannot activate traffic for release in status {:?}; release must be ready or active",
                release.status
            ));
        }

        // Prepare and activate via traffic router
        let route = router.prepare(release)?;
        router.activate(&route)?;

        // Find existing active release
        let prev_active = self
            .get_active_release(release.project_id, &release.environment)
            .await?
            .filter(|p| p.id != release.id);

        let now = OffsetDateTime::now_utc();
        // Upsert into active_routes
        self.executor
            .execute(
                sqlx::query(
                    "INSERT INTO active_routes (project_id, environment, active_release_id, route_target, activated_at, updated_at)
                     VALUES ($1, $2, $3, $4, $5, $5)
                     ON CONFLICT (project_id, environment)
                     DO UPDATE SET
                       active_release_id = EXCLUDED.active_release_id,
                       route_target = EXCLUDED.route_target,
                       updated_at = EXCLUDED.updated_at",
                )
                .bind(release.project_id)
                .bind(&release.environment)
                .bind(release.id)
                .bind(&route.target_url)
                .bind(now),
            )
            .await
            .context("failed to persist active route")?;

        // Transition release to Active if not already
        if release.status == ReleaseStatus::Ready {
            let _ = self
                .transition(
                    release.id,
                    release.version,
                    ReleaseStatus::Active,
                    Some("activated traffic via router"),
                )
                .await?;
        }

        Ok(prev_active)
    }

    pub async fn orchestrate_release(
        &mut self,
        plan: ReleaseOrchestrationPlan,
        health_client: &(dyn HealthProbeClient + Send + Sync),
        router: &mut dyn TrafficRouter,
        container_stopper: &mut dyn ContainerStopper,
    ) -> Result<ReleaseOrchestrationResult> {
        // Step 1: Create release in Starting
        let mut release = self
            .create_release(
                plan.deployment_id,
                plan.project_id,
                &plan.environment,
                Some(plan.container_id.clone()),
                Some(plan.port),
                Some(plan.url.clone()),
            )
            .await?;

        // Step 2: Transition to HealthChecking
        release = self
            .transition(
                release.id,
                release.version,
                ReleaseStatus::HealthChecking,
                Some("probing container health"),
            )
            .await?;

        // Step 3: Probe health
        let health_res =
            crate::health::wait_until_healthy(health_client, &plan.url, &plan.health_policy).await;

        if !health_res.is_healthy {
            let error_msg = health_res
                .final_error
                .unwrap_or_else(|| "health check failed".to_string());
            // Stop failed replacement container
            let _ = container_stopper.stop_container(&plan.container_id);
            // Transition release to Failed (old release remains completely active and untouched)
            let _ = self
                .transition(
                    release.id,
                    release.version,
                    ReleaseStatus::Failed,
                    Some(&format!("health check failed: {error_msg}")),
                )
                .await?;
            return Err(anyhow!("release health check failed: {error_msg}"));
        }

        // Step 4: Health check passed -> Transition to Ready
        release = self
            .transition(
                release.id,
                release.version,
                ReleaseStatus::Ready,
                Some("health check passed"),
            )
            .await?;

        // Step 5: Activate traffic via router and upsert active_routes
        let prev_release = self.activate_traffic(&release, router).await?;
        release = self.get_release(release.id).await?;

        // Step 6: Drain and stop previous active release if present
        let mut drain_result = None;
        let mut drained_prev = None;

        if let Some(prev) = prev_release {
            let prev_draining = self
                .transition(
                    prev.id,
                    prev.version,
                    ReleaseStatus::Draining,
                    Some("traffic switched to new release"),
                )
                .await?;

            let prev_route = router.prepare(&prev_draining)?;
            let drain_res = router.drain(&prev_route, plan.drain_timeout)?;
            drain_result = Some(drain_res);
            let _ = router.remove(&prev_route);

            if let Some(ref cid) = prev_draining.container_id {
                let _ = container_stopper.stop_container(cid);
            }

            let prev_stopped = self
                .transition(
                    prev_draining.id,
                    prev_draining.version,
                    ReleaseStatus::Stopped,
                    Some("drain complete, container stopped"),
                )
                .await?;
            drained_prev = Some(prev_stopped);
        }

        Ok(ReleaseOrchestrationResult {
            active_release: release,
            previous_release: drained_prev,
            drain_result,
        })
    }

    pub async fn reconcile_crashed_releases(&mut self) -> Result<Vec<Release>> {
        // Find releases in intermediate states: starting, health_checking, draining
        let rows = self
            .executor
            .fetch_all(
                sqlx::query(
                    "SELECT id, deployment_id, project_id, environment, status, version, container_id, port, url, created_at, updated_at
                     FROM releases
                     WHERE status IN ('starting', 'health_checking', 'draining')
                     ORDER BY created_at ASC",
                ),
            )
            .await
            .context("failed to query crashed releases")?;

        let mut reconciled = Vec::new();
        for row in rows {
            let rel_id: Uuid = row.get("id");
            let version: i32 = row.get("version");
            let status_str: String = row.get("status");
            let status = ReleaseStatus::parse(&status_str)?;

            let target = match status {
                ReleaseStatus::Starting | ReleaseStatus::HealthChecking => ReleaseStatus::Failed,
                ReleaseStatus::Draining => ReleaseStatus::Stopped,
                _ => continue,
            };

            let reason = match status {
                ReleaseStatus::Starting | ReleaseStatus::HealthChecking => {
                    "reconciled failed: release interrupted in startup/health check during host crash"
                }
                ReleaseStatus::Draining => {
                    "reconciled stopped: drain completed after host crash recovery"
                }
                _ => "",
            };

            if let Ok(updated) = self.transition(rel_id, version, target, Some(reason)).await {
                reconciled.push(updated);
            }
        }

        Ok(reconciled)
    }

    pub async fn reconcile_crashed_releases_full(
        &mut self,
        mut container_stopper: Option<&mut dyn ContainerStopper>,
        mut router: Option<&mut dyn TrafficRouter>,
    ) -> Result<Vec<Release>> {
        let rows = self
            .executor
            .fetch_all(
                sqlx::query(
                    "SELECT id, deployment_id, project_id, environment, status, version, container_id, port, url, created_at, updated_at
                     FROM releases
                     WHERE status IN ('starting', 'health_checking', 'ready', 'draining')
                     ORDER BY created_at ASC",
                ),
            )
            .await
            .context("failed to query crashed releases")?;

        let mut reconciled = Vec::new();
        for row in rows {
            let rel_id: Uuid = row.get("id");
            let project_id: Uuid = row.get("project_id");
            let environment: String = row.get("environment");
            let version: i32 = row.get("version");
            let status_str: String = row.get("status");
            let container_id: Option<String> = row.get("container_id");
            let status = ReleaseStatus::parse(&status_str)?;

            // Check if this release is currently active in active_routes
            let active = self.get_active_release(project_id, &environment).await?;
            let is_active_route = active.as_ref().map(|a| a.id) == Some(rel_id);

            let (target, reason) = match status {
                ReleaseStatus::Starting | ReleaseStatus::HealthChecking => (
                    ReleaseStatus::Failed,
                    "reconciled failed: release interrupted in startup/health check during host crash",
                ),
                ReleaseStatus::Ready => {
                    if is_active_route {
                        (
                            ReleaseStatus::Active,
                            "reconciled active: release was registered in active_routes",
                        )
                    } else {
                        (
                            ReleaseStatus::Failed,
                            "reconciled failed: release was ready but never received traffic before host crash",
                        )
                    }
                }
                ReleaseStatus::Draining => (
                    ReleaseStatus::Stopped,
                    "reconciled stopped: drain completed after host crash recovery",
                ),
                _ => continue,
            };

            // If stopping/failing and container exists, stop container
            if target == ReleaseStatus::Failed || target == ReleaseStatus::Stopped {
                if let (Some(cid), Some(stopper)) =
                    (&container_id, container_stopper.as_deref_mut())
                {
                    let _ = stopper.stop_container(cid);
                }
            }

            if status == ReleaseStatus::Draining {
                if let Some(r) = router.as_deref_mut() {
                    let rel = self.get_release(rel_id).await?;
                    if let Ok(handle) = r.prepare(&rel) {
                        let _ = r.remove(&handle);
                    }
                }
            }

            if let Ok(updated) = self.transition(rel_id, version, target, Some(reason)).await {
                reconciled.push(updated);
            }
        }

        Ok(reconciled)
    }
}

pub struct ReleaseRecoveryWorker<'a> {
    controller: ReleaseController<'a>,
}

impl<'a> ReleaseRecoveryWorker<'a> {
    pub fn new(controller: ReleaseController<'a>) -> Self {
        Self { controller }
    }

    pub async fn run_recovery(
        &mut self,
        container_stopper: Option<&mut dyn ContainerStopper>,
        router: Option<&mut dyn TrafficRouter>,
    ) -> Result<Vec<Release>> {
        self.controller
            .reconcile_crashed_releases_full(container_stopper, router)
            .await
    }
}
