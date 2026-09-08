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
