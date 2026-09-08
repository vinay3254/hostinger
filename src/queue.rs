use crate::db::Database;
use anyhow::{Context, Result};
use redis::aio::MultiplexedConnection;
use sqlx::Row;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobPriority {
    Production,
    Preview,
}

impl JobPriority {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Production => "production",
            Self::Preview => "preview",
        }
    }
}

impl std::str::FromStr for JobPriority {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "production" => Ok(Self::Production),
            "preview" => Ok(Self::Preview),
            other => anyhow::bail!("invalid job priority: {other}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

impl std::str::FromStr for JobStatus {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            other => anyhow::bail!("invalid job status: {other}"),
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct BuildJobRecord {
    pub id: Uuid,
    pub deployment_id: Uuid,
    pub project_id: Uuid,
    pub priority: JobPriority,
    pub status: JobStatus,
    pub attempt: i32,
    pub max_attempts: i32,
    pub lease_owner: Option<String>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub lease_expires_at: Option<OffsetDateTime>,
    pub last_error: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub updated_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct EnqueueJobInput {
    pub deployment_id: Uuid,
    pub project_id: Uuid,
    pub priority: JobPriority,
    pub max_attempts: i32,
}

#[derive(Debug, Clone)]
pub struct ClaimedJob {
    pub job: BuildJobRecord,
    pub attempt_number: i32,
    pub stream_name: String,
    pub stream_id: String,
}

#[derive(Clone)]
pub struct BuildQueue {
    db: Database,
    redis_client: redis::Client,
    consumer_group: String,
    stream_prefix: String,
    prod_claims_counter: Arc<AtomicUsize>,
}

impl BuildQueue {
    pub async fn new(
        db: Database,
        redis_client: redis::Client,
        consumer_group: String,
    ) -> Result<Self> {
        Self::with_prefix(db, redis_client, consumer_group, "build_jobs".to_string()).await
    }

    pub async fn with_prefix(
        db: Database,
        redis_client: redis::Client,
        consumer_group: String,
        stream_prefix: String,
    ) -> Result<Self> {
        let queue = Self {
            db,
            redis_client,
            consumer_group,
            stream_prefix,
            prod_claims_counter: Arc::new(AtomicUsize::new(0)),
        };

        queue.ensure_groups().await?;
        Ok(queue)
    }

    pub fn prod_stream(&self) -> String {
        format!("{}:production", self.stream_prefix)
    }

    pub fn preview_stream(&self) -> String {
        format!("{}:preview", self.stream_prefix)
    }

    async fn get_redis_conn(&self) -> Result<MultiplexedConnection> {
        self.redis_client
            .get_multiplexed_tokio_connection()
            .await
            .context("failed to get redis multiplexed connection")
    }

    async fn ensure_groups(&self) -> Result<()> {
        let mut conn = self.get_redis_conn().await?;
        let prod = self.prod_stream();
        let prev = self.preview_stream();
        for stream in &[&prod, &prev] {
            let res: redis::RedisResult<()> = redis::cmd("XGROUP")
                .arg("CREATE")
                .arg(*stream)
                .arg(&self.consumer_group)
                .arg("$")
                .arg("MKSTREAM")
                .query_async(&mut conn)
                .await;
            if let Err(e) = res {
                let msg = e.to_string();
                if !msg.contains("BUSYGROUP") {
                    eprintln!("XGROUP CREATE error on {stream}: {e}");
                }
            }
        }
        Ok(())
    }

    pub async fn enqueue(&self, input: EnqueueJobInput) -> Result<BuildJobRecord> {
        let now = OffsetDateTime::now_utc();
        let job_id = Uuid::new_v4();

        // 1. Insert or get existing job from postgres (idempotent by deployment_id)
        let query = r#"
            INSERT INTO build_jobs (id, deployment_id, project_id, queue_name, priority, status, max_attempts, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, 'queued', $6, $7, $7)
            ON CONFLICT (deployment_id) DO NOTHING
            RETURNING id, deployment_id, project_id, priority, status, attempt, max_attempts, lease_owner, lease_expires_at, last_error, created_at, updated_at
        "#;

        let row_opt = sqlx::query(query)
            .bind(job_id)
            .bind(input.deployment_id)
            .bind(input.project_id)
            .bind(&self.stream_prefix)
            .bind(input.priority.as_str())
            .bind(input.max_attempts)
            .bind(now)
            .fetch_optional(self.db.pool())
            .await
            .context("failed to insert build job")?;

        let record = if let Some(row) = row_opt {
            let priority_str: String = row.get("priority");
            let status_str: String = row.get("status");
            let rec = BuildJobRecord {
                id: row.get("id"),
                deployment_id: row.get("deployment_id"),
                project_id: row.get("project_id"),
                priority: priority_str.parse()?,
                status: status_str.parse()?,
                attempt: row.get("attempt"),
                max_attempts: row.get("max_attempts"),
                lease_owner: row.get("lease_owner"),
                lease_expires_at: row.get("lease_expires_at"),
                last_error: row.get("last_error"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
            };

            // 2. Publish to Redis stream
            let stream = match rec.priority {
                JobPriority::Production => self.prod_stream(),
                JobPriority::Preview => self.preview_stream(),
            };
            let mut conn = self.get_redis_conn().await?;
            let _: () = redis::cmd("XADD")
                .arg(&stream)
                .arg("*")
                .arg("job_id")
                .arg(rec.id.to_string())
                .arg("deployment_id")
                .arg(rec.deployment_id.to_string())
                .query_async(&mut conn)
                .await
                .context("failed to XADD job to redis")?;

            // Update deployment status to queued
            let _ = sqlx::query("UPDATE deployments SET status = 'queued' WHERE id = $1")
                .bind(rec.deployment_id)
                .execute(self.db.pool())
                .await;

            rec
        } else {
            // Already existed
            self.get_job_by_deployment(input.deployment_id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("job existed but could not be loaded"))?
        };

        Ok(record)
    }

    pub async fn claim(
        &self,
        worker_id: &str,
        lease_duration: Duration,
    ) -> Result<Option<ClaimedJob>> {
        // Weighted priority: 3 production jobs for every 1 preview job if available
        let count = self.prod_claims_counter.load(Ordering::Relaxed);
        let prod = self.prod_stream();
        let prev = self.preview_stream();
        let streams = if count >= 3 {
            self.prod_claims_counter.store(0, Ordering::Relaxed);
            vec![prev, prod]
        } else {
            vec![prod, prev]
        };

        let mut conn = self.get_redis_conn().await?;

        for stream in streams {
            // Read next entry using consumer group
            let res: redis::RedisResult<redis::Value> = redis::cmd("XREADGROUP")
                .arg("GROUP")
                .arg(&self.consumer_group)
                .arg(worker_id)
                .arg("COUNT")
                .arg(1)
                .arg("STREAMS")
                .arg(&stream)
                .arg(">")
                .query_async(&mut conn)
                .await;

            let Ok(val) = res else {
                continue;
            };

            let Some((stream_id, job_id)) = parse_xreadgroup_entry(&val) else {
                continue;
            };

            // Try to acquire lease in PostgreSQL
            let now = OffsetDateTime::now_utc();
            let lease_expires_at = now + lease_duration;

            let query = r#"
                UPDATE build_jobs
                SET status = 'running',
                    attempt = attempt + 1,
                    lease_owner = $2,
                    lease_expires_at = $3,
                    updated_at = $4
                WHERE id = $1 AND status IN ('queued', 'failed') AND attempt < max_attempts
                RETURNING id, deployment_id, project_id, priority, status, attempt, max_attempts, lease_owner, lease_expires_at, last_error, created_at, updated_at
            "#;

            let row_opt = sqlx::query(query)
                .bind(job_id)
                .bind(worker_id)
                .bind(lease_expires_at)
                .bind(now)
                .fetch_optional(self.db.pool())
                .await
                .context("failed to claim job in postgres")?;

            if let Some(row) = row_opt {
                let priority_str: String = row.get("priority");
                let status_str: String = row.get("status");
                let record = BuildJobRecord {
                    id: row.get("id"),
                    deployment_id: row.get("deployment_id"),
                    project_id: row.get("project_id"),
                    priority: priority_str.parse()?,
                    status: status_str.parse()?,
                    attempt: row.get("attempt"),
                    max_attempts: row.get("max_attempts"),
                    lease_owner: row.get("lease_owner"),
                    lease_expires_at: row.get("lease_expires_at"),
                    last_error: row.get("last_error"),
                    created_at: row.get("created_at"),
                    updated_at: row.get("updated_at"),
                };

                // Record attempt in build_attempts
                let attempt_query = r#"
                    INSERT INTO build_attempts (id, job_id, attempt_number, worker_id, status, started_at)
                    VALUES ($1, $2, $3, $4, 'running', $5)
                "#;
                let _ = sqlx::query(attempt_query)
                    .bind(Uuid::new_v4())
                    .bind(record.id)
                    .bind(record.attempt)
                    .bind(worker_id)
                    .bind(now)
                    .execute(self.db.pool())
                    .await;

                // Update deployment status to building
                let _ = sqlx::query("UPDATE deployments SET status = 'building' WHERE id = $1")
                    .bind(record.deployment_id)
                    .execute(self.db.pool())
                    .await;

                if stream == self.prod_stream() {
                    self.prod_claims_counter.fetch_add(1, Ordering::Relaxed);
                }

                return Ok(Some(ClaimedJob {
                    job: record.clone(),
                    attempt_number: record.attempt,
                    stream_name: stream.to_string(),
                    stream_id,
                }));
            } else {
                // Job cannot be claimed (e.g. cancelled, max attempts exceeded, or already completed)
                // Acknowledge the message so it's not pending
                let _: redis::RedisResult<()> = redis::cmd("XACK")
                    .arg(stream)
                    .arg(&self.consumer_group)
                    .arg(&stream_id)
                    .query_async(&mut conn)
                    .await;
            }
        }

        Ok(None)
    }

    pub async fn renew(
        &self,
        job_id: Uuid,
        worker_id: &str,
        lease_duration: Duration,
    ) -> Result<bool> {
        let now = OffsetDateTime::now_utc();
        let lease_expires_at = now + lease_duration;
        let query = r#"
            UPDATE build_jobs
            SET lease_expires_at = $3, updated_at = $4
            WHERE id = $1 AND lease_owner = $2 AND status = 'running'
        "#;
        let res = sqlx::query(query)
            .bind(job_id)
            .bind(worker_id)
            .bind(lease_expires_at)
            .bind(now)
            .execute(self.db.pool())
            .await
            .context("failed to renew lease in postgres")?;
        Ok(res.rows_affected() > 0)
    }

    pub async fn ack(
        &self,
        job_id: Uuid,
        worker_id: &str,
        stream_name: &str,
        stream_id: &str,
    ) -> Result<()> {
        let now = OffsetDateTime::now_utc();
        let query = r#"
            UPDATE build_jobs
            SET status = 'completed', lease_owner = NULL, lease_expires_at = NULL, updated_at = $2
            WHERE id = $1 AND lease_owner = $3 AND status = 'running'
        "#;
        sqlx::query(query)
            .bind(job_id)
            .bind(now)
            .bind(worker_id)
            .execute(self.db.pool())
            .await
            .context("failed to mark build job completed")?;

        let attempt_query = r#"
            UPDATE build_attempts
            SET status = 'succeeded', finished_at = $2
            WHERE job_id = $1 AND worker_id = $3 AND status = 'running'
        "#;
        let _ = sqlx::query(attempt_query)
            .bind(job_id)
            .bind(now)
            .bind(worker_id)
            .execute(self.db.pool())
            .await;

        let mut conn = self.get_redis_conn().await?;
        let _: redis::RedisResult<()> = redis::cmd("XACK")
            .arg(stream_name)
            .arg(&self.consumer_group)
            .arg(stream_id)
            .query_async(&mut conn)
            .await;

        Ok(())
    }

    pub async fn fail(
        &self,
        job_id: Uuid,
        worker_id: &str,
        stream_name: &str,
        stream_id: &str,
        error_message: &str,
        retryable: bool,
    ) -> Result<()> {
        let now = OffsetDateTime::now_utc();
        let job = self
            .get_job(job_id)
            .await?
            .ok_or_else(|| anyhow::anyhow!("job not found: {job_id}"))?;

        let mut conn = self.get_redis_conn().await?;

        if retryable && job.attempt < job.max_attempts {
            // Requeue job
            let query = r#"
                UPDATE build_jobs
                SET status = 'queued', lease_owner = NULL, lease_expires_at = NULL, last_error = $2, updated_at = $3
                WHERE id = $1 AND lease_owner = $4
            "#;
            sqlx::query(query)
                .bind(job_id)
                .bind(error_message)
                .bind(now)
                .bind(worker_id)
                .execute(self.db.pool())
                .await
                .context("failed to requeue failed job")?;

            let attempt_query = r#"
                UPDATE build_attempts
                SET status = 'failed', error_message = $2, finished_at = $3
                WHERE job_id = $1 AND worker_id = $4 AND status = 'running'
            "#;
            let _ = sqlx::query(attempt_query)
                .bind(job_id)
                .bind(error_message)
                .bind(now)
                .bind(worker_id)
                .execute(self.db.pool())
                .await;

            // Re-publish to stream and ack previous
            let _: () = redis::cmd("XADD")
                .arg(stream_name)
                .arg("*")
                .arg("job_id")
                .arg(job.id.to_string())
                .arg("deployment_id")
                .arg(job.deployment_id.to_string())
                .query_async(&mut conn)
                .await
                .context("failed to re-publish job to redis")?;

            let _: redis::RedisResult<()> = redis::cmd("XACK")
                .arg(stream_name)
                .arg(&self.consumer_group)
                .arg(stream_id)
                .query_async(&mut conn)
                .await;
        } else {
            // Terminal failure / dead-letter
            let query = r#"
                UPDATE build_jobs
                SET status = 'failed', lease_owner = NULL, lease_expires_at = NULL, last_error = $2, updated_at = $3
                WHERE id = $1 AND lease_owner = $4
            "#;
            sqlx::query(query)
                .bind(job_id)
                .bind(error_message)
                .bind(now)
                .bind(worker_id)
                .execute(self.db.pool())
                .await
                .context("failed to mark build job failed")?;

            let attempt_query = r#"
                UPDATE build_attempts
                SET status = 'failed', error_message = $2, finished_at = $3
                WHERE job_id = $1 AND worker_id = $4 AND status = 'running'
            "#;
            let _ = sqlx::query(attempt_query)
                .bind(job_id)
                .bind(error_message)
                .bind(now)
                .bind(worker_id)
                .execute(self.db.pool())
                .await;

            // Update deployment status to failed
            let _ =
                sqlx::query("UPDATE deployments SET status = 'failed', error = $2 WHERE id = $1")
                    .bind(job.deployment_id)
                    .bind(error_message)
                    .execute(self.db.pool())
                    .await;

            let _: redis::RedisResult<()> = redis::cmd("XACK")
                .arg(stream_name)
                .arg(&self.consumer_group)
                .arg(stream_id)
                .query_async(&mut conn)
                .await;
        }

        Ok(())
    }

    pub async fn cancel(&self, job_id: Uuid) -> Result<()> {
        let now = OffsetDateTime::now_utc();
        let query = r#"
            UPDATE build_jobs
            SET status = 'cancelled', lease_owner = NULL, lease_expires_at = NULL, updated_at = $2
            WHERE id = $1 AND status IN ('queued', 'running')
            RETURNING deployment_id
        "#;
        let row_opt = sqlx::query(query)
            .bind(job_id)
            .bind(now)
            .fetch_optional(self.db.pool())
            .await
            .context("failed to cancel build job")?;

        if let Some(row) = row_opt {
            let deployment_id: Uuid = row.get("deployment_id");
            let _ = sqlx::query("UPDATE deployments SET status = 'cancelled' WHERE id = $1")
                .bind(deployment_id)
                .execute(self.db.pool())
                .await;
        }

        Ok(())
    }

    pub async fn requeue_expired(&self) -> Result<u64> {
        let now = OffsetDateTime::now_utc();
        let query = r#"
            SELECT id, deployment_id, priority, attempt, max_attempts
            FROM build_jobs
            WHERE queue_name = $1 AND status = 'running' AND lease_expires_at < $2
        "#;
        let rows = sqlx::query(query)
            .bind(&self.stream_prefix)
            .bind(now)
            .fetch_all(self.db.pool())
            .await
            .context("failed to fetch expired build jobs")?;

        let mut count = 0;
        let mut conn = self.get_redis_conn().await?;

        for row in rows {
            let id: Uuid = row.get("id");
            let deployment_id: Uuid = row.get("deployment_id");
            let priority_str: String = row.get("priority");
            let attempt: i32 = row.get("attempt");
            let max_attempts: i32 = row.get("max_attempts");

            if attempt < max_attempts {
                let update_query = r#"
                    UPDATE build_jobs
                    SET status = 'queued', lease_owner = NULL, lease_expires_at = NULL, updated_at = $2
                    WHERE id = $1 AND status = 'running'
                "#;
                sqlx::query(update_query)
                    .bind(id)
                    .bind(now)
                    .execute(self.db.pool())
                    .await?;

                let stream = if priority_str == "production" {
                    self.prod_stream()
                } else {
                    self.preview_stream()
                };

                let _: () = redis::cmd("XADD")
                    .arg(&stream)
                    .arg("*")
                    .arg("job_id")
                    .arg(id.to_string())
                    .arg("deployment_id")
                    .arg(deployment_id.to_string())
                    .query_async(&mut conn)
                    .await?;

                count += 1;
            } else {
                // Max attempts exceeded
                let update_query = r#"
                    UPDATE build_jobs
                    SET status = 'failed', lease_owner = NULL, lease_expires_at = NULL, last_error = 'lease expired and maximum attempts exceeded', updated_at = $2
                    WHERE id = $1 AND status = 'running'
                "#;
                sqlx::query(update_query)
                    .bind(id)
                    .bind(now)
                    .execute(self.db.pool())
                    .await?;

                let _ = sqlx::query("UPDATE deployments SET status = 'failed', error = 'lease expired and maximum attempts exceeded' WHERE id = $1")
                    .bind(deployment_id)
                    .execute(self.db.pool())
                    .await;
            }
        }

        Ok(count)
    }

    pub async fn get_job(&self, job_id: Uuid) -> Result<Option<BuildJobRecord>> {
        let query = r#"
            SELECT id, deployment_id, project_id, priority, status, attempt, max_attempts, lease_owner, lease_expires_at, last_error, created_at, updated_at
            FROM build_jobs
            WHERE id = $1
        "#;
        let row_opt = sqlx::query(query)
            .bind(job_id)
            .fetch_optional(self.db.pool())
            .await
            .context("failed to get build job")?;

        let Some(row) = row_opt else {
            return Ok(None);
        };

        let priority_str: String = row.get("priority");
        let status_str: String = row.get("status");
        Ok(Some(BuildJobRecord {
            id: row.get("id"),
            deployment_id: row.get("deployment_id"),
            project_id: row.get("project_id"),
            priority: priority_str.parse()?,
            status: status_str.parse()?,
            attempt: row.get("attempt"),
            max_attempts: row.get("max_attempts"),
            lease_owner: row.get("lease_owner"),
            lease_expires_at: row.get("lease_expires_at"),
            last_error: row.get("last_error"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        }))
    }

    pub async fn get_job_by_deployment(
        &self,
        deployment_id: Uuid,
    ) -> Result<Option<BuildJobRecord>> {
        let query = r#"
            SELECT id, deployment_id, project_id, priority, status, attempt, max_attempts, lease_owner, lease_expires_at, last_error, created_at, updated_at
            FROM build_jobs
            WHERE deployment_id = $1
        "#;
        let row_opt = sqlx::query(query)
            .bind(deployment_id)
            .fetch_optional(self.db.pool())
            .await
            .context("failed to get build job by deployment")?;

        let Some(row) = row_opt else {
            return Ok(None);
        };

        let priority_str: String = row.get("priority");
        let status_str: String = row.get("status");
        Ok(Some(BuildJobRecord {
            id: row.get("id"),
            deployment_id: row.get("deployment_id"),
            project_id: row.get("project_id"),
            priority: priority_str.parse()?,
            status: status_str.parse()?,
            attempt: row.get("attempt"),
            max_attempts: row.get("max_attempts"),
            lease_owner: row.get("lease_owner"),
            lease_expires_at: row.get("lease_expires_at"),
            last_error: row.get("last_error"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        }))
    }
}

fn parse_xreadgroup_entry(val: &redis::Value) -> Option<(String, Uuid)> {
    // Redis XREADGROUP structure:
    // [[stream_name, [[stream_id, [field1, val1, field2, val2]]]]]
    if let redis::Value::Bulk(streams) = val {
        for stream_entry in streams {
            if let redis::Value::Bulk(stream_items) = stream_entry {
                if stream_items.len() >= 2 {
                    if let redis::Value::Bulk(messages) = &stream_items[1] {
                        for msg in messages {
                            if let redis::Value::Bulk(msg_parts) = msg {
                                if msg_parts.len() >= 2 {
                                    let stream_id = match &msg_parts[0] {
                                        redis::Value::Data(d) => {
                                            String::from_utf8_lossy(d).to_string()
                                        }
                                        _ => continue,
                                    };
                                    if let redis::Value::Bulk(fields) = &msg_parts[1] {
                                        for chunk in fields.chunks(2) {
                                            if chunk.len() == 2 {
                                                if let (
                                                    redis::Value::Data(k),
                                                    redis::Value::Data(v),
                                                ) = (&chunk[0], &chunk[1])
                                                {
                                                    if k.as_slice() == b"job_id" {
                                                        if let Ok(v_str) = std::str::from_utf8(v) {
                                                            if let Ok(job_id) =
                                                                Uuid::parse_str(v_str)
                                                            {
                                                                return Some((stream_id, job_id));
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    None
}
