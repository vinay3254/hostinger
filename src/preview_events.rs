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
}
