use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::build_executor::{LogSink, LogStream};
use crate::repository::DbExecutor;
use sqlx::Row;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LogEntry {
    pub sequence: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub timestamp: OffsetDateTime,
    pub stream: String,
    pub message: String,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LogQuery {
    pub after_sequence: Option<u64>,
    pub limit: Option<usize>,
    pub stream: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogsResponse {
    pub lines: Vec<LogEntry>,
    pub next_sequence: Option<u64>,
    pub has_more: bool,
    pub is_terminal: bool,
    pub logs: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogSegmentRecord {
    pub id: Uuid,
    pub deployment_id: Uuid,
    pub project_id: Uuid,
    pub start_sequence: u64,
    pub end_sequence: u64,
    pub storage_path: String,
    pub size_bytes: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Clone)]
pub struct Redactor {
    secret_fingerprints: Vec<String>,
    regex_bearer: Regex,
    regex_aws: Regex,
    regex_private_key: Regex,
    regex_assignment: Regex,
    regex_tokens: Regex,
}

impl Default for Redactor {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl Redactor {
    pub fn new(mut fingerprints: Vec<String>) -> Self {
        fingerprints.retain(|f| !f.trim().is_empty());
        fingerprints.sort_by_key(|a| std::cmp::Reverse(a.len()));

        Self {
            secret_fingerprints: fingerprints,
            regex_bearer: Regex::new(r"(?i)bearer\s+[a-zA-Z0-9_\-\.\~]+").unwrap(),
            regex_aws: Regex::new(r"AKIA[0-9A-Z]{16}").unwrap(),
            regex_private_key: Regex::new(r"(?s)-----BEGIN [A-Z ]+PRIVATE KEY-----.*?-----END [A-Z ]+PRIVATE KEY-----").unwrap(),
            regex_assignment: Regex::new(r"(?i)\b(password|passwd|secret|token|api_key|access_token|auth_token)\s*([=:])\s*([^\s&]+)").unwrap(),
            regex_tokens: Regex::new(r"\b(ghp_[A-Za-z0-9]{36}|glpat-[A-Za-z0-9_-]{20,}|xox[baprs]-[A-Za-z0-9-]+)\b").unwrap(),
        }
    }

    pub fn redact(&self, input: &str) -> String {
        let mut text = input.to_string();

        for secret in &self.secret_fingerprints {
            text = text.replace(secret, "[REDACTED]");
        }

        text = self
            .regex_private_key
            .replace_all(&text, "[REDACTED PRIVATE KEY]")
            .to_string();
        text = self.regex_aws.replace_all(&text, "[REDACTED]").to_string();
        text = self
            .regex_bearer
            .replace_all(&text, "Bearer [REDACTED]")
            .to_string();
        text = self
            .regex_tokens
            .replace_all(&text, "[REDACTED]")
            .to_string();
        text = self
            .regex_assignment
            .replace_all(&text, "$1$2[REDACTED]")
            .to_string();

        text
    }
}

pub struct DurableLogSink {
    deployment_id: Uuid,
    project_id: Uuid,
    pool: sqlx::PgPool,
    redactor: Redactor,
    sequence: Arc<AtomicU64>,
    segment_buffer: Vec<LogEntry>,
    segment_threshold: usize,
    storage_dir: Option<PathBuf>,
    handles: Vec<tokio::task::JoinHandle<()>>,
}

impl DurableLogSink {
    pub fn new(
        deployment_id: Uuid,
        project_id: Uuid,
        pool: sqlx::PgPool,
        redactor: Redactor,
        storage_dir: Option<PathBuf>,
        segment_threshold: usize,
    ) -> Self {
        Self {
            deployment_id,
            project_id,
            pool,
            redactor,
            sequence: Arc::new(AtomicU64::new(0)),
            segment_buffer: Vec::new(),
            segment_threshold: if segment_threshold == 0 {
                500
            } else {
                segment_threshold
            },
            storage_dir,
            handles: Vec::new(),
        }
    }

    pub fn current_sequence(&self) -> u64 {
        self.sequence.load(Ordering::SeqCst)
    }

    pub fn flush(&mut self) -> Result<()> {
        if self.segment_buffer.is_empty() {
            return Ok(());
        }
        self.rotate_segment()
    }

    pub async fn wait_all(&mut self) {
        let handles = std::mem::take(&mut self.handles);
        for h in handles {
            let _ = h.await;
        }
    }

    fn rotate_segment(&mut self) -> Result<()> {
        if self.segment_buffer.is_empty() {
            return Ok(());
        }

        let start_seq = self.segment_buffer.first().map(|l| l.sequence).unwrap_or(0);
        let end_seq = self.segment_buffer.last().map(|l| l.sequence).unwrap_or(0);

        if let Some(storage_dir) = &self.storage_dir {
            let dep_dir = storage_dir.join(self.deployment_id.to_string());
            fs::create_dir_all(&dep_dir)?;
            let file_name = format!("{start_seq:08}-{end_seq:08}.log");
            let file_path = dep_dir.join(&file_name);

            let mut contents = String::new();
            for entry in &self.segment_buffer {
                contents.push_str(&format!(
                    "[{}] [{}] {}\n",
                    entry
                        .timestamp
                        .format(&time::format_description::well_known::Rfc3339)
                        .unwrap_or_default(),
                    entry.stream,
                    entry.message
                ));
            }

            fs::write(&file_path, contents.as_bytes())?;
            let size_bytes = contents.len() as u64;

            let segment_id = Uuid::new_v4();
            let dep_id = self.deployment_id;
            let proj_id = self.project_id;
            let storage_path_str = file_path.to_string_lossy().to_string();
            let now = OffsetDateTime::now_utc();
            let pool = self.pool.clone();

            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                self.handles.push(handle.spawn(async move {
                    let _ = sqlx::query(
                        r#"
                        INSERT INTO log_segments (id, deployment_id, project_id, start_sequence, end_sequence, storage_path, size_bytes, created_at)
                        VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                        "#,
                    )
                    .bind(segment_id)
                    .bind(dep_id)
                    .bind(proj_id)
                    .bind(start_seq as i64)
                    .bind(end_seq as i64)
                    .bind(storage_path_str)
                    .bind(size_bytes as i64)
                    .bind(now)
                    .execute(&pool)
                    .await;
                }));
            }
        }

        self.segment_buffer.clear();
        Ok(())
    }
}

impl Drop for DurableLogSink {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

impl LogSink for DurableLogSink {
    fn write_line(&mut self, stream: LogStream, message: &str) -> Result<()> {
        let redacted = self.redactor.redact(message);
        let seq = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let now = OffsetDateTime::now_utc();
        let stream_str = match stream {
            LogStream::Stdout => "stdout",
            LogStream::Stderr => "stderr",
        };

        let entry = LogEntry {
            sequence: seq,
            timestamp: now,
            stream: stream_str.to_string(),
            message: redacted.clone(),
        };

        let id = Uuid::new_v4();
        let query = r#"
            INSERT INTO deployment_logs (id, deployment_id, project_id, sequence, stream, message, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
        "#;

        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            let pool = self.pool.clone();
            let dep_id = self.deployment_id;
            let proj_id = self.project_id;
            let st = stream_str.to_string();
            let msg = redacted.clone();
            self.handles.push(handle.spawn(async move {
                let _ = sqlx::query(query)
                    .bind(id)
                    .bind(dep_id)
                    .bind(proj_id)
                    .bind(seq as i64)
                    .bind(st)
                    .bind(msg)
                    .bind(now)
                    .execute(&pool)
                    .await;
            }));
        }

        self.segment_buffer.push(entry);

        if self.segment_buffer.len() >= self.segment_threshold {
            self.rotate_segment()?;
        }

        Ok(())
    }
}

pub struct LogRepository<'a> {
    executor: DbExecutor<'a>,
}

impl<'a> LogRepository<'a> {
    pub fn new(executor: DbExecutor<'a>) -> Self {
        Self { executor }
    }

    pub async fn append(
        &mut self,
        deployment_id: Uuid,
        project_id: Uuid,
        sequence: u64,
        stream: &str,
        message: &str,
        created_at: OffsetDateTime,
    ) -> Result<LogEntry> {
        let id = Uuid::new_v4();
        let query = r#"
            INSERT INTO deployment_logs (id, deployment_id, project_id, sequence, stream, message, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7)
            RETURNING sequence, created_at, stream, message
        "#;

        let row = self
            .executor
            .fetch_one(
                sqlx::query(query)
                    .bind(id)
                    .bind(deployment_id)
                    .bind(project_id)
                    .bind(sequence as i64)
                    .bind(stream)
                    .bind(message)
                    .bind(created_at),
            )
            .await
            .context("failed to append deployment log")?;

        let seq_i64: i64 = row.get("sequence");
        Ok(LogEntry {
            sequence: seq_i64 as u64,
            timestamp: row.get("created_at"),
            stream: row.get("stream"),
            message: row.get("message"),
        })
    }

    pub async fn query_logs(
        &mut self,
        deployment_id: Uuid,
        query: &LogQuery,
    ) -> Result<LogsResponse> {
        let after_seq = query.after_sequence.unwrap_or(0) as i64;
        let limit = query.limit.unwrap_or(100).min(1000) as i64;

        let sql = r#"
            SELECT sequence, stream, message, created_at
            FROM deployment_logs
            WHERE deployment_id = $1
              AND sequence > $2
              AND ($3::TEXT IS NULL OR stream = $3)
            ORDER BY sequence ASC
            LIMIT $4
        "#;

        // Fetch limit + 1 to determine has_more
        let fetch_limit = limit + 1;
        let rows = self
            .executor
            .fetch_all(
                sqlx::query(sql)
                    .bind(deployment_id)
                    .bind(after_seq)
                    .bind(query.stream.as_deref())
                    .bind(fetch_limit),
            )
            .await
            .context("failed to query deployment logs")?;

        let mut lines = Vec::new();
        for row in rows {
            let seq_i64: i64 = row.get("sequence");
            lines.push(LogEntry {
                sequence: seq_i64 as u64,
                timestamp: row.get("created_at"),
                stream: row.get("stream"),
                message: row.get("message"),
            });
        }

        let has_more = lines.len() > limit as usize;
        if has_more {
            lines.truncate(limit as usize);
        }

        let next_sequence = lines.last().map(|l| l.sequence);

        // Check if deployment is terminal
        let dep_status_opt: Option<String> = self
            .executor
            .fetch_optional(
                sqlx::query("SELECT status FROM deployments WHERE id = $1").bind(deployment_id),
            )
            .await
            .context("failed to check deployment status")?
            .map(|r| r.get("status"));

        let is_terminal = matches!(
            dep_status_opt.as_deref(),
            Some("completed") | Some("failed") | Some("cancelled") | Some("stopped")
        );

        let logs = lines
            .iter()
            .map(|l| {
                let ts_str = l
                    .timestamp
                    .format(&time::format_description::well_known::Rfc3339)
                    .unwrap_or_default();
                format!("[{ts_str}] [{}] {}\n", l.stream, l.message)
            })
            .collect::<String>();

        Ok(LogsResponse {
            lines,
            next_sequence,
            has_more,
            is_terminal,
            logs,
        })
    }

    pub async fn get_full_log(&mut self, deployment_id: Uuid) -> Result<String> {
        let sql = r#"
            SELECT sequence, stream, message, created_at
            FROM deployment_logs
            WHERE deployment_id = $1
            ORDER BY sequence ASC
        "#;

        let rows = self
            .executor
            .fetch_all(sqlx::query(sql).bind(deployment_id))
            .await
            .context("failed to fetch full deployment log")?;

        let mut out = String::new();
        for row in rows {
            let timestamp: OffsetDateTime = row.get("created_at");
            let stream: String = row.get("stream");
            let message: String = row.get("message");
            let ts_str = timestamp
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default();
            out.push_str(&format!("[{ts_str}] [{stream}] {message}\n"));
        }

        Ok(out)
    }

    pub async fn purge_logs_older_than(&mut self, threshold: OffsetDateTime) -> Result<u64> {
        let res = self
            .executor
            .execute(
                sqlx::query("DELETE FROM deployment_logs WHERE created_at < $1").bind(threshold),
            )
            .await
            .context("failed to purge old deployment logs")?;

        let _ = self
            .executor
            .execute(sqlx::query("DELETE FROM log_segments WHERE created_at < $1").bind(threshold))
            .await;

        Ok(res.rows_affected())
    }

    pub async fn record_segment(&mut self, segment: &LogSegmentRecord) -> Result<()> {
        let query = r#"
            INSERT INTO log_segments (id, deployment_id, project_id, start_sequence, end_sequence, storage_path, size_bytes, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
        "#;

        self.executor
            .execute(
                sqlx::query(query)
                    .bind(segment.id)
                    .bind(segment.deployment_id)
                    .bind(segment.project_id)
                    .bind(segment.start_sequence as i64)
                    .bind(segment.end_sequence as i64)
                    .bind(&segment.storage_path)
                    .bind(segment.size_bytes as i64)
                    .bind(segment.created_at),
            )
            .await
            .context("failed to record log segment")?;

        Ok(())
    }

    pub async fn list_segments(&mut self, deployment_id: Uuid) -> Result<Vec<LogSegmentRecord>> {
        let query = r#"
            SELECT id, deployment_id, project_id, start_sequence, end_sequence, storage_path, size_bytes, created_at
            FROM log_segments
            WHERE deployment_id = $1
            ORDER BY start_sequence ASC
        "#;

        let rows = self
            .executor
            .fetch_all(sqlx::query(query).bind(deployment_id))
            .await
            .context("failed to list log segments")?;

        let mut segments = Vec::new();
        for row in rows {
            let start_seq: i64 = row.get("start_sequence");
            let end_seq: i64 = row.get("end_sequence");
            let size_bytes: i64 = row.get("size_bytes");
            segments.push(LogSegmentRecord {
                id: row.get("id"),
                deployment_id: row.get("deployment_id"),
                project_id: row.get("project_id"),
                start_sequence: start_seq as u64,
                end_sequence: end_seq as u64,
                storage_path: row.get("storage_path"),
                size_bytes: size_bytes as u64,
                created_at: row.get("created_at"),
            });
        }

        Ok(segments)
    }
}
