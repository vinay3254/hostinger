use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use std::time::Duration;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::auth::{AuthContext, ProjectAccess};
use crate::db::Database;
use crate::health::{HealthPolicy, HealthProbeClient};
use crate::releases::{ContainerStopper, Release, ReleaseOrchestrationPlan};
use crate::repository::CreateAuditEventRecord;
use crate::traffic::{DrainResult, TrafficRouter};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RollbackTarget {
    pub deployment_id: Uuid,
    pub release_id: Uuid,
    pub project_id: Uuid,
    pub environment: String,
    pub framework: String,
    pub commit_sha: Option<String>,
    pub image_path: Option<String>,
    pub container_id: Option<String>,
    pub port: Option<i32>,
    pub url: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

pub fn check_rollback_permission(
    auth: &AuthContext,
    project_user_id: Option<Uuid>,
    environment: &str,
) -> Result<()> {
    let access = ProjectAccess::new(auth, project_user_id);
    if !access.is_owner() {
        return Err(anyhow!(
            "unauthorized: user does not have access to this project"
        ));
    }
    if environment == "production" {
        if !(auth.has_scope("*")
            || auth.has_scope("production:operate")
            || auth.has_scope("deploy:rollback"))
        {
            return Err(anyhow!(
                "unauthorized: only production operators can rollback production traffic"
            ));
        }
    } else if !access.can_operate() {
        return Err(anyhow!(
            "unauthorized: insufficient operator permissions to trigger rollback"
        ));
    }
    Ok(())
}

pub async fn select_rollback_target(
    pool: &sqlx::PgPool,
    project_id: Uuid,
    environment: &str,
    current_deployment_id: Uuid,
) -> Result<RollbackTarget> {
    let query = r#"
        SELECT 
            d.id AS deployment_id,
            r.id AS release_id,
            d.project_id,
            r.environment,
            d.framework,
            d.commit_sha,
            d.image_path,
            r.container_id,
            r.port,
            r.url,
            r.created_at
        FROM releases r
        JOIN deployments d ON d.id = r.deployment_id
        WHERE d.project_id = $1
          AND r.environment = $2
          AND d.id != $3
          AND r.status IN ('active', 'stopped')
          AND d.status IN ('running', 'stopped')
          AND (d.image_path IS NOT NULL OR d.commit_sha IS NOT NULL OR r.container_id IS NOT NULL)
        ORDER BY r.created_at DESC
        LIMIT 1
    "#;

    let row = sqlx::query(query)
        .bind(project_id)
        .bind(environment)
        .bind(current_deployment_id)
        .fetch_optional(pool)
        .await
        .context("failed to query rollback target")?
        .ok_or_else(|| {
            anyhow!(
                "no healthy predecessor release found for rollback in environment '{environment}'"
            )
        })?;

    Ok(RollbackTarget {
        deployment_id: row.get("deployment_id"),
        release_id: row.get("release_id"),
        project_id: row.get("project_id"),
        environment: row.get("environment"),
        framework: row.get("framework"),
        commit_sha: row.get("commit_sha"),
        image_path: row.get("image_path"),
        container_id: row.get("container_id"),
        port: row.get("port"),
        url: row.get("url"),
        created_at: row.get("created_at"),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackCommand {
    pub project_id: Uuid,
    pub environment: String,
    pub current_deployment_id: Uuid,
    pub actor_id: Uuid,
    pub health_policy: Option<HealthPolicy>,
    pub drain_timeout: Option<Duration>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RollbackResult {
    pub new_deployment_id: Uuid,
    pub target: RollbackTarget,
    pub active_release: Release,
    pub previous_release: Option<Release>,
    pub drain_result: Option<DrainResult>,
    pub audit_event_id: Uuid,
}

pub async fn execute_rollback(
    db: &Database,
    auth: &AuthContext,
    cmd: RollbackCommand,
    health_client: &(dyn HealthProbeClient + Send + Sync),
    router: &mut dyn TrafficRouter,
    container_stopper: &mut dyn ContainerStopper,
) -> Result<RollbackResult> {
    // 1. Verify project exists
    let project = db.projects().get_project(cmd.project_id).await?;

    // 2. Permission check
    check_rollback_permission(auth, Some(project.user_id), &cmd.environment)?;

    // 3. Select healthy target with retained artifact
    let target = select_rollback_target(
        db.pool(),
        cmd.project_id,
        &cmd.environment,
        cmd.current_deployment_id,
    )
    .await?;

    // 4. Create new deployment with cause 'rollback'
    let new_deployment_id = Uuid::new_v4();
    let now = OffsetDateTime::now_utc();
    sqlx::query(
        "INSERT INTO deployments (id, project_id, framework, status, image_path, commit_sha, target, cause, rollback_from_deployment_id, created_at)
         VALUES ($1, $2, $3, 'running', $4, $5, $6, 'rollback', $7, $8)",
    )
    .bind(new_deployment_id)
    .bind(cmd.project_id)
    .bind(&target.framework)
    .bind(target.image_path.as_deref())
    .bind(target.commit_sha.as_deref())
    .bind(&cmd.environment)
    .bind(cmd.current_deployment_id)
    .bind(now)
    .execute(db.pool())
    .await
    .context("failed to create rollback deployment")?;

    // 5. Orchestrate health-gated release
    let port = target.port.unwrap_or(8080);
    let target_url = target
        .url
        .clone()
        .unwrap_or_else(|| format!("http://127.0.0.1:{port}"));
    let container_id = format!("rb-{}", Uuid::new_v4().simple());
    let health_policy = cmd.health_policy.unwrap_or_default();
    let drain_timeout = cmd.drain_timeout.unwrap_or(Duration::from_secs(5));

    let plan = ReleaseOrchestrationPlan {
        deployment_id: new_deployment_id,
        project_id: cmd.project_id,
        environment: cmd.environment.clone(),
        container_id,
        port,
        url: target_url,
        health_policy,
        drain_timeout,
    };

    let mut controller = db.releases();
    let orch_res = controller
        .orchestrate_release(plan, health_client, router, container_stopper)
        .await?;

    // 6. Record audit event
    let audit_event_id = Uuid::new_v4();
    let audit_record = CreateAuditEventRecord {
        id: audit_event_id,
        user_id: Some(cmd.actor_id),
        action: "deployment.rollback".to_string(),
        target_type: "deployment".to_string(),
        target_id: new_deployment_id,
        metadata: serde_json::json!({
            "project_id": cmd.project_id,
            "environment": cmd.environment,
            "current_deployment_id": cmd.current_deployment_id,
            "target_deployment_id": target.deployment_id,
            "target_release_id": target.release_id,
            "target_commit_sha": target.commit_sha,
            "previous_active_release_id": orch_res.previous_release.as_ref().map(|r| r.id),
            "new_active_release_id": orch_res.active_release.id,
        }),
        occurred_at: now,
    };
    let mut audit_repo = db.audit();
    audit_repo
        .record(audit_record)
        .await
        .context("failed to record rollback audit event")?;

    Ok(RollbackResult {
        new_deployment_id,
        target,
        active_release: orch_res.active_release,
        previous_release: orch_res.previous_release,
        drain_result: orch_res.drain_result,
        audit_event_id,
    })
}
