use crate::repository::{
    AuditRepository, DbExecutor, DeploymentRepository, ProjectRepository, ProviderRepository,
    UserRepository,
};
use crate::source_events::SourceEventRepository;
use anyhow::{Context, Result};
use sqlx::{postgres::PgPoolOptions, PgPool};
use std::path::Path;

#[derive(Clone)]
pub struct Database {
    pool: PgPool,
}

impl Database {
    pub async fn connect(url: &str) -> Result<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(url)
            .await
            .with_context(|| format!("failed to connect to database at {url}"))?;
        Ok(Self { pool })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn migrate(&self) -> Result<()> {
        let migrator = sqlx::migrate::Migrator::new(Path::new("migrations"))
            .await
            .context("failed to load migrations from migrations/")?;
        migrator
            .run(&self.pool)
            .await
            .context("failed to run database migrations")?;
        Ok(())
    }

    pub async fn begin(&self) -> Result<DbTransaction> {
        let tx = self
            .pool
            .begin()
            .await
            .context("failed to begin transaction")?;
        Ok(DbTransaction { tx })
    }

    pub fn users(&self) -> UserRepository<'_> {
        UserRepository::new(DbExecutor::Pool(&self.pool))
    }

    pub fn projects(&self) -> ProjectRepository<'_> {
        ProjectRepository::new(DbExecutor::Pool(&self.pool))
    }

    pub fn deployments(&self) -> DeploymentRepository<'_> {
        DeploymentRepository::new(DbExecutor::Pool(&self.pool))
    }

    pub fn audit(&self) -> AuditRepository<'_> {
        AuditRepository::new(DbExecutor::Pool(&self.pool))
    }

    pub fn audits(&self) -> AuditRepository<'_> {
        self.audit()
    }

    pub fn providers(&self) -> ProviderRepository<'_> {
        ProviderRepository::new(DbExecutor::Pool(&self.pool))
    }

    pub fn source_events(&self) -> SourceEventRepository<'_> {
        SourceEventRepository::new(DbExecutor::Pool(&self.pool))
    }

    pub fn previews(&self) -> crate::previews::PreviewRepository<'_> {
        crate::previews::PreviewRepository::new(DbExecutor::Pool(&self.pool))
    }

    pub fn cache(&self) -> crate::cache::BuildCacheRepository<'_> {
        crate::cache::BuildCacheRepository::new(DbExecutor::Pool(&self.pool))
    }

    pub fn logs(&self) -> crate::logs::LogRepository<'_> {
        crate::logs::LogRepository::new(DbExecutor::Pool(&self.pool))
    }
}

pub struct DbTransaction {
    tx: sqlx::Transaction<'static, sqlx::Postgres>,
}

impl DbTransaction {
    pub fn users(&mut self) -> UserRepository<'_> {
        UserRepository::new(DbExecutor::Tx(&mut self.tx))
    }

    pub fn projects(&mut self) -> ProjectRepository<'_> {
        ProjectRepository::new(DbExecutor::Tx(&mut self.tx))
    }

    pub fn deployments(&mut self) -> DeploymentRepository<'_> {
        DeploymentRepository::new(DbExecutor::Tx(&mut self.tx))
    }

    pub fn audit(&mut self) -> AuditRepository<'_> {
        AuditRepository::new(DbExecutor::Tx(&mut self.tx))
    }

    pub fn providers(&mut self) -> ProviderRepository<'_> {
        ProviderRepository::new(DbExecutor::Tx(&mut self.tx))
    }

    pub fn source_events(&mut self) -> SourceEventRepository<'_> {
        SourceEventRepository::new(DbExecutor::Tx(&mut self.tx))
    }

    pub fn previews(&mut self) -> crate::previews::PreviewRepository<'_> {
        crate::previews::PreviewRepository::new(DbExecutor::Tx(&mut self.tx))
    }

    pub fn cache(&mut self) -> crate::cache::BuildCacheRepository<'_> {
        crate::cache::BuildCacheRepository::new(DbExecutor::Tx(&mut self.tx))
    }

    pub fn logs(&mut self) -> crate::logs::LogRepository<'_> {
        crate::logs::LogRepository::new(DbExecutor::Tx(&mut self.tx))
    }

    pub async fn commit(self) -> Result<()> {
        self.tx
            .commit()
            .await
            .context("failed to commit transaction")
    }

    pub async fn rollback(self) -> Result<()> {
        self.tx
            .rollback()
            .await
            .context("failed to rollback transaction")
    }
}
