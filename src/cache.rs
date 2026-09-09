use crate::repository::DbExecutor;
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use sqlx::Row;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BuildCacheInput {
    pub schema_version: String,
    pub framework: String,
    pub toolchain: String,
    pub build_command: String,
    pub lockfile_digest: String,
    pub declared_env_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CacheKey {
    digest: String,
    canonical_json: String,
}

impl CacheKey {
    pub fn from_build_inputs(input: &BuildCacheInput) -> Self {
        let mut sorted_env_keys = input.declared_env_keys.clone();
        sorted_env_keys.sort();
        sorted_env_keys.dedup();

        let canonical_value = serde_json::json!({
            "schema_version": input.schema_version,
            "framework": input.framework,
            "toolchain": input.toolchain,
            "build_command": input.build_command,
            "lockfile_digest": input.lockfile_digest,
            "declared_env_keys": sorted_env_keys,
        });

        let canonical_json = serde_json::to_string(&canonical_value).unwrap_or_default();
        let mut hasher = Sha256::new();
        hasher.update(canonical_json.as_bytes());
        let digest = hex::encode(hasher.finalize());

        Self {
            digest,
            canonical_json,
        }
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }

    pub fn as_str(&self) -> &str {
        &self.digest
    }

    pub fn canonical_json(&self) -> &str {
        &self.canonical_json
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BuildCacheEntry {
    pub id: Uuid,
    pub cache_key: String,
    pub project_id: Uuid,
    pub artifact_checksum: String,
    pub size_bytes: u64,
    pub storage_path: String,
    pub toolchain: String,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub last_used_at: OffsetDateTime,
    pub is_invalidated: bool,
}

#[derive(Debug, Clone)]
pub struct StoreCacheEntryInput {
    pub cache_key: String,
    pub project_id: Uuid,
    pub artifact_checksum: String,
    pub size_bytes: u64,
    pub storage_path: String,
    pub toolchain: String,
}

pub struct BuildCacheRepository<'a> {
    executor: DbExecutor<'a>,
}

impl<'a> BuildCacheRepository<'a> {
    pub fn new(executor: DbExecutor<'a>) -> Self {
        Self { executor }
    }

    pub async fn store(&mut self, input: &StoreCacheEntryInput) -> Result<BuildCacheEntry> {
        let id = Uuid::new_v4();
        let now = OffsetDateTime::now_utc();
        let query = r#"
            INSERT INTO build_cache (
                id, cache_key, project_id, artifact_checksum, size_bytes, storage_path, toolchain,
                created_at, last_used_at, is_invalidated
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $8, false)
            ON CONFLICT (cache_key) DO UPDATE SET
                artifact_checksum = EXCLUDED.artifact_checksum,
                size_bytes = EXCLUDED.size_bytes,
                storage_path = EXCLUDED.storage_path,
                toolchain = EXCLUDED.toolchain,
                last_used_at = EXCLUDED.last_used_at,
                is_invalidated = false
            RETURNING
                id, cache_key, project_id, artifact_checksum, size_bytes, storage_path, toolchain,
                created_at, last_used_at, is_invalidated
        "#;

        let row = self
            .executor
            .fetch_one(
                sqlx::query(query)
                    .bind(id)
                    .bind(&input.cache_key)
                    .bind(input.project_id)
                    .bind(&input.artifact_checksum)
                    .bind(input.size_bytes as i64)
                    .bind(&input.storage_path)
                    .bind(&input.toolchain)
                    .bind(now),
            )
            .await
            .context("failed to store cache entry")?;

        map_row_to_cache_entry(&row)
    }

    pub async fn get_valid(&mut self, cache_key: &str) -> Result<Option<BuildCacheEntry>> {
        let query = r#"
            SELECT
                id, cache_key, project_id, artifact_checksum, size_bytes, storage_path, toolchain,
                created_at, last_used_at, is_invalidated
            FROM build_cache
            WHERE cache_key = $1 AND is_invalidated = false
        "#;

        let row_opt = self
            .executor
            .fetch_optional(sqlx::query(query).bind(cache_key))
            .await
            .context("failed to query build cache")?;

        row_opt.map(|r| map_row_to_cache_entry(&r)).transpose()
    }

    pub async fn touch_used(&mut self, cache_key: &str) -> Result<()> {
        let now = OffsetDateTime::now_utc();
        let query = r#"
            UPDATE build_cache
            SET last_used_at = $2
            WHERE cache_key = $1
        "#;

        self.executor
            .execute(sqlx::query(query).bind(cache_key).bind(now))
            .await
            .context("failed to touch cache entry")?;

        Ok(())
    }

    pub async fn invalidate(&mut self, cache_key: &str) -> Result<()> {
        let query = r#"
            UPDATE build_cache
            SET is_invalidated = true
            WHERE cache_key = $1
        "#;

        self.executor
            .execute(sqlx::query(query).bind(cache_key))
            .await
            .context("failed to invalidate cache entry")?;

        Ok(())
    }

    pub async fn invalidate_project(&mut self, project_id: Uuid) -> Result<()> {
        let query = r#"
            UPDATE build_cache
            SET is_invalidated = true
            WHERE project_id = $1
        "#;

        self.executor
            .execute(sqlx::query(query).bind(project_id))
            .await
            .context("failed to invalidate project cache entries")?;

        Ok(())
    }
}

fn map_row_to_cache_entry(row: &sqlx::postgres::PgRow) -> Result<BuildCacheEntry> {
    let size_bytes_i64: i64 = row.try_get("size_bytes")?;

    Ok(BuildCacheEntry {
        id: row.try_get("id")?,
        cache_key: row.try_get("cache_key")?,
        project_id: row.try_get("project_id")?,
        artifact_checksum: row.try_get("artifact_checksum")?,
        size_bytes: size_bytes_i64 as u64,
        storage_path: row.try_get("storage_path")?,
        toolchain: row.try_get("toolchain")?,
        created_at: row.try_get("created_at")?,
        last_used_at: row.try_get("last_used_at")?,
        is_invalidated: row.try_get("is_invalidated")?,
    })
}
