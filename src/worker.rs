use crate::artifacts::{ArtifactMeta, ArtifactStore};
use crate::build_executor::{BuildExecutor, LogSink, LogStream};
use crate::framework::detect_build_plan;
use crate::queue::{BuildQueue, ClaimedJob};
use crate::source_checkout::checkout_commit;
use anyhow::{Context, Result};
use sqlx::{PgPool, Row};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuildWorkerOutcome {
    Completed(Uuid),
    Retried(Uuid),
    Failed(Uuid),
    Cancelled(Uuid),
    Idle,
}

pub struct BuildWorker {
    pub worker_id: String,
    pub pool: PgPool,
    pub queue: BuildQueue,
    pub executor: Arc<dyn BuildExecutor>,
    pub artifact_store: ArtifactStore,
    pub workspace_root: PathBuf,
    pub lease_duration: Duration,
}

impl BuildWorker {
    pub fn new(
        worker_id: impl Into<String>,
        pool: PgPool,
        queue: BuildQueue,
        executor: Arc<dyn BuildExecutor>,
        artifact_store: ArtifactStore,
        workspace_root: PathBuf,
    ) -> Self {
        Self {
            worker_id: worker_id.into(),
            pool,
            queue,
            executor,
            artifact_store,
            workspace_root,
            lease_duration: Duration::from_secs(30),
        }
    }

    pub fn with_lease_duration(mut self, duration: Duration) -> Self {
        self.lease_duration = duration;
        self
    }

    pub async fn run_once(&self) -> Result<BuildWorkerOutcome> {
        let lease_dur_time = time::Duration::milliseconds(self.lease_duration.as_millis() as i64);
        let claimed = match self.queue.claim(&self.worker_id, lease_dur_time).await? {
            Some(c) => c,
            None => return Ok(BuildWorkerOutcome::Idle),
        };

        let job_id = claimed.job.id;
        let deployment_id = claimed.job.deployment_id;
        let stream_name = claimed.stream_name.clone();
        let stream_id = claimed.stream_id.clone();

        // Check if job is cancelled
        let job_status: Option<String> =
            sqlx::query_scalar("SELECT status FROM build_jobs WHERE id = $1")
                .bind(job_id)
                .fetch_optional(&self.pool)
                .await?;
        if job_status.as_deref() == Some("cancelled") {
            let _ = self
                .queue
                .ack(job_id, &self.worker_id, &stream_name, &stream_id)
                .await;
            return Ok(BuildWorkerOutcome::Cancelled(job_id));
        }

        // Record build attempt
        let attempt_id = Uuid::new_v4();
        let now = OffsetDateTime::now_utc();
        sqlx::query(
            r#"
            INSERT INTO build_attempts (id, job_id, attempt_number, worker_id, status, started_at)
            VALUES ($1, $2, $3, $4, 'running', $5)
            ON CONFLICT (job_id, attempt_number) DO UPDATE
            SET worker_id = $4, status = 'running', started_at = $5, finished_at = NULL, error_message = NULL
            "#,
        )
        .bind(attempt_id)
        .bind(job_id)
        .bind(claimed.attempt_number)
        .bind(&self.worker_id)
        .bind(now)
        .execute(&self.pool)
        .await?;

        // Publish build.started event
        self.publish_event(
            job_id,
            deployment_id,
            "build.started",
            serde_json::json!({
                "worker_id": self.worker_id,
                "attempt": claimed.attempt_number,
            }),
        )
        .await?;

        // Setup workspace
        let job_workspace = self.workspace_root.join(job_id.to_string());
        if let Err(e) = fs::create_dir_all(&job_workspace) {
            let err_msg = format!("failed to create workspace: {e}");
            self.queue
                .fail(
                    job_id,
                    &self.worker_id,
                    &stream_name,
                    &stream_id,
                    &err_msg,
                    true,
                )
                .await?;
            return Ok(BuildWorkerOutcome::Retried(job_id));
        }

        // Lease renewer in background
        let (stop_tx, mut stop_rx) = tokio::sync::oneshot::channel();
        let queue_clone = self.queue.clone();
        let worker_id_clone = self.worker_id.clone();
        let lease_dur = self.lease_duration;
        let renew_tick = (lease_dur / 3).max(Duration::from_millis(500));
        let renewer = tokio::spawn(async move {
            let mut interval = tokio::time::interval(renew_tick);
            interval.tick().await; // first immediate tick
            loop {
                tokio::select! {
                    _ = &mut stop_rx => break,
                    _ = interval.tick() => {
                        let dur_time = time::Duration::milliseconds(lease_dur.as_millis() as i64);
                        let _ = queue_clone.renew(job_id, &worker_id_clone, dur_time).await;
                    }
                }
            }
        });

        // Run build pipeline
        let res = self.execute_pipeline(&claimed, &job_workspace).await;

        // Stop renewer
        let _ = stop_tx.send(());
        let _ = renewer.await;

        // Cleanup workspace
        let _ = fs::remove_dir_all(&job_workspace);

        match res {
            Ok(artifact_meta) => {
                let finish_time = OffsetDateTime::now_utc();
                let _ = sqlx::query(
                    "UPDATE deployments SET status = 'running', image_path = $1, finished_at = $2 WHERE id = $3",
                )
                .bind(artifact_meta.file_path.to_string_lossy().to_string())
                .bind(finish_time)
                .bind(deployment_id)
                .execute(&self.pool)
                .await;

                self.publish_event(
                    job_id,
                    deployment_id,
                    "build.succeeded",
                    serde_json::json!({
                        "artifact_id": artifact_meta.id,
                        "sha256": artifact_meta.sha256,
                        "size_bytes": artifact_meta.size_bytes,
                    }),
                )
                .await?;

                self.queue
                    .ack(job_id, &self.worker_id, &stream_name, &stream_id)
                    .await?;
                Ok(BuildWorkerOutcome::Completed(job_id))
            }
            Err(err) => {
                let err_str = err.to_string();
                let retryable = is_retryable_error(&err_str);
                let finish_time = OffsetDateTime::now_utc();

                if !retryable {
                    let _ = sqlx::query(
                        "UPDATE deployments SET status = 'failed', error = $1, finished_at = $2 WHERE id = $3",
                    )
                    .bind(&err_str)
                    .bind(finish_time)
                    .bind(deployment_id)
                    .execute(&self.pool)
                    .await;

                    self.publish_event(
                        job_id,
                        deployment_id,
                        "build.failed",
                        serde_json::json!({
                            "error": err_str,
                            "retryable": false,
                        }),
                    )
                    .await?;

                    self.queue
                        .fail(
                            job_id,
                            &self.worker_id,
                            &stream_name,
                            &stream_id,
                            &err_str,
                            false,
                        )
                        .await?;
                    Ok(BuildWorkerOutcome::Failed(job_id))
                } else {
                    self.publish_event(
                        job_id,
                        deployment_id,
                        "build.retry",
                        serde_json::json!({
                            "error": err_str,
                            "retryable": true,
                        }),
                    )
                    .await?;

                    self.queue
                        .fail(
                            job_id,
                            &self.worker_id,
                            &stream_name,
                            &stream_id,
                            &err_str,
                            true,
                        )
                        .await?;
                    Ok(BuildWorkerOutcome::Retried(job_id))
                }
            }
        }
    }

    async fn execute_pipeline(
        &self,
        claimed: &ClaimedJob,
        workspace: &Path,
    ) -> Result<ArtifactMeta> {
        let project_id = claimed.job.project_id;
        let deployment_id = claimed.job.deployment_id;

        // Fetch project
        let proj_row = sqlx::query(
            "SELECT name, source_dir, base_image, server_command, repository_id, target_branch
             FROM projects WHERE id = $1",
        )
        .bind(project_id)
        .fetch_one(&self.pool)
        .await
        .context("failed to fetch project for build")?;

        let source_dir: String = proj_row.get("source_dir");
        let repository_id: Option<Uuid> = proj_row.get("repository_id");

        let commit_sha: Option<String> =
            sqlx::query_scalar("SELECT commit_sha FROM build_jobs WHERE id = $1")
                .bind(claimed.job.id)
                .fetch_optional(&self.pool)
                .await
                .unwrap_or(None)
                .flatten();

        let source_path = workspace.join("source");

        if let (Some(repo_id), Some(commit)) = (repository_id, commit_sha.as_deref()) {
            let repo_row = sqlx::query(
                "SELECT c.provider, r.clone_url, r.default_branch, r.external_id
                 FROM provider_repositories r
                 JOIN provider_connections c ON r.connection_id = c.id
                 WHERE r.id = $1",
            )
            .bind(repo_id)
            .fetch_optional(&self.pool)
            .await?;

            if let Some(r_row) = repo_row {
                let prov_str: String = r_row.get("provider");
                let clone_url_str: String = r_row.get("clone_url");
                let def_branch: String = r_row.get("default_branch");
                let ext_id: String = r_row.get("external_id");
                let provider: crate::providers::Provider = prov_str.parse()?;
                let clone_url = url::Url::parse(&clone_url_str)?;

                let repo_ref = crate::providers::RepositoryRef {
                    provider,
                    external_id: ext_id,
                    clone_url,
                    default_branch: def_branch,
                };
                checkout_commit(&repo_ref, commit, workspace)?;
            } else {
                copy_dir_recursive(Path::new(&source_dir), &source_path)?;
            }
        } else {
            copy_dir_recursive(Path::new(&source_dir), &source_path)?;
        }

        let plan = detect_build_plan(&source_path)?;

        // Cache checking
        let force_rebuild = claimed.job.force_rebuild;

        let lockfile_digest = compute_lockfile_digest(&source_path);
        let declared_env_keys: Vec<String> = sqlx::query_scalar(
            "SELECT key FROM environment_variables WHERE project_id = $1 ORDER BY key ASC",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();

        let cache_input = crate::cache::BuildCacheInput {
            schema_version: "v1".to_string(),
            framework: format!("{:?}", plan.framework).to_lowercase(),
            toolchain: "default".to_string(),
            build_command: format!("{} ; {}", plan.install.join(" "), plan.build.join(" "))
                .trim()
                .to_string(),
            lockfile_digest,
            declared_env_keys,
        };
        let cache_key = crate::cache::CacheKey::from_build_inputs(&cache_input);

        let mut cache_repo = crate::cache::BuildCacheRepository::new(
            crate::repository::DbExecutor::Pool(&self.pool),
        );

        if !force_rebuild {
            if let Ok(Some(cached_entry)) =
                cache_repo.get_valid(project_id, cache_key.as_str()).await
            {
                let cached_path = PathBuf::from(&cached_entry.storage_path);
                if cached_path.is_file() {
                    if let Ok(()) = self
                        .artifact_store
                        .verify_checksum(&cached_path, &cached_entry.artifact_checksum)
                    {
                        // Cache hit! Copy cached artifact for this deployment
                        let meta = self
                            .artifact_store
                            .store_artifact(deployment_id, &cached_path)?;

                        // Update last-used asynchronously
                        let pool_clone = self.pool.clone();
                        let c_key = cache_key.as_str().to_string();
                        tokio::spawn(async move {
                            let mut repo = crate::cache::BuildCacheRepository::new(
                                crate::repository::DbExecutor::Pool(&pool_clone),
                            );
                            let _ = repo.touch_used(project_id, &c_key).await;
                        });

                        self.publish_event(
                            claimed.job.id,
                            deployment_id,
                            "build.cache.checked",
                            serde_json::json!({
                                "hit": true,
                                "cache_key": cache_key.as_str(),
                                "checksum": cached_entry.artifact_checksum,
                            }),
                        )
                        .await?;

                        return Ok(meta);
                    }
                }
            }
        }

        // Cache miss or force rebuild
        self.publish_event(
            claimed.job.id,
            deployment_id,
            "build.cache.checked",
            serde_json::json!({
                "hit": false,
                "reason": if force_rebuild { "force_rebuild" } else { "cache_miss" },
                "cache_key": cache_key.as_str(),
            }),
        )
        .await?;

        let secret_fingerprints: Vec<String> =
            sqlx::query_scalar("SELECT value FROM environment_variables WHERE project_id = $1")
                .bind(project_id)
                .fetch_all(&self.pool)
                .await
                .unwrap_or_default();

        let redactor = crate::logs::Redactor::new(secret_fingerprints);
        let storage_dir = self.workspace_root.join("logs");
        let mut sink = crate::logs::DurableLogSink::new(
            deployment_id,
            project_id,
            self.pool.clone(),
            redactor,
            Some(storage_dir),
            100,
        );

        let build_res = self.executor.execute(&plan, &source_path, &mut sink)?;
        let _ = sink.flush();
        sink.wait_all().await;
        let meta = self
            .artifact_store
            .store_artifact(deployment_id, &build_res.artifact_path)?;

        // Publish to cache (failure to publish must preserve build success)
        let toolchain = "default".to_string();
        let store_input = crate::cache::StoreCacheEntryInput {
            cache_key: cache_key.as_str().to_string(),
            project_id,
            artifact_checksum: meta.sha256.clone(),
            size_bytes: meta.size_bytes,
            storage_path: meta.file_path.to_string_lossy().to_string(),
            toolchain,
        };
        let _ = cache_repo.store(&store_input).await;

        Ok(meta)
    }

    pub async fn run_loop(&self, mut shutdown: tokio::sync::watch::Receiver<bool>) -> Result<()> {
        while !*shutdown.borrow() {
            tokio::select! {
                _ = shutdown.changed() => {
                    break;
                }
                outcome = self.run_once() => {
                    match outcome {
                        Ok(BuildWorkerOutcome::Idle) => {
                            tokio::time::sleep(Duration::from_millis(500)).await;
                        }
                        Ok(_) => {}
                        Err(e) => {
                            eprintln!("worker run error: {e}");
                            tokio::time::sleep(Duration::from_millis(500)).await;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    async fn publish_event(
        &self,
        job_id: Uuid,
        deployment_id: Uuid,
        event_type: &str,
        payload: serde_json::Value,
    ) -> Result<()> {
        let now = OffsetDateTime::now_utc();
        sqlx::query(
            "INSERT INTO build_events (id, job_id, deployment_id, event_type, payload, created_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(Uuid::new_v4())
        .bind(job_id)
        .bind(deployment_id)
        .bind(event_type)
        .bind(payload)
        .bind(now)
        .execute(&self.pool)
        .await
        .context("failed to publish build event")?;
        Ok(())
    }
}

pub fn is_retryable_error(msg: &str) -> bool {
    let lower = msg.to_lowercase();
    if lower.contains("invalid package.json")
        || lower.contains("ambiguous framework")
        || lower.contains("unsupported framework")
        || lower.contains("traversal")
        || lower.contains("failed with status")
        || lower.contains("exit code")
        || lower.contains("build output path does not exist")
        || lower.contains("cannot be empty")
    {
        return false;
    }
    true
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let ft = entry.file_type()?;
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if ft.is_dir() {
            copy_dir_recursive(&from, &to)?;
        } else if ft.is_file() {
            fs::copy(&from, &to)?;
        }
    }
    Ok(())
}

pub struct DatabaseLogSink {
    pool: PgPool,
    job_id: Uuid,
    deployment_id: Uuid,
    sequence: u64,
}

impl LogSink for DatabaseLogSink {
    fn write_line(&mut self, stream: LogStream, message: &str) -> Result<()> {
        self.sequence += 1;
        let event_id = Uuid::new_v4();
        let now = OffsetDateTime::now_utc();
        let payload = serde_json::json!({
            "sequence": self.sequence,
            "stream": stream,
            "message": message,
        });

        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let pool = self.pool.clone();
            let job_id = self.job_id;
            let deployment_id = self.deployment_id;
            handle.spawn(async move {
                let _ = sqlx::query(
                    "INSERT INTO build_events (id, job_id, deployment_id, event_type, payload, created_at)
                     VALUES ($1, $2, $3, 'build.log', $4, $5)",
                )
                .bind(event_id)
                .bind(job_id)
                .bind(deployment_id)
                .bind(payload)
                .bind(now)
                .execute(&pool)
                .await;
            });
        }
        Ok(())
    }
}

fn compute_lockfile_digest(source_path: &Path) -> String {
    use sha2::{Digest, Sha256};
    let lockfiles = [
        "package-lock.json",
        "yarn.lock",
        "pnpm-lock.yaml",
        "Cargo.lock",
        "Gemfile.lock",
        "poetry.lock",
        "requirements.txt",
    ];

    let mut hasher = Sha256::new();
    let mut found_any = false;
    for lf in &lockfiles {
        let path = source_path.join(lf);
        if let Ok(contents) = fs::read(&path) {
            hasher.update(lf.as_bytes());
            hasher.update(&contents);
            found_any = true;
        }
    }

    if !found_any {
        if let Ok(entries) = fs::read_dir(source_path) {
            let mut paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
            paths.sort();
            for path in paths {
                if let Some(name) = path.file_name() {
                    hasher.update(name.to_string_lossy().as_bytes());
                }
                if let Ok(meta) = fs::metadata(&path) {
                    hasher.update(meta.len().to_le_bytes());
                    if path.is_file() {
                        if let Ok(bytes) = fs::read(&path) {
                            hasher.update(&bytes);
                        }
                    }
                }
            }
        }
    }

    hex::encode(hasher.finalize())
}
