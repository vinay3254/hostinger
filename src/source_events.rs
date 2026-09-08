use crate::providers::{Provider, PullRequestRef, SourceEvent, SourceEventKind};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SourceEventRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub provider: Provider,
    pub delivery_id: String,
    pub kind: SourceEventKind,
    pub commit_sha: String,
    pub branch: Option<String>,
    pub pull_request: Option<PullRequestRef>,
    pub idempotency_key: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

impl SourceEventRecord {
    pub fn from_event(
        id: Uuid,
        project_id: Uuid,
        event: &SourceEvent,
        created_at: OffsetDateTime,
    ) -> Self {
        let idempotency_key = format!(
            "{}:{}:{}",
            event.repository.provider, event.delivery_id, event.commit_sha
        );
        Self {
            id,
            project_id,
            provider: event.repository.provider,
            delivery_id: event.delivery_id.clone(),
            kind: event.kind,
            commit_sha: event.commit_sha.clone(),
            branch: event.branch.clone(),
            pull_request: event.pull_request.clone(),
            idempotency_key,
            created_at,
        }
    }
}

use crate::repository::DbExecutor;
use anyhow::{Context, Result};
use sqlx::Row;

pub struct SourceEventRepository<'a> {
    executor: DbExecutor<'a>,
}

impl<'a> SourceEventRepository<'a> {
    pub fn new(executor: DbExecutor<'a>) -> Self {
        Self { executor }
    }

    pub async fn record_delivery(
        &mut self,
        provider: &str,
        delivery_id: &str,
        project_id: Option<Uuid>,
        created_at: OffsetDateTime,
    ) -> Result<bool> {
        let query = r#"
            INSERT INTO provider_deliveries (id, provider, delivery_id, project_id, created_at)
            VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (provider, delivery_id) DO NOTHING
            RETURNING id
        "#;
        let q = sqlx::query(query)
            .bind(Uuid::new_v4())
            .bind(provider)
            .bind(delivery_id)
            .bind(project_id)
            .bind(created_at);
        let row_opt = self
            .executor
            .fetch_optional(q)
            .await
            .context("failed to record provider delivery")?;
        Ok(row_opt.is_some())
    }

    pub async fn record_event(&mut self, event: &SourceEventRecord) -> Result<()> {
        let query = r#"
            INSERT INTO source_events (id, project_id, provider, delivery_id, kind, commit_sha, branch, pull_request, idempotency_key, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (idempotency_key) DO NOTHING
        "#;
        let kind_str = match event.kind {
            SourceEventKind::Push => "push",
            SourceEventKind::PullRequestOpened => "pull_request_opened",
            SourceEventKind::PullRequestUpdated => "pull_request_updated",
            SourceEventKind::PullRequestClosed => "pull_request_closed",
        };
        let pr_json = event
            .pull_request
            .as_ref()
            .map(|pr| serde_json::to_value(pr).unwrap_or_default());
        let q = sqlx::query(query)
            .bind(event.id)
            .bind(event.project_id)
            .bind(event.provider.to_string())
            .bind(&event.delivery_id)
            .bind(kind_str)
            .bind(&event.commit_sha)
            .bind(&event.branch)
            .bind(pr_json)
            .bind(&event.idempotency_key)
            .bind(event.created_at);
        self.executor
            .execute(q)
            .await
            .context("failed to record source event")?;
        Ok(())
    }

    pub async fn list_by_project(&mut self, project_id: Uuid) -> Result<Vec<SourceEventRecord>> {
        let query = r#"
            SELECT id, project_id, provider, delivery_id, kind, commit_sha, branch, pull_request, idempotency_key, created_at
            FROM source_events
            WHERE project_id = $1
            ORDER BY created_at ASC
        "#;
        let q = sqlx::query(query).bind(project_id);
        let rows = self
            .executor
            .fetch_all(q)
            .await
            .context("failed to list source events")?;
        let mut list = Vec::new();
        for row in rows {
            let provider_str: String = row.get("provider");
            let kind_str: String = row.get("kind");
            let kind = match kind_str.as_str() {
                "push" => SourceEventKind::Push,
                "pull_request_opened" => SourceEventKind::PullRequestOpened,
                "pull_request_updated" => SourceEventKind::PullRequestUpdated,
                "pull_request_closed" => SourceEventKind::PullRequestClosed,
                _ => SourceEventKind::Push,
            };
            let pr_val: Option<serde_json::Value> = row.get("pull_request");
            let pull_request = pr_val.and_then(|v| serde_json::from_value(v).ok());

            list.push(SourceEventRecord {
                id: row.get("id"),
                project_id: row.get("project_id"),
                provider: provider_str.parse()?,
                delivery_id: row.get("delivery_id"),
                kind,
                commit_sha: row.get("commit_sha"),
                branch: row.get("branch"),
                pull_request,
                idempotency_key: row.get("idempotency_key"),
                created_at: row.get("created_at"),
            });
        }
        Ok(list)
    }
}
