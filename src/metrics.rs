use crate::repository::DbExecutor;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sqlx::Row;
use time::OffsetDateTime;
use uuid::Uuid;

pub const METRIC_BUILD_DURATION_SECONDS: &str = "build_duration_seconds";
pub const METRIC_BUILD_CACHE_HIT_TOTAL: &str = "build_cache_hit_total";
pub const METRIC_DEPLOYMENT_HEALTH_CHECK_TOTAL: &str = "deployment_health_check_total";
pub const METRIC_REQUEST_TOTAL: &str = "request_total";
pub const METRIC_REQUEST_ERROR_TOTAL: &str = "request_error_total";
pub const METRIC_REQUEST_LATENCY_MS: &str = "request_latency_ms";
pub const METRIC_CONTAINER_CPU_SECONDS: &str = "container_cpu_seconds";
pub const METRIC_CONTAINER_MEMORY_BYTES: &str = "container_memory_bytes";

pub const ALL_METRICS: &[&str] = &[
    METRIC_BUILD_DURATION_SECONDS,
    METRIC_BUILD_CACHE_HIT_TOTAL,
    METRIC_DEPLOYMENT_HEALTH_CHECK_TOTAL,
    METRIC_REQUEST_TOTAL,
    METRIC_REQUEST_ERROR_TOTAL,
    METRIC_REQUEST_LATENCY_MS,
    METRIC_CONTAINER_CPU_SECONDS,
    METRIC_CONTAINER_MEMORY_BYTES,
];

pub fn metric_unit(name: &str) -> &'static str {
    match name {
        METRIC_BUILD_DURATION_SECONDS => "seconds",
        METRIC_BUILD_CACHE_HIT_TOTAL => "count",
        METRIC_DEPLOYMENT_HEALTH_CHECK_TOTAL => "count",
        METRIC_REQUEST_TOTAL => "count",
        METRIC_REQUEST_ERROR_TOTAL => "count",
        METRIC_REQUEST_LATENCY_MS => "milliseconds",
        METRIC_CONTAINER_CPU_SECONDS => "seconds",
        METRIC_CONTAINER_MEMORY_BYTES => "bytes",
        _ => "value",
    }
}

pub fn parse_resolution_secs(res: &str) -> i64 {
    match res {
        "1m" => 60,
        "5m" => 300,
        "15m" => 900,
        "1h" => 3600,
        "1d" => 86400,
        _ => 60,
    }
}

pub fn parse_range_duration(range: &str) -> time::Duration {
    match range {
        "15m" => time::Duration::minutes(15),
        "1h" => time::Duration::hours(1),
        "6h" => time::Duration::hours(6),
        "24h" => time::Duration::hours(24),
        "7d" => time::Duration::days(7),
        _ => time::Duration::hours(1),
    }
}

pub fn default_resolution_for_range(range: &str) -> &'static str {
    match range {
        "15m" => "1m",
        "1h" => "1m",
        "6h" => "5m",
        "24h" => "5m",
        "7d" => "1h",
        _ => "1m",
    }
}

pub fn calculate_percentile(sorted_values: &[f64], pct: f64) -> f64 {
    if sorted_values.is_empty() {
        return 0.0;
    }
    if sorted_values.len() == 1 {
        return sorted_values[0];
    }
    let rank = (pct / 100.0) * (sorted_values.len() - 1) as f64;
    let lower_idx = rank.floor() as usize;
    let upper_idx = rank.ceil() as usize;
    if lower_idx == upper_idx {
        sorted_values[lower_idx]
    } else {
        let weight = rank - lower_idx as f64;
        sorted_values[lower_idx] * (1.0 - weight) + sorted_values[upper_idx] * weight
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSample {
    pub project_id: Uuid,
    pub deployment_id: Option<Uuid>,
    pub environment: String,
    pub metric_name: String,
    pub value: f64,
    pub unit: String,
    pub recorded_at: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricPoint {
    #[serde(with = "time::serde::rfc3339")]
    pub timestamp: OffsetDateTime,
    pub count: u64,
    pub value: f64,
    pub sum: f64,
    pub min: f64,
    pub max: f64,
    pub avg: f64,
    pub p50: Option<f64>,
    pub p95: Option<f64>,
    pub p99: Option<f64>,
    pub is_partial: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MetricSummary {
    pub total: f64,
    pub count: u64,
    pub avg: f64,
    pub min: f64,
    pub max: f64,
    pub p50: Option<f64>,
    pub p95: Option<f64>,
    pub p99: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricSeries {
    pub metric_name: String,
    pub unit: String,
    pub environment: Option<String>,
    pub points: Vec<MetricPoint>,
    pub summary: MetricSummary,
    pub has_data: bool,
    pub is_partial: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MetricsQuery {
    pub metric: Option<String>,
    pub range: Option<String>,
    pub resolution: Option<String>,
    pub environment: Option<String>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub start: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub end: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsResponse {
    pub project_id: Uuid,
    pub range: String,
    pub resolution: String,
    #[serde(with = "time::serde::rfc3339")]
    pub start: OffsetDateTime,
    #[serde(with = "time::serde::rfc3339")]
    pub end: OffsetDateTime,
    pub series: Vec<MetricSeries>,
    #[serde(with = "time::serde::rfc3339")]
    pub last_updated: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricRollupRecord {
    pub id: Uuid,
    pub project_id: Uuid,
    pub environment: String,
    pub metric_name: String,
    pub resolution: String,
    #[serde(with = "time::serde::rfc3339")]
    pub bucket_start: OffsetDateTime,
    pub sample_count: i64,
    pub sample_sum: f64,
    pub sample_min: f64,
    pub sample_max: f64,
    pub p50: Option<f64>,
    pub p95: Option<f64>,
    pub p99: Option<f64>,
    pub is_partial: bool,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

pub fn aggregate_samples_into_series(
    metric_name: &str,
    environment: Option<&str>,
    samples: &[MetricSample],
    start: OffsetDateTime,
    end: OffsetDateTime,
    resolution_secs: i64,
    now: OffsetDateTime,
) -> MetricSeries {
    let unit = metric_unit(metric_name).to_string();
    let is_counter = matches!(
        metric_name,
        METRIC_BUILD_CACHE_HIT_TOTAL
            | METRIC_DEPLOYMENT_HEALTH_CHECK_TOTAL
            | METRIC_REQUEST_TOTAL
            | METRIC_REQUEST_ERROR_TOTAL
    );

    // Filter samples for this metric and environment within [start, end]
    let mut matching_samples: Vec<&MetricSample> = samples
        .iter()
        .filter(|s| {
            s.metric_name == metric_name
                && environment.map_or(true, |env| s.environment == env)
                && s.recorded_at >= start
                && s.recorded_at <= end
        })
        .collect();

    // Sort matching samples by recorded_at (handling out-of-order)
    matching_samples.sort_by_key(|s| s.recorded_at);

    // Align start and end to resolution buckets
    let start_epoch = (start.unix_timestamp() / resolution_secs) * resolution_secs;
    let end_epoch = (end.unix_timestamp() / resolution_secs) * resolution_secs;

    let mut points = Vec::new();
    let mut current_epoch = start_epoch;

    let mut sample_idx = 0;
    let mut all_values: Vec<f64> = Vec::new();

    while current_epoch <= end_epoch {
        let bucket_start = OffsetDateTime::from_unix_timestamp(current_epoch).unwrap_or(start);
        let bucket_end_epoch = current_epoch + resolution_secs;
        let is_partial = now.unix_timestamp() < bucket_end_epoch;

        // Collect samples falling in [current_epoch, bucket_end_epoch)
        let mut bucket_values = Vec::new();
        while sample_idx < matching_samples.len() {
            let s_epoch = matching_samples[sample_idx].recorded_at.unix_timestamp();
            if s_epoch < current_epoch {
                sample_idx += 1;
                continue;
            }
            if s_epoch >= bucket_end_epoch {
                break;
            }
            bucket_values.push(matching_samples[sample_idx].value);
            all_values.push(matching_samples[sample_idx].value);
            sample_idx += 1;
        }

        bucket_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let count = bucket_values.len() as u64;
        let sum: f64 = bucket_values.iter().sum();
        let (min, max, avg, p50, p95, p99, val) = if count > 0 {
            let min_v = bucket_values[0];
            let max_v = bucket_values[bucket_values.len() - 1];
            let avg_v = sum / count as f64;
            let p50_v = calculate_percentile(&bucket_values, 50.0);
            let p95_v = calculate_percentile(&bucket_values, 95.0);
            let p99_v = calculate_percentile(&bucket_values, 99.0);
            let point_val = if is_counter { sum } else { avg_v };
            (
                min_v,
                max_v,
                avg_v,
                Some(p50_v),
                Some(p95_v),
                Some(p99_v),
                point_val,
            )
        } else {
            (0.0, 0.0, 0.0, None, None, None, 0.0)
        };

        points.push(MetricPoint {
            timestamp: bucket_start,
            count,
            value: val,
            sum,
            min,
            max,
            avg,
            p50,
            p95,
            p99,
            is_partial,
        });

        current_epoch += resolution_secs;
    }

    all_values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let total_count = all_values.len() as u64;
    let total_sum: f64 = all_values.iter().sum();
    let summary = if total_count > 0 {
        MetricSummary {
            total: if is_counter {
                total_sum
            } else {
                total_sum / total_count as f64
            },
            count: total_count,
            avg: total_sum / total_count as f64,
            min: all_values[0],
            max: all_values[all_values.len() - 1],
            p50: Some(calculate_percentile(&all_values, 50.0)),
            p95: Some(calculate_percentile(&all_values, 95.0)),
            p99: Some(calculate_percentile(&all_values, 99.0)),
        }
    } else {
        MetricSummary {
            total: 0.0,
            count: 0,
            avg: 0.0,
            min: 0.0,
            max: 0.0,
            p50: None,
            p95: None,
            p99: None,
        }
    };

    let has_data = total_count > 0;
    let any_partial = points.iter().any(|p| p.is_partial);

    MetricSeries {
        metric_name: metric_name.to_string(),
        unit,
        environment: environment.map(|e| e.to_string()),
        points,
        summary,
        has_data,
        is_partial: any_partial,
    }
}

pub struct MetricRepository<'a> {
    executor: DbExecutor<'a>,
}

impl<'a> MetricRepository<'a> {
    pub fn new(executor: DbExecutor<'a>) -> Self {
        Self { executor }
    }

    pub async fn ingest_sample(&mut self, sample: &MetricSample) -> Result<()> {
        let id = Uuid::new_v4();
        let now = OffsetDateTime::now_utc();
        self.executor
            .execute(
                sqlx::query(
                    "INSERT INTO metric_samples
                     (id, project_id, deployment_id, environment, metric_name, value, unit, recorded_at, created_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
                )
                .bind(id)
                .bind(sample.project_id)
                .bind(sample.deployment_id)
                .bind(&sample.environment)
                .bind(&sample.metric_name)
                .bind(sample.value)
                .bind(&sample.unit)
                .bind(sample.recorded_at)
                .bind(now),
            )
            .await
            .context("failed to insert metric sample")?;
        Ok(())
    }

    pub async fn ingest_samples(&mut self, samples: &[MetricSample]) -> Result<usize> {
        let mut count = 0;
        for s in samples {
            self.ingest_sample(s).await?;
            count += 1;
        }
        Ok(count)
    }

    pub async fn query_metrics(
        &mut self,
        project_id: Uuid,
        query: &MetricsQuery,
    ) -> Result<MetricsResponse> {
        let now = OffsetDateTime::now_utc();
        let range_str = query.range.as_deref().unwrap_or("1h");
        let (start, end) = match (query.start, query.end) {
            (Some(s), Some(e)) => (s, e),
            (Some(s), None) => (s, s + parse_range_duration(range_str)),
            _ => {
                let dur = parse_range_duration(range_str);
                (now - dur, now)
            }
        };

        let res_str = query
            .resolution
            .as_deref()
            .unwrap_or_else(|| default_resolution_for_range(range_str));
        let resolution_secs = parse_resolution_secs(res_str);

        // Fetch raw samples from metric_samples within [start, end]
        let rows = self
            .executor
            .fetch_all(
                sqlx::query(
                    "SELECT project_id, deployment_id, environment, metric_name, value, unit, recorded_at
                     FROM metric_samples
                     WHERE project_id = $1
                       AND recorded_at >= $2
                       AND recorded_at <= $3
                     ORDER BY recorded_at ASC",
                )
                .bind(project_id)
                .bind(start)
                .bind(end),
            )
            .await
            .context("failed to query metric samples")?;

        let samples: Vec<MetricSample> = rows
            .into_iter()
            .map(|r| MetricSample {
                project_id: r.get("project_id"),
                deployment_id: r.get("deployment_id"),
                environment: r.get("environment"),
                metric_name: r.get("metric_name"),
                value: r.get("value"),
                unit: r.get("unit"),
                recorded_at: r.get("recorded_at"),
            })
            .collect();

        let metric_names: Vec<&str> = if let Some(ref m) = query.metric {
            vec![m.as_str()]
        } else {
            ALL_METRICS.to_vec()
        };

        let env_filter = query.environment.as_deref();

        let series = metric_names
            .into_iter()
            .map(|name| {
                aggregate_samples_into_series(
                    name,
                    env_filter,
                    &samples,
                    start,
                    end,
                    resolution_secs,
                    now,
                )
            })
            .collect();

        Ok(MetricsResponse {
            project_id,
            range: range_str.to_string(),
            resolution: res_str.to_string(),
            start,
            end,
            series,
            last_updated: now,
        })
    }

    pub async fn record_rollup(&mut self, rollup: &MetricRollupRecord) -> Result<()> {
        let now = OffsetDateTime::now_utc();
        self.executor
            .execute(
                sqlx::query(
                    "INSERT INTO metric_rollups
                     (id, project_id, environment, metric_name, resolution, bucket_start,
                      sample_count, sample_sum, sample_min, sample_max, p50, p95, p99, is_partial, created_at)
                     VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15)
                     ON CONFLICT (project_id, environment, metric_name, resolution, bucket_start)
                     DO UPDATE SET
                      sample_count = EXCLUDED.sample_count,
                      sample_sum = EXCLUDED.sample_sum,
                      sample_min = EXCLUDED.sample_min,
                      sample_max = EXCLUDED.sample_max,
                      p50 = EXCLUDED.p50,
                      p95 = EXCLUDED.p95,
                      p99 = EXCLUDED.p99,
                      is_partial = EXCLUDED.is_partial,
                      created_at = EXCLUDED.created_at",
                )
                .bind(rollup.id)
                .bind(rollup.project_id)
                .bind(&rollup.environment)
                .bind(&rollup.metric_name)
                .bind(&rollup.resolution)
                .bind(rollup.bucket_start)
                .bind(rollup.sample_count)
                .bind(rollup.sample_sum)
                .bind(rollup.sample_min)
                .bind(rollup.sample_max)
                .bind(rollup.p50)
                .bind(rollup.p95)
                .bind(rollup.p99)
                .bind(rollup.is_partial)
                .bind(now),
            )
            .await
            .context("failed to record metric rollup")?;
        Ok(())
    }

    pub async fn purge_samples_older_than(&mut self, age: time::Duration) -> Result<u64> {
        let threshold = OffsetDateTime::now_utc() - age;
        let res = self
            .executor
            .execute(
                sqlx::query("DELETE FROM metric_samples WHERE recorded_at < $1").bind(threshold),
            )
            .await
            .context("failed to purge old metric samples")?;
        Ok(res.rows_affected())
    }
}
