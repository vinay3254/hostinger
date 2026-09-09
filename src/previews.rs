use crate::repository::DbExecutor;
use anyhow::{Context, Result};
use sqlx::Row;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PreviewStatus {
    Building,
    Ready,
    Failed,
    Closed,
}

impl std::fmt::Display for PreviewStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Building => write!(f, "building"),
            Self::Ready => write!(f, "ready"),
            Self::Failed => write!(f, "failed"),
            Self::Closed => write!(f, "closed"),
        }
    }
}

impl std::str::FromStr for PreviewStatus {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s {
            "building" => Ok(Self::Building),
            "ready" => Ok(Self::Ready),
            "failed" => Ok(Self::Failed),
            "closed" => Ok(Self::Closed),
            other => Err(anyhow::anyhow!("unknown preview status: {other}")),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Preview {
    pub id: Uuid,
    pub project_id: Uuid,
    pub provider: String,
    pub pr_number: u64,
    pub head_sha: String,
    pub base_branch: String,
    pub head_branch: String,
    pub deployment_id: Option<Uuid>,
    pub hostname: String,
    pub status: PreviewStatus,
    #[serde(with = "time::serde::rfc3339::option")]
    pub closed_at: Option<OffsetDateTime>,
    pub cleanup_attempt: i32,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct UpsertPreviewInput {
    pub project_id: Uuid,
    pub provider: String,
    pub pr_number: u64,
    pub head_sha: String,
    pub base_branch: String,
    pub head_branch: String,
    pub deployment_id: Option<Uuid>,
    pub hostname: String,
    pub status: PreviewStatus,
}

pub struct PreviewRepository<'a> {
    executor: DbExecutor<'a>,
}

impl<'a> PreviewRepository<'a> {
    pub fn new(executor: DbExecutor<'a>) -> Self {
        Self { executor }
    }

    pub async fn upsert(&mut self, input: &UpsertPreviewInput) -> Result<Preview> {
        let id = Uuid::new_v4();
        let now = OffsetDateTime::now_utc();
        let query = r#"
            INSERT INTO previews (
                id, project_id, provider, pr_number, head_sha, base_branch, head_branch,
                deployment_id, hostname, status, created_at, updated_at
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $11)
            ON CONFLICT (project_id, provider, pr_number) DO UPDATE SET
                head_sha = EXCLUDED.head_sha,
                base_branch = EXCLUDED.base_branch,
                head_branch = EXCLUDED.head_branch,
                deployment_id = COALESCE(EXCLUDED.deployment_id, previews.deployment_id),
                hostname = EXCLUDED.hostname,
                status = EXCLUDED.status,
                updated_at = EXCLUDED.updated_at
            RETURNING
                id, project_id, provider, pr_number, head_sha, base_branch, head_branch,
                deployment_id, hostname, status, closed_at, cleanup_attempt, created_at, updated_at
        "#;

        let row = self
            .executor
            .fetch_one(
                sqlx::query(query)
                    .bind(id)
                    .bind(input.project_id)
                    .bind(&input.provider)
                    .bind(input.pr_number as i64)
                    .bind(&input.head_sha)
                    .bind(&input.base_branch)
                    .bind(&input.head_branch)
                    .bind(input.deployment_id)
                    .bind(&input.hostname)
                    .bind(input.status.to_string())
                    .bind(now),
            )
            .await
            .context("failed to upsert preview")?;

        map_row_to_preview(&row)
    }

    pub async fn find_by_id(&mut self, id: Uuid) -> Result<Option<Preview>> {
        let query = r#"
            SELECT
                id, project_id, provider, pr_number, head_sha, base_branch, head_branch,
                deployment_id, hostname, status, closed_at, cleanup_attempt, created_at, updated_at
            FROM previews
            WHERE id = $1
        "#;
        let row_opt = self
            .executor
            .fetch_optional(sqlx::query(query).bind(id))
            .await
            .context("failed to find preview by id")?;

        row_opt.map(|r| map_row_to_preview(&r)).transpose()
    }

    pub async fn find_by_pr(
        &mut self,
        project_id: Uuid,
        provider: &str,
        pr_number: u64,
    ) -> Result<Option<Preview>> {
        let query = r#"
            SELECT
                id, project_id, provider, pr_number, head_sha, base_branch, head_branch,
                deployment_id, hostname, status, closed_at, cleanup_attempt, created_at, updated_at
            FROM previews
            WHERE project_id = $1 AND provider = $2 AND pr_number = $3
        "#;
        let row_opt = self
            .executor
            .fetch_optional(
                sqlx::query(query)
                    .bind(project_id)
                    .bind(provider)
                    .bind(pr_number as i64),
            )
            .await
            .context("failed to find preview by pr")?;

        row_opt.map(|r| map_row_to_preview(&r)).transpose()
    }

    pub async fn list_by_project(&mut self, project_id: Uuid) -> Result<Vec<Preview>> {
        let query = r#"
            SELECT
                id, project_id, provider, pr_number, head_sha, base_branch, head_branch,
                deployment_id, hostname, status, closed_at, cleanup_attempt, created_at, updated_at
            FROM previews
            WHERE project_id = $1
            ORDER BY created_at DESC
        "#;
        let rows = self
            .executor
            .fetch_all(sqlx::query(query).bind(project_id))
            .await
            .context("failed to list previews by project")?;

        rows.into_iter().map(|r| map_row_to_preview(&r)).collect()
    }

    pub async fn mark_closed(&mut self, id: Uuid, closed_at: OffsetDateTime) -> Result<()> {
        let query = r#"
            UPDATE previews
            SET status = 'closed', closed_at = $2, updated_at = $2
            WHERE id = $1
        "#;
        self.executor
            .execute(sqlx::query(query).bind(id).bind(closed_at))
            .await
            .context("failed to mark preview closed")?;
        Ok(())
    }

    pub async fn update_status_and_deployment(
        &mut self,
        id: Uuid,
        status: PreviewStatus,
        deployment_id: Option<Uuid>,
    ) -> Result<()> {
        let now = OffsetDateTime::now_utc();
        let query = r#"
            UPDATE previews
            SET status = $2,
                deployment_id = COALESCE($3, deployment_id),
                updated_at = $4
            WHERE id = $1
        "#;
        self.executor
            .execute(
                sqlx::query(query)
                    .bind(id)
                    .bind(status.to_string())
                    .bind(deployment_id)
                    .bind(now),
            )
            .await
            .context("failed to update preview status and deployment")?;
        Ok(())
    }

    pub async fn increment_cleanup_attempt(&mut self, id: Uuid) -> Result<()> {
        let now = OffsetDateTime::now_utc();
        let query = r#"
            UPDATE previews
            SET cleanup_attempt = cleanup_attempt + 1, updated_at = $2
            WHERE id = $1
        "#;
        self.executor
            .execute(sqlx::query(query).bind(id).bind(now))
            .await
            .context("failed to increment cleanup attempt")?;
        Ok(())
    }
}

fn map_row_to_preview(row: &sqlx::postgres::PgRow) -> Result<Preview> {
    let pr_number_i64: i64 = row.try_get("pr_number")?;
    let status_str: String = row.try_get("status")?;
    let status = status_str.parse::<PreviewStatus>()?;

    Ok(Preview {
        id: row.try_get("id")?,
        project_id: row.try_get("project_id")?,
        provider: row.try_get("provider")?,
        pr_number: pr_number_i64 as u64,
        head_sha: row.try_get("head_sha")?,
        base_branch: row.try_get("base_branch")?,
        head_branch: row.try_get("head_branch")?,
        deployment_id: row.try_get("deployment_id")?,
        hostname: row.try_get("hostname")?,
        status,
        closed_at: row.try_get("closed_at")?,
        cleanup_attempt: row.try_get("cleanup_attempt")?,
        created_at: row.try_get("created_at")?,
        updated_at: row.try_get("updated_at")?,
    })
}
