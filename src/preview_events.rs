use crate::{
    db::Database,
    hostname::preview_hostname,
    previews::{PreviewRepository, PreviewStatus, UpsertPreviewInput},
    providers::PullRequestAction,
    queue::{BuildPriority, BuildQueue},
    source_events::SourceEventRecord,
};
use anyhow::{Context, Result};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewOutcome {
    Created {
        preview_id: Uuid,
        deployment_id: Uuid,
    },
    Updated {
        preview_id: Uuid,
        deployment_id: Uuid,
    },
    Closed {
        preview_id: Uuid,
    },
    Unchanged {
        preview_id: Uuid,
    },
    IgnoredOlder {
        preview_id: Uuid,
    },
}

#[derive(Clone)]
pub struct PreviewService {
    db: Database,
    queue: Option<BuildQueue>,
}

impl PreviewService {
    pub fn new(db: Database, queue: Option<BuildQueue>) -> Self {
        Self { db, queue }
    }

    pub async fn apply_event(&self, event: &SourceEventRecord) -> Result<Option<PreviewOutcome>> {
        let Some(pr) = &event.pull_request else {
            return Ok(None);
        };

        let mut repo = PreviewRepository::new(self.db.pool().into());
        let provider_str = event.provider.to_string();

        let existing = repo
            .find_by_pr(event.project_id, &provider_str, pr.number)
            .await
            .context("failed to check existing preview")?;

        // 1. Handle Closed action
        if matches!(pr.action, PullRequestAction::Closed) {
            if let Some(existing_preview) = existing {
                repo.mark_closed(existing_preview.id, event.created_at)
                    .await
                    .context("failed to mark preview closed")?;

                // If existing preview had an active deployment, cancel any queued/running job
                if let (Some(queue), Some(dep_id)) = (&self.queue, existing_preview.deployment_id) {
                    let _ = queue.cancel(dep_id).await;
                }

                return Ok(Some(PreviewOutcome::Closed {
                    preview_id: existing_preview.id,
                }));
            }
            return Ok(None);
        }

        // 2. Check event ordering and idempotency if existing preview exists
        if let Some(existing_preview) = &existing {
            // If identical commit sha and preview is active, it's a duplicate delivery
            if existing_preview.head_sha == pr.head_sha
                && existing_preview.status != PreviewStatus::Closed
            {
                return Ok(Some(PreviewOutcome::Unchanged {
                    preview_id: existing_preview.id,
                }));
            }

            // Event ordering: if the event timestamp is older than the current preview updated_at, ignore it
            if event.created_at < existing_preview.updated_at {
                return Ok(Some(PreviewOutcome::IgnoredOlder {
                    preview_id: existing_preview.id,
                }));
            }
        }

        // 3. Fetch project name to generate deterministic preview hostname
        let project_name = self.fetch_project_name(event.project_id).await?;
        let hostname = preview_hostname(&project_name, pr.number)
            .context("failed to generate preview hostname")?;

        // 4. Create new deployment record for this preview
        let deployment_id = Uuid::new_v4();
        let query = r#"
            INSERT INTO deployments (id, project_id, framework, status, created_at, commit_sha)
            VALUES ($1, $2, 'static', 'queued', $3, $4)
        "#;
        sqlx::query(query)
            .bind(deployment_id)
            .bind(event.project_id)
            .bind(event.created_at)
            .bind(&pr.head_sha)
            .execute(self.db.pool())
            .await
            .context("failed to create preview deployment record")?;

        // 5. Enqueue build job if queue is configured
        if let Some(queue) = &self.queue {
            queue
                .enqueue_job(deployment_id, event.project_id, BuildPriority::Preview)
                .await
                .context("failed to enqueue preview build job")?;
        }

        // 6. Upsert preview record
        let is_create = existing.is_none();
        let input = UpsertPreviewInput {
            project_id: event.project_id,
            provider: provider_str,
            pr_number: pr.number,
            head_sha: pr.head_sha.clone(),
            base_branch: pr.base_branch.clone(),
            head_branch: event.branch.clone().unwrap_or_else(|| "unknown".into()),
            deployment_id: Some(deployment_id),
            hostname,
            status: PreviewStatus::Building,
        };

        let preview = repo
            .upsert(&input)
            .await
            .context("failed to upsert preview record")?;

        if is_create {
            Ok(Some(PreviewOutcome::Created {
                preview_id: preview.id,
                deployment_id,
            }))
        } else {
            Ok(Some(PreviewOutcome::Updated {
                preview_id: preview.id,
                deployment_id,
            }))
        }
    }

    async fn fetch_project_name(&self, project_id: Uuid) -> Result<String> {
        let query = "SELECT name FROM projects WHERE id = $1";
        let row = sqlx::query(query)
            .bind(project_id)
            .fetch_one(self.db.pool())
            .await
            .context("failed to fetch project name for preview")?;
        let name: String = sqlx::Row::get(&row, "name");
        Ok(name)
    }

    pub async fn record_build_success(
        &self,
        preview_id: Uuid,
        deployment_id: Uuid,
        port: u16,
        url: &str,
        image_path: &str,
    ) -> Result<()> {
        let now = time::OffsetDateTime::now_utc();
        sqlx::query(
            "UPDATE deployments
             SET status = 'running', port = $1, url = $2, image_path = $3, finished_at = $4
             WHERE id = $5",
        )
        .bind(port as i32)
        .bind(url)
        .bind(image_path)
        .bind(now)
        .bind(deployment_id)
        .execute(self.db.pool())
        .await
        .context("failed to update deployment on preview build success")?;

        let mut repo = PreviewRepository::new(self.db.pool().into());
        repo.update_status_and_deployment(preview_id, PreviewStatus::Ready, Some(deployment_id))
            .await
            .context("failed to mark preview ready")?;

        Ok(())
    }

    pub async fn record_build_failure(
        &self,
        preview_id: Uuid,
        failed_deployment_id: Uuid,
        error: &str,
    ) -> Result<()> {
        let now = time::OffsetDateTime::now_utc();
        sqlx::query(
            "UPDATE deployments
             SET status = 'failed', error = $1, finished_at = $2
             WHERE id = $3",
        )
        .bind(error)
        .bind(now)
        .bind(failed_deployment_id)
        .execute(self.db.pool())
        .await
        .context("failed to update deployment on preview build failure")?;

        let mut repo = PreviewRepository::new(self.db.pool().into());
        let Some(preview) = repo.find_by_id(preview_id).await? else {
            return Ok(());
        };

        // If the preview currently points to the failed deployment, mark it failed.
        // If it points to a prior healthy deployment, preserve the prior deployment and Ready status!
        if preview.deployment_id == Some(failed_deployment_id) {
            repo.update_status_and_deployment(preview_id, PreviewStatus::Failed, None)
                .await?;
        }

        Ok(())
    }

    pub async fn record_health_failure(
        &self,
        preview_id: Uuid,
        deployment_id: Uuid,
        error: &str,
    ) -> Result<()> {
        self.record_build_failure(preview_id, deployment_id, error)
            .await
    }

    pub async fn teardown_preview(&self, preview_id: Uuid) -> Result<()> {
        let mut repo = PreviewRepository::new(self.db.pool().into());
        let Some(preview) = repo.find_by_id(preview_id).await? else {
            return Ok(());
        };

        if preview.status == PreviewStatus::Closed {
            return Ok(());
        }

        let now = time::OffsetDateTime::now_utc();
        if let Some(dep_id) = preview.deployment_id {
            if let Some(queue) = &self.queue {
                let _ = queue.cancel(dep_id).await;
            }

            sqlx::query(
                "UPDATE deployments
                 SET status = 'stopped', finished_at = $1
                 WHERE id = $2",
            )
            .bind(now)
            .bind(dep_id)
            .execute(self.db.pool())
            .await
            .context("failed to stop preview deployment")?;
        }

        repo.mark_closed(preview_id, now)
            .await
            .context("failed to mark preview closed during teardown")?;

        Ok(())
    }

    pub async fn promote_to_production(
        &self,
        preview_id: Uuid,
    ) -> Result<crate::model::Deployment> {
        let mut repo = PreviewRepository::new(self.db.pool().into());
        let preview = repo
            .find_by_id(preview_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("preview not found: {preview_id}"))?;

        if preview.status == PreviewStatus::Closed {
            anyhow::bail!("cannot promote a closed preview");
        }

        let new_dep_id = Uuid::new_v4();
        let now = time::OffsetDateTime::now_utc();

        let image_path: Option<String> = if let Some(dep_id) = preview.deployment_id {
            sqlx::query_scalar("SELECT image_path FROM deployments WHERE id = $1")
                .bind(dep_id)
                .fetch_optional(self.db.pool())
                .await?
                .flatten()
        } else {
            None
        };

        sqlx::query(
            "INSERT INTO deployments (id, project_id, framework, status, image_path, created_at, commit_sha, target)
             VALUES ($1, $2, 'static', 'queued', $3, $4, $5, 'production')",
        )
        .bind(new_dep_id)
        .bind(preview.project_id)
        .bind(&image_path)
        .bind(now)
        .bind(&preview.head_sha)
        .execute(self.db.pool())
        .await
        .context("failed to create production deployment during promotion")?;

        if let Some(queue) = &self.queue {
            queue
                .enqueue_job(new_dep_id, preview.project_id, BuildPriority::Production)
                .await
                .context("failed to enqueue production deployment job")?;
        }

        Ok(crate::model::Deployment {
            id: new_dep_id,
            project_id: preview.project_id,
            framework: crate::model::Framework::Static,
            status: crate::model::DeploymentStatus::Queued,
            image_path: image_path.map(std::path::PathBuf::from),
            container_id: None,
            port: None,
            url: None,
            created_at: now,
            finished_at: None,
            error: None,
            commit_sha: Some(preview.head_sha),
            target: Some("production".into()),
        })
    }
}
