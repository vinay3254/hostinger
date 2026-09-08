use crate::model::{DeploymentStatus, Framework};
use anyhow::{Context, Result};
use sqlx::{
    postgres::PgArguments, postgres::PgQueryResult, postgres::PgRow, query::Query, Postgres, Row,
};
use std::path::PathBuf;
use time::OffsetDateTime;
use uuid::Uuid;

pub enum DbExecutor<'a> {
    Pool(&'a sqlx::PgPool),
    Tx(&'a mut sqlx::Transaction<'static, sqlx::Postgres>),
}

impl<'a> DbExecutor<'a> {
    pub async fn execute<'q>(
        &mut self,
        query: Query<'q, Postgres, PgArguments>,
    ) -> Result<PgQueryResult, sqlx::Error> {
        match self {
            DbExecutor::Pool(pool) => query.execute(*pool).await,
            DbExecutor::Tx(tx) => query.execute(&mut ***tx).await,
        }
    }

    pub async fn fetch_optional<'q>(
        &mut self,
        query: Query<'q, Postgres, PgArguments>,
    ) -> Result<Option<PgRow>, sqlx::Error> {
        match self {
            DbExecutor::Pool(pool) => query.fetch_optional(*pool).await,
            DbExecutor::Tx(tx) => query.fetch_optional(&mut ***tx).await,
        }
    }

    pub async fn fetch_all<'q>(
        &mut self,
        query: Query<'q, Postgres, PgArguments>,
    ) -> Result<Vec<PgRow>, sqlx::Error> {
        match self {
            DbExecutor::Pool(pool) => query.fetch_all(*pool).await,
            DbExecutor::Tx(tx) => query.fetch_all(&mut ***tx).await,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserRecord {
    pub id: Uuid,
    pub email: String,
    pub password_hash: String,
    pub name: String,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectRecord {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub source_dir: PathBuf,
    pub base_image: PathBuf,
    pub server_command: Vec<String>,
    pub active_deployment: Option<Uuid>,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateProjectRecord {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub source_dir: PathBuf,
    pub base_image: PathBuf,
    pub server_command: Vec<String>,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeploymentRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub framework: Framework,
    pub status: DeploymentStatus,
    pub image_path: Option<PathBuf>,
    pub container_id: Option<Uuid>,
    pub port: Option<u16>,
    pub url: Option<String>,
    pub error: Option<String>,
    pub created_at: OffsetDateTime,
    pub finished_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateDeploymentRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub framework: Framework,
    pub status: DeploymentStatus,
    pub image_path: Option<PathBuf>,
    pub container_id: Option<Uuid>,
    pub port: Option<u16>,
    pub url: Option<String>,
    pub created_at: OffsetDateTime,
}

pub struct UserRepository<'a> {
    executor: DbExecutor<'a>,
}

impl<'a> UserRepository<'a> {
    pub fn new(executor: DbExecutor<'a>) -> Self {
        Self { executor }
    }

    pub async fn create_user(
        &mut self,
        id: Uuid,
        email: &str,
        password_hash: &str,
        name: &str,
        created_at: OffsetDateTime,
    ) -> Result<UserRecord> {
        let query = "INSERT INTO users (id, email, password_hash, name, created_at) VALUES ($1, $2, $3, $4, $5)";
        let q = sqlx::query(query)
            .bind(id)
            .bind(email)
            .bind(password_hash)
            .bind(name)
            .bind(created_at);
        self.executor
            .execute(q)
            .await
            .with_context(|| format!("failed to insert user {email}"))?;

        Ok(UserRecord {
            id,
            email: email.to_string(),
            password_hash: password_hash.to_string(),
            name: name.to_string(),
            created_at,
        })
    }

    pub async fn get_by_id(&mut self, id: Uuid) -> Result<UserRecord> {
        let query = "SELECT id, email, password_hash, name, created_at FROM users WHERE id = $1";
        let q = sqlx::query(query).bind(id);
        let row = self
            .executor
            .fetch_optional(q)
            .await
            .context("failed to query user by id")?
            .ok_or_else(|| anyhow::anyhow!("user not found: {id}"))?;

        Ok(UserRecord {
            id: row.get("id"),
            email: row.get("email"),
            password_hash: row.get("password_hash"),
            name: row.get("name"),
            created_at: row.get("created_at"),
        })
    }

    pub async fn get_by_email(&mut self, email: &str) -> Result<UserRecord> {
        let query = "SELECT id, email, password_hash, name, created_at FROM users WHERE email = $1";
        let q = sqlx::query(query).bind(email);
        let row = self
            .executor
            .fetch_optional(q)
            .await
            .context("failed to query user by email")?
            .ok_or_else(|| anyhow::anyhow!("user not found with email: {email}"))?;

        Ok(UserRecord {
            id: row.get("id"),
            email: row.get("email"),
            password_hash: row.get("password_hash"),
            name: row.get("name"),
            created_at: row.get("created_at"),
        })
    }
}

pub struct ProjectRepository<'a> {
    executor: DbExecutor<'a>,
}

impl<'a> ProjectRepository<'a> {
    pub fn new(executor: DbExecutor<'a>) -> Self {
        Self { executor }
    }

    pub async fn create_project(&mut self, record: CreateProjectRecord) -> Result<ProjectRecord> {
        let query = r#"
            INSERT INTO projects (id, user_id, name, source_dir, base_image, server_command, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#;
        let source_str = record.source_dir.to_str().unwrap_or("");
        let base_str = record.base_image.to_str().unwrap_or("");

        let q = sqlx::query(query)
            .bind(record.id)
            .bind(record.user_id)
            .bind(&record.name)
            .bind(source_str)
            .bind(base_str)
            .bind(&record.server_command)
            .bind(record.created_at);

        self.executor
            .execute(q)
            .await
            .with_context(|| format!("failed to create project {}", record.name))?;

        Ok(ProjectRecord {
            id: record.id,
            user_id: record.user_id,
            name: record.name,
            source_dir: record.source_dir,
            base_image: record.base_image,
            server_command: record.server_command,
            active_deployment: None,
            created_at: record.created_at,
        })
    }

    pub async fn get_project(&mut self, id: Uuid) -> Result<ProjectRecord> {
        let query = "SELECT id, user_id, name, source_dir, base_image, server_command, active_deployment, created_at FROM projects WHERE id = $1";
        let q = sqlx::query(query).bind(id);
        let row = self
            .executor
            .fetch_optional(q)
            .await
            .context("failed to query project")?
            .ok_or_else(|| anyhow::anyhow!("project not found: {id}"))?;

        let source_str: String = row.get("source_dir");
        let base_str: String = row.get("base_image");

        Ok(ProjectRecord {
            id: row.get("id"),
            user_id: row.get("user_id"),
            name: row.get("name"),
            source_dir: PathBuf::from(source_str),
            base_image: PathBuf::from(base_str),
            server_command: row.get("server_command"),
            active_deployment: row.get("active_deployment"),
            created_at: row.get("created_at"),
        })
    }

    pub async fn list_projects(&mut self, user_id: Uuid) -> Result<Vec<ProjectRecord>> {
        let query = "SELECT id, user_id, name, source_dir, base_image, server_command, active_deployment, created_at FROM projects WHERE user_id = $1 ORDER BY created_at ASC";
        let q = sqlx::query(query).bind(user_id);
        let rows = self
            .executor
            .fetch_all(q)
            .await
            .context("failed to list projects")?;

        let mut list = Vec::new();
        for row in rows {
            let source_str: String = row.get("source_dir");
            let base_str: String = row.get("base_image");
            list.push(ProjectRecord {
                id: row.get("id"),
                user_id: row.get("user_id"),
                name: row.get("name"),
                source_dir: PathBuf::from(source_str),
                base_image: PathBuf::from(base_str),
                server_command: row.get("server_command"),
                active_deployment: row.get("active_deployment"),
                created_at: row.get("created_at"),
            });
        }
        Ok(list)
    }

    pub async fn update_active_deployment(
        &mut self,
        id: Uuid,
        deployment_id: Option<Uuid>,
    ) -> Result<()> {
        let query = "UPDATE projects SET active_deployment = $2 WHERE id = $1";
        let q = sqlx::query(query).bind(id).bind(deployment_id);
        self.executor
            .execute(q)
            .await
            .with_context(|| format!("failed to update active deployment for project {id}"))?;
        Ok(())
    }

    pub async fn get_project_source(&mut self, project_id: Uuid) -> Result<ProjectSourceDetails> {
        let query = r#"
            SELECT p.repository_id, p.target_branch, p.webhook_secret,
                   r.id as repo_id, r.connection_id, r.external_id, r.full_name, r.clone_url, r.default_branch, r.synced_at,
                   c.provider
            FROM projects p
            LEFT JOIN provider_repositories r ON p.repository_id = r.id
            LEFT JOIN provider_connections c ON r.connection_id = c.id
            WHERE p.id = $1
        "#;
        let q = sqlx::query(query).bind(project_id);
        let row = self
            .executor
            .fetch_optional(q)
            .await
            .context("failed to query project source")?
            .ok_or_else(|| anyhow::anyhow!("project not found: {project_id}"))?;

        let repo_id: Option<Uuid> = row.get("repo_id");
        let repository = repo_id.map(|id| ProviderRepositoryRecord {
            id,
            connection_id: row.get("connection_id"),
            external_id: row.get("external_id"),
            full_name: row.get("full_name"),
            clone_url: row.get("clone_url"),
            default_branch: row.get("default_branch"),
            synced_at: row.get("synced_at"),
        });

        let provider: Option<String> = row.get("provider");
        let target_branch: Option<String> = row.get("target_branch");
        let webhook_secret: Option<String> = row.get("webhook_secret");

        let delivery_query = "SELECT created_at FROM provider_deliveries WHERE project_id = $1 ORDER BY created_at DESC LIMIT 1";
        let last_del_row = self
            .executor
            .fetch_optional(sqlx::query(delivery_query).bind(project_id))
            .await
            .context("failed to query last delivery")?;
        let last_delivery_at: Option<OffsetDateTime> = last_del_row.map(|r| r.get("created_at"));

        let webhook_url = provider
            .as_ref()
            .map(|p| format!("/v1/webhooks/{p}/{project_id}"));

        Ok(ProjectSourceDetails {
            repository,
            provider,
            target_branch,
            webhook_url,
            has_webhook_secret: webhook_secret.is_some(),
            last_delivery_at: last_delivery_at.map(|t| {
                t.format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default()
            }),
        })
    }

    pub async fn set_project_source(
        &mut self,
        project_id: Uuid,
        repository_id: Uuid,
        target_branch: &str,
        default_secret: &str,
    ) -> Result<()> {
        let query = r#"
            UPDATE projects
            SET repository_id = $2,
                target_branch = $3,
                webhook_secret = COALESCE(webhook_secret, $4)
            WHERE id = $1
        "#;
        let q = sqlx::query(query)
            .bind(project_id)
            .bind(repository_id)
            .bind(target_branch)
            .bind(default_secret);
        self.executor
            .execute(q)
            .await
            .context("failed to update project source")?;
        Ok(())
    }

    pub async fn disconnect_project_source(&mut self, project_id: Uuid) -> Result<()> {
        let query = "UPDATE projects SET repository_id = NULL WHERE id = $1";
        let q = sqlx::query(query).bind(project_id);
        self.executor
            .execute(q)
            .await
            .context("failed to disconnect project source")?;
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProjectSourceDetails {
    pub repository: Option<ProviderRepositoryRecord>,
    pub provider: Option<String>,
    pub target_branch: Option<String>,
    pub webhook_url: Option<String>,
    pub has_webhook_secret: bool,
    pub last_delivery_at: Option<String>,
}

pub struct DeploymentRepository<'a> {
    executor: DbExecutor<'a>,
}

fn parse_status(s: &str) -> DeploymentStatus {
    match s {
        "building" => DeploymentStatus::Building,
        "running" => DeploymentStatus::Running,
        "failed" => DeploymentStatus::Failed,
        "stopped" => DeploymentStatus::Stopped,
        _ => DeploymentStatus::Pending,
    }
}

fn status_to_str(s: DeploymentStatus) -> &'static str {
    match s {
        DeploymentStatus::Pending => "pending",
        DeploymentStatus::Building => "building",
        DeploymentStatus::Running => "running",
        DeploymentStatus::Failed => "failed",
        DeploymentStatus::Stopped => "stopped",
    }
}

impl<'a> DeploymentRepository<'a> {
    pub fn new(executor: DbExecutor<'a>) -> Self {
        Self { executor }
    }

    pub async fn create_deployment(
        &mut self,
        record: CreateDeploymentRecord,
    ) -> Result<DeploymentRecord> {
        let query = r#"
            INSERT INTO deployments (id, project_id, framework, status, image_path, container_id, port, url, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        "#;
        let image_str = record.image_path.as_ref().and_then(|p| p.to_str());
        let port_i32 = record.port.map(|p| p as i32);

        let q = sqlx::query(query)
            .bind(record.id)
            .bind(record.project_id)
            .bind("static")
            .bind(status_to_str(record.status))
            .bind(image_str)
            .bind(record.container_id)
            .bind(port_i32)
            .bind(&record.url)
            .bind(record.created_at);

        self.executor
            .execute(q)
            .await
            .with_context(|| format!("failed to insert deployment {}", record.id))?;

        Ok(DeploymentRecord {
            id: record.id,
            project_id: record.project_id,
            framework: record.framework,
            status: record.status,
            image_path: record.image_path,
            container_id: record.container_id,
            port: record.port,
            url: record.url,
            error: None,
            created_at: record.created_at,
            finished_at: None,
        })
    }

    pub async fn get_deployment(&mut self, id: Uuid) -> Result<DeploymentRecord> {
        let query = "SELECT id, project_id, framework, status, image_path, container_id, port, url, error, created_at, finished_at FROM deployments WHERE id = $1";
        let q = sqlx::query(query).bind(id);
        let row = self
            .executor
            .fetch_optional(q)
            .await
            .context("failed to query deployment")?
            .ok_or_else(|| anyhow::anyhow!("deployment not found: {id}"))?;

        let status_str: String = row.get("status");
        let image_str: Option<String> = row.get("image_path");
        let port_i32: Option<i32> = row.get("port");

        Ok(DeploymentRecord {
            id: row.get("id"),
            project_id: row.get("project_id"),
            framework: Framework::Static,
            status: parse_status(&status_str),
            image_path: image_str.map(PathBuf::from),
            container_id: row.get("container_id"),
            port: port_i32.map(|p| p as u16),
            url: row.get("url"),
            error: row.get("error"),
            created_at: row.get("created_at"),
            finished_at: row.get("finished_at"),
        })
    }

    pub async fn list_deployments(&mut self, project_id: Uuid) -> Result<Vec<DeploymentRecord>> {
        let query = "SELECT id, project_id, framework, status, image_path, container_id, port, url, error, created_at, finished_at FROM deployments WHERE project_id = $1 ORDER BY created_at ASC";
        let q = sqlx::query(query).bind(project_id);
        let rows = self
            .executor
            .fetch_all(q)
            .await
            .context("failed to list deployments")?;

        let mut list = Vec::new();
        for row in rows {
            let status_str: String = row.get("status");
            let image_str: Option<String> = row.get("image_path");
            let port_i32: Option<i32> = row.get("port");
            list.push(DeploymentRecord {
                id: row.get("id"),
                project_id: row.get("project_id"),
                framework: Framework::Static,
                status: parse_status(&status_str),
                image_path: image_str.map(PathBuf::from),
                container_id: row.get("container_id"),
                port: port_i32.map(|p| p as u16),
                url: row.get("url"),
                error: row.get("error"),
                created_at: row.get("created_at"),
                finished_at: row.get("finished_at"),
            });
        }
        Ok(list)
    }

    pub async fn update_deployment_status(
        &mut self,
        id: Uuid,
        status: DeploymentStatus,
        port: Option<u16>,
        url: Option<String>,
        finished_at: Option<OffsetDateTime>,
        error: Option<String>,
    ) -> Result<()> {
        let query = r#"
            UPDATE deployments
            SET status = $2,
                port = COALESCE($3, port),
                url = COALESCE($4, url),
                finished_at = COALESCE($5, finished_at),
                error = COALESCE($6, error)
            WHERE id = $1
        "#;
        let port_i32 = port.map(|p| p as i32);

        let q = sqlx::query(query)
            .bind(id)
            .bind(status_to_str(status))
            .bind(port_i32)
            .bind(url)
            .bind(finished_at)
            .bind(error);

        self.executor
            .execute(q)
            .await
            .with_context(|| format!("failed to update deployment {id}"))?;

        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateAuditEventRecord {
    pub id: Uuid,
    pub user_id: Option<Uuid>,
    pub action: String,
    pub target_type: String,
    pub target_id: Uuid,
    pub metadata: serde_json::Value,
    pub occurred_at: OffsetDateTime,
}

pub struct AuditRepository<'a> {
    executor: DbExecutor<'a>,
}

impl<'a> AuditRepository<'a> {
    pub fn new(executor: DbExecutor<'a>) -> Self {
        Self { executor }
    }

    pub async fn record(&mut self, event: CreateAuditEventRecord) -> Result<()> {
        let query = r#"
            INSERT INTO audit_events (id, user_id, action, target_type, target_id, metadata, occurred_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#;
        let q = sqlx::query(query)
            .bind(event.id)
            .bind(event.user_id)
            .bind(&event.action)
            .bind(&event.target_type)
            .bind(event.target_id)
            .bind(event.metadata)
            .bind(event.occurred_at);

        self.executor
            .execute(q)
            .await
            .context("failed to insert audit event")?;
        Ok(())
    }

    pub async fn list_by_user(&mut self, user_id: Uuid) -> Result<Vec<CreateAuditEventRecord>> {
        let query = "SELECT id, user_id, action, target_type, target_id, metadata, occurred_at FROM audit_events WHERE user_id = $1 ORDER BY occurred_at DESC";
        let q = sqlx::query(query).bind(user_id);
        let rows = self
            .executor
            .fetch_all(q)
            .await
            .context("failed to list audit events")?;

        let mut list = Vec::new();
        for row in rows {
            list.push(CreateAuditEventRecord {
                id: row.get("id"),
                user_id: row.get("user_id"),
                action: row.get("action"),
                target_type: row.get("target_type"),
                target_id: row.get("target_id"),
                metadata: row.get("metadata"),
                occurred_at: row.get("occurred_at"),
            });
        }
        Ok(list)
    }
}

use crate::providers::{Provider, RemoteRepository};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderConnectionRecord {
    pub id: Uuid,
    pub user_id: Uuid,
    pub provider: Provider,
    pub external_user_id: String,
    pub access_token_encrypted: String,
    pub refresh_token_encrypted: Option<String>,
    pub expires_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpsertConnectionInput {
    pub id: Uuid,
    pub user_id: Uuid,
    pub provider: Provider,
    pub external_user_id: String,
    pub access_token_encrypted: String,
    pub refresh_token_encrypted: Option<String>,
    pub expires_at: Option<OffsetDateTime>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ProviderRepositoryRecord {
    pub id: Uuid,
    pub connection_id: Uuid,
    pub external_id: String,
    pub full_name: String,
    pub clone_url: String,
    pub default_branch: String,
    #[serde(with = "time::serde::rfc3339")]
    pub synced_at: OffsetDateTime,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthStateRecord {
    pub state: String,
    pub user_id: Uuid,
    pub provider: Provider,
    pub redirect_url: Option<String>,
    pub expires_at: OffsetDateTime,
    pub created_at: OffsetDateTime,
}

pub struct ProviderRepository<'a> {
    executor: DbExecutor<'a>,
}

impl<'a> ProviderRepository<'a> {
    pub fn new(executor: DbExecutor<'a>) -> Self {
        Self { executor }
    }

    pub async fn save_oauth_state(
        &mut self,
        state: &str,
        user_id: Uuid,
        provider: Provider,
        redirect_url: Option<&str>,
        expires_at: OffsetDateTime,
        created_at: OffsetDateTime,
    ) -> Result<()> {
        let query = "INSERT INTO oauth_states (state, user_id, provider, redirect_url, expires_at, created_at) VALUES ($1, $2, $3, $4, $5, $6)";
        let q = sqlx::query(query)
            .bind(state)
            .bind(user_id)
            .bind(provider.to_string())
            .bind(redirect_url)
            .bind(expires_at)
            .bind(created_at);
        self.executor
            .execute(q)
            .await
            .context("failed to insert oauth state")?;
        Ok(())
    }

    pub async fn consume_oauth_state(&mut self, state: &str) -> Result<Option<OAuthStateRecord>> {
        let query = "DELETE FROM oauth_states WHERE state = $1 AND expires_at > NOW() RETURNING state, user_id, provider, redirect_url, expires_at, created_at";
        let q = sqlx::query(query).bind(state);
        let row_opt = self
            .executor
            .fetch_optional(q)
            .await
            .context("failed to consume oauth state")?;
        if let Some(row) = row_opt {
            let provider_str: String = row.get("provider");
            let provider = provider_str
                .parse()
                .context("invalid provider in oauth state")?;
            Ok(Some(OAuthStateRecord {
                state: row.get("state"),
                user_id: row.get("user_id"),
                provider,
                redirect_url: row.get("redirect_url"),
                expires_at: row.get("expires_at"),
                created_at: row.get("created_at"),
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn upsert_connection(
        &mut self,
        input: UpsertConnectionInput,
    ) -> Result<ProviderConnectionRecord> {
        let query = r#"
            INSERT INTO provider_connections (id, user_id, provider, external_user_id, access_token_encrypted, refresh_token_encrypted, expires_at, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            ON CONFLICT (user_id, provider) DO UPDATE SET
                external_user_id = EXCLUDED.external_user_id,
                access_token_encrypted = EXCLUDED.access_token_encrypted,
                refresh_token_encrypted = EXCLUDED.refresh_token_encrypted,
                expires_at = EXCLUDED.expires_at,
                updated_at = EXCLUDED.updated_at
            RETURNING id, user_id, provider, external_user_id, access_token_encrypted, refresh_token_encrypted, expires_at, created_at, updated_at
        "#;
        let q = sqlx::query(query)
            .bind(input.id)
            .bind(input.user_id)
            .bind(input.provider.to_string())
            .bind(input.external_user_id)
            .bind(input.access_token_encrypted)
            .bind(input.refresh_token_encrypted)
            .bind(input.expires_at)
            .bind(input.created_at)
            .bind(input.updated_at);
        let row = self
            .executor
            .fetch_optional(q)
            .await
            .context("failed to upsert provider connection")?
            .ok_or_else(|| anyhow::anyhow!("failed to return upserted provider connection"))?;
        let provider_str: String = row.get("provider");
        Ok(ProviderConnectionRecord {
            id: row.get("id"),
            user_id: row.get("user_id"),
            provider: provider_str.parse()?,
            external_user_id: row.get("external_user_id"),
            access_token_encrypted: row.get("access_token_encrypted"),
            refresh_token_encrypted: row.get("refresh_token_encrypted"),
            expires_at: row.get("expires_at"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
    }

    pub async fn get_connection(
        &mut self,
        user_id: Uuid,
        provider: Provider,
    ) -> Result<Option<ProviderConnectionRecord>> {
        let query = "SELECT id, user_id, provider, external_user_id, access_token_encrypted, refresh_token_encrypted, expires_at, created_at, updated_at FROM provider_connections WHERE user_id = $1 AND provider = $2";
        let q = sqlx::query(query).bind(user_id).bind(provider.to_string());
        let row_opt = self
            .executor
            .fetch_optional(q)
            .await
            .context("failed to get provider connection")?;
        if let Some(row) = row_opt {
            let provider_str: String = row.get("provider");
            Ok(Some(ProviderConnectionRecord {
                id: row.get("id"),
                user_id: row.get("user_id"),
                provider: provider_str.parse()?,
                external_user_id: row.get("external_user_id"),
                access_token_encrypted: row.get("access_token_encrypted"),
                refresh_token_encrypted: row.get("refresh_token_encrypted"),
                expires_at: row.get("expires_at"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn save_repositories(
        &mut self,
        connection_id: Uuid,
        repos: &[RemoteRepository],
        synced_at: OffsetDateTime,
    ) -> Result<()> {
        for repo in repos {
            let query = r#"
                INSERT INTO provider_repositories (id, connection_id, external_id, full_name, clone_url, default_branch, synced_at)
                VALUES ($1, $2, $3, $4, $5, $6, $7)
                ON CONFLICT (connection_id, external_id) DO UPDATE SET
                    full_name = EXCLUDED.full_name,
                    clone_url = EXCLUDED.clone_url,
                    default_branch = EXCLUDED.default_branch,
                    synced_at = EXCLUDED.synced_at
            "#;
            let q = sqlx::query(query)
                .bind(Uuid::new_v4())
                .bind(connection_id)
                .bind(&repo.external_id)
                .bind(&repo.full_name)
                .bind(repo.clone_url.as_str())
                .bind(&repo.default_branch)
                .bind(synced_at);
            self.executor
                .execute(q)
                .await
                .context("failed to save repository")?;
        }
        Ok(())
    }

    pub async fn list_repositories(
        &mut self,
        user_id: Uuid,
        provider: Provider,
    ) -> Result<Vec<ProviderRepositoryRecord>> {
        let query = r#"
            SELECT r.id, r.connection_id, r.external_id, r.full_name, r.clone_url, r.default_branch, r.synced_at
            FROM provider_repositories r
            JOIN provider_connections c ON r.connection_id = c.id
            WHERE c.user_id = $1 AND c.provider = $2
            ORDER BY r.full_name ASC
        "#;
        let q = sqlx::query(query).bind(user_id).bind(provider.to_string());
        let rows = self
            .executor
            .fetch_all(q)
            .await
            .context("failed to list provider repositories")?;
        let mut list = Vec::new();
        for row in rows {
            list.push(ProviderRepositoryRecord {
                id: row.get("id"),
                connection_id: row.get("connection_id"),
                external_id: row.get("external_id"),
                full_name: row.get("full_name"),
                clone_url: row.get("clone_url"),
                default_branch: row.get("default_branch"),
                synced_at: row.get("synced_at"),
            });
        }
        Ok(list)
    }
}
