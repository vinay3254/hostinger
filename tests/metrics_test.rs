use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use deploy_platform::{
    api::router_with_auth,
    auth::AuthService,
    builder::{BuildOutput, ImageBuilder},
    db::Database,
    detector::StaticSource,
    metrics::{
        aggregate_samples_into_series, MetricPoint, MetricSample, MetricsQuery, MetricsResponse,
        ALL_METRICS, METRIC_BUILD_DURATION_SECONDS, METRIC_CONTAINER_CPU_SECONDS,
        METRIC_REQUEST_LATENCY_MS, METRIC_REQUEST_TOTAL,
    },
    model::RunningContainer,
    repository::CreateProjectRecord,
    runtime::Runtime,
    service::{DeploymentService, PlatformService},
    store::StateStore,
    Result,
};
use http_body_util::BodyExt;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tempfile::tempdir;
use time::{Duration, OffsetDateTime};
use tower::ServiceExt;
use uuid::Uuid;

#[derive(Clone)]
struct FakeBuilder {
    image_path: PathBuf,
}

impl ImageBuilder for FakeBuilder {
    fn build(&self, _: &Path, _: Uuid, _: &StaticSource, _: &Path) -> Result<BuildOutput> {
        Ok(BuildOutput {
            rootfs_path: PathBuf::from("/tmp/fake-rootfs"),
            image_path: self.image_path.clone(),
        })
    }
}

#[derive(Default, Clone)]
struct FakeRuntime {
    stopped: Arc<Mutex<Vec<Uuid>>>,
}

impl Runtime for FakeRuntime {
    fn start_static(&mut self, _: &Path, _: &str, _: &[String]) -> Result<RunningContainer> {
        Ok(RunningContainer {
            id: Uuid::new_v4(),
            port: 43123,
            url: "http://127.0.0.1:43123".into(),
        })
    }

    fn stop(&mut self, id: Uuid) -> Result<()> {
        self.stopped.lock().unwrap().push(id);
        Ok(())
    }

    fn logs(&self, _: Uuid) -> Result<String> {
        Ok("fake runtime logs".into())
    }
}

fn test_service() -> (Arc<dyn PlatformService>, PathBuf, PathBuf) {
    let root = tempfile::TempDir::keep(tempdir().unwrap());
    let fake_img = root.join("fake-image.tar.gz");
    fs::write(&fake_img, b"fake").unwrap();

    let source = root.join("site");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("index.html"), "<h1>Hello Metrics</h1>").unwrap();

    let builder = FakeBuilder {
        image_path: fake_img.clone(),
    };
    let runtime = FakeRuntime::default();
    (
        Arc::new(DeploymentService::new(
            StateStore::at(root),
            runtime,
            builder,
        )),
        source,
        fake_img,
    )
}

async fn setup_test_db() -> Database {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/deploy_platform".into());
    let db = Database::connect(&url)
        .await
        .expect("Failed to connect to database");
    db.migrate().await.expect("Failed to run migrations");
    db
}

async fn create_test_project(db: &Database, user_id: Uuid, name: &str) -> Uuid {
    let mut repo = db.projects();
    let project = repo
        .create_project(CreateProjectRecord {
            id: Uuid::new_v4(),
            user_id,
            name: name.to_string(),
            source_dir: PathBuf::from("/tmp"),
            base_image: PathBuf::from("/tmp"),
            server_command: vec!["echo".into()],
            created_at: OffsetDateTime::now_utc(),
        })
        .await
        .unwrap();
    project.id
}

#[tokio::test]
async fn test_counter_and_rate_aggregation() {
    let now = OffsetDateTime::now_utc();
    let start_epoch = (now.unix_timestamp() / 60) * 60 - 1800;
    let start = OffsetDateTime::from_unix_timestamp(start_epoch).unwrap();
    let end = now;
    let project_id = Uuid::new_v4();

    // Create counter samples for request_total across several minutes
    let samples = vec![
        MetricSample {
            project_id,
            deployment_id: None,
            environment: "production".into(),
            metric_name: METRIC_REQUEST_TOTAL.into(),
            value: 10.0,
            unit: "count".into(),
            recorded_at: start + Duration::minutes(5),
        },
        MetricSample {
            project_id,
            deployment_id: None,
            environment: "production".into(),
            metric_name: METRIC_REQUEST_TOTAL.into(),
            value: 15.0,
            unit: "count".into(),
            recorded_at: start + Duration::minutes(5) + Duration::seconds(20),
        },
        MetricSample {
            project_id,
            deployment_id: None,
            environment: "production".into(),
            metric_name: METRIC_REQUEST_TOTAL.into(),
            value: 5.0,
            unit: "count".into(),
            recorded_at: start + Duration::minutes(15),
        },
    ];

    let series = aggregate_samples_into_series(
        METRIC_REQUEST_TOTAL,
        Some("production"),
        &samples,
        start,
        end,
        60, // 1m resolution
        now,
    );

    assert_eq!(series.metric_name, METRIC_REQUEST_TOTAL);
    assert_eq!(series.unit, "count");
    assert!(series.has_data);
    assert_eq!(series.summary.count, 3);
    assert_eq!(series.summary.total, 30.0);

    // Verify bucket at minute 5 contains sum of 25.0
    let bucket_5 = series
        .points
        .iter()
        .find(|p| {
            let s_epoch = (start + Duration::minutes(5)).unix_timestamp();
            let aligned = (s_epoch / 60) * 60;
            p.timestamp.unix_timestamp() == aligned
        })
        .expect("minute 5 bucket not found");

    assert_eq!(bucket_5.count, 2);
    assert_eq!(bucket_5.value, 25.0);
    assert_eq!(bucket_5.sum, 25.0);
}

#[tokio::test]
async fn test_latency_aggregation_percentiles() {
    let now = OffsetDateTime::now_utc();
    let start = now - Duration::hours(1);
    let end = now;
    let project_id = Uuid::new_v4();

    // 100 latency samples from 1.0 to 100.0 ms in a single 5m bucket
    let bucket_time = start + Duration::minutes(10);
    let mut samples = Vec::new();
    for i in 1..=100 {
        samples.push(MetricSample {
            project_id,
            deployment_id: None,
            environment: "production".into(),
            metric_name: METRIC_REQUEST_LATENCY_MS.into(),
            value: i as f64,
            unit: "milliseconds".into(),
            recorded_at: bucket_time + Duration::seconds(i),
        });
    }

    let series = aggregate_samples_into_series(
        METRIC_REQUEST_LATENCY_MS,
        Some("production"),
        &samples,
        start,
        end,
        300, // 5m resolution
        now,
    );

    assert_eq!(series.metric_name, METRIC_REQUEST_LATENCY_MS);
    assert_eq!(series.unit, "milliseconds");
    assert!(series.has_data);
    assert_eq!(series.summary.count, 100);
    assert_eq!(series.summary.min, 1.0);
    assert_eq!(series.summary.max, 100.0);
    assert!((series.summary.avg - 50.5).abs() < 0.001);

    // Percentiles
    let p50 = series.summary.p50.unwrap();
    let p95 = series.summary.p95.unwrap();
    let p99 = series.summary.p99.unwrap();

    assert!((p50 - 50.5).abs() <= 1.0, "p50 expected ~50.5, got {p50}");
    assert!((p95 - 95.0).abs() <= 1.0, "p95 expected ~95.0, got {p95}");
    assert!((p99 - 99.0).abs() <= 1.0, "p99 expected ~99.0, got {p99}");
}

#[tokio::test]
async fn test_out_of_order_samples() {
    let now = OffsetDateTime::now_utc();
    let start = now - Duration::hours(1);
    let end = now;
    let project_id = Uuid::new_v4();

    // Ingest samples out of chronological order
    let samples = vec![
        MetricSample {
            project_id,
            deployment_id: None,
            environment: "production".into(),
            metric_name: METRIC_CONTAINER_CPU_SECONDS.into(),
            value: 30.0,
            unit: "seconds".into(),
            recorded_at: start + Duration::minutes(40),
        },
        MetricSample {
            project_id,
            deployment_id: None,
            environment: "production".into(),
            metric_name: METRIC_CONTAINER_CPU_SECONDS.into(),
            value: 10.0,
            unit: "seconds".into(),
            recorded_at: start + Duration::minutes(10),
        },
        MetricSample {
            project_id,
            deployment_id: None,
            environment: "production".into(),
            metric_name: METRIC_CONTAINER_CPU_SECONDS.into(),
            value: 20.0,
            unit: "seconds".into(),
            recorded_at: start + Duration::minutes(25),
        },
    ];

    let series = aggregate_samples_into_series(
        METRIC_CONTAINER_CPU_SECONDS,
        Some("production"),
        &samples,
        start,
        end,
        300, // 5m resolution
        now,
    );

    // Points should be strictly ordered chronologically
    for i in 1..series.points.len() {
        assert!(series.points[i].timestamp > series.points[i - 1].timestamp);
    }

    let non_empty: Vec<&MetricPoint> = series.points.iter().filter(|p| p.count > 0).collect();
    assert_eq!(non_empty.len(), 3);
    assert_eq!(non_empty[0].value, 10.0);
    assert_eq!(non_empty[1].value, 20.0);
    assert_eq!(non_empty[2].value, 30.0);
}

#[tokio::test]
async fn test_timezone_boundaries_and_alignment() {
    let now = OffsetDateTime::now_utc();
    // Test that odd timestamps are rounded down to UTC bucket boundaries
    let custom_time = OffsetDateTime::from_unix_timestamp(1700000123).unwrap();
    let start = custom_time;
    let end = custom_time + Duration::minutes(10);
    let project_id = Uuid::new_v4();

    let samples = vec![MetricSample {
        project_id,
        deployment_id: None,
        environment: "production".into(),
        metric_name: METRIC_REQUEST_TOTAL.into(),
        value: 1.0,
        unit: "count".into(),
        recorded_at: custom_time + Duration::seconds(45), // 1700000168
    }];

    let series = aggregate_samples_into_series(
        METRIC_REQUEST_TOTAL,
        Some("production"),
        &samples,
        start,
        end,
        60, // 1m resolution (60s)
        now,
    );

    let first_point = &series.points[0];
    assert_eq!(
        first_point.timestamp.unix_timestamp() % 60,
        0,
        "Timestamp should be aligned to 60s boundary"
    );
    assert_eq!(
        first_point.timestamp.unix_timestamp(),
        (1700000123 / 60) * 60
    );
}

#[tokio::test]
async fn test_no_data_windows_and_partial_data_flag() {
    let now = OffsetDateTime::now_utc();
    let start = now - Duration::minutes(10);
    let end = now;

    // No samples at all for build_duration_seconds
    let series = aggregate_samples_into_series(
        METRIC_BUILD_DURATION_SECONDS,
        Some("production"),
        &[],
        start,
        end,
        60,
        now,
    );

    assert_eq!(series.metric_name, METRIC_BUILD_DURATION_SECONDS);
    assert!(!series.has_data);
    assert_eq!(series.summary.count, 0);
    assert_eq!(series.summary.total, 0.0);
    assert!(series.points.len() >= 10);
    // Every point has count 0
    for p in &series.points {
        assert_eq!(p.count, 0);
        assert_eq!(p.value, 0.0);
    }

    // The point covering current time `now` should be flagged is_partial: true
    let last_point = series.points.last().unwrap();
    assert!(last_point.is_partial);
}

#[tokio::test]
async fn test_metrics_http_endpoint_filtering_and_permissions() {
    let db = setup_test_db().await;
    let auth = AuthService::new(db.clone());
    let (service, _source, _base_img) = test_service();
    let app = router_with_auth(service, Some(auth.clone()), Some(db.clone()));

    // Create User A and User B
    let user_a = auth
        .register(
            &format!("user-a-{}@example.com", Uuid::new_v4().simple()),
            "Password123!",
            "User A",
        )
        .await
        .unwrap();
    let session_a = auth
        .create_session(user_a.id, Duration::days(1))
        .await
        .unwrap();

    let user_b = auth
        .register(
            &format!("user-b-{}@example.com", Uuid::new_v4().simple()),
            "Password123!",
            "User B",
        )
        .await
        .unwrap();
    let session_b = auth
        .create_session(user_b.id, Duration::days(1))
        .await
        .unwrap();

    // Create Project A owned by User A
    let project_id =
        create_test_project(&db, user_a.id, &format!("proj-{}", Uuid::new_v4().simple())).await;

    // Ingest samples for User A's project
    let now = OffsetDateTime::now_utc();
    let mut metrics_repo = db.metrics();
    metrics_repo
        .ingest_samples(&[
            MetricSample {
                project_id,
                deployment_id: None,
                environment: "production".into(),
                metric_name: METRIC_REQUEST_TOTAL.into(),
                value: 42.0,
                unit: "count".into(),
                recorded_at: now - Duration::minutes(5),
            },
            MetricSample {
                project_id,
                deployment_id: None,
                environment: "preview".into(),
                metric_name: METRIC_REQUEST_TOTAL.into(),
                value: 7.0,
                unit: "count".into(),
                recorded_at: now - Duration::minutes(5),
            },
            MetricSample {
                project_id,
                deployment_id: None,
                environment: "production".into(),
                metric_name: METRIC_REQUEST_LATENCY_MS.into(),
                value: 25.5,
                unit: "milliseconds".into(),
                recorded_at: now - Duration::minutes(2),
            },
        ])
        .await
        .unwrap();

    // 1. Unauthenticated request -> 401 Unauthorized
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/projects/{project_id}/metrics"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // 2. User B tries to query User A's project metrics -> 403 Forbidden
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/projects/{project_id}/metrics"))
                .header(header::AUTHORIZATION, format!("Bearer {}", session_b.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // 3. User A queries metrics with environment and metric filters -> 200 OK
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/v1/projects/{project_id}/metrics?metric={METRIC_REQUEST_TOTAL}&range=1h&resolution=1m&environment=production"
                ))
                .header(header::AUTHORIZATION, format!("Bearer {}", session_a.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let body: MetricsResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body.project_id, project_id);
    assert_eq!(body.range, "1h");
    assert_eq!(body.resolution, "1m");
    assert_eq!(body.series.len(), 1);
    let series = &body.series[0];
    assert_eq!(series.metric_name, METRIC_REQUEST_TOTAL);
    assert_eq!(series.unit, "count");
    assert_eq!(series.environment.as_deref(), Some("production"));
    assert!(series.has_data);
    assert_eq!(series.summary.total, 42.0); // Only production sample (42.0), not preview (7.0)

    // 4. User A queries without metric filter -> returns all 8 metrics
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/projects/{project_id}/metrics?range=15m"))
                .header(header::AUTHORIZATION, format!("Bearer {}", session_a.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let body: MetricsResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body.series.len(), ALL_METRICS.len());
}

#[tokio::test]
async fn test_metric_retention_purge() {
    let db = setup_test_db().await;
    let auth = AuthService::new(db.clone());
    let user = auth
        .register(
            &format!("user-purge-{}@example.com", Uuid::new_v4().simple()),
            "Password123!",
            "User Purge",
        )
        .await
        .unwrap();

    let project_id = create_test_project(
        &db,
        user.id,
        &format!("proj-purge-{}", Uuid::new_v4().simple()),
    )
    .await;

    let now = OffsetDateTime::now_utc();
    let mut metrics_repo = db.metrics();

    // 1 old sample (30 days ago) and 1 recent sample (10 minutes ago)
    metrics_repo
        .ingest_samples(&[
            MetricSample {
                project_id,
                deployment_id: None,
                environment: "production".into(),
                metric_name: METRIC_REQUEST_TOTAL.into(),
                value: 100.0,
                unit: "count".into(),
                recorded_at: now - Duration::days(30),
            },
            MetricSample {
                project_id,
                deployment_id: None,
                environment: "production".into(),
                metric_name: METRIC_REQUEST_TOTAL.into(),
                value: 50.0,
                unit: "count".into(),
                recorded_at: now - Duration::minutes(10),
            },
        ])
        .await
        .unwrap();

    // Purge older than 7 days
    let deleted = metrics_repo
        .purge_samples_older_than(Duration::days(7))
        .await
        .unwrap();
    assert!(deleted >= 1);

    // Query 60 days of metrics
    let resp = metrics_repo
        .query_metrics(
            project_id,
            &MetricsQuery {
                metric: Some(METRIC_REQUEST_TOTAL.into()),
                range: Some("7d".into()),
                resolution: Some("1h".into()),
                environment: None,
                start: Some(now - Duration::days(60)),
                end: Some(now),
            },
        )
        .await
        .unwrap();

    assert_eq!(resp.series[0].summary.count, 1);
    assert_eq!(resp.series[0].summary.total, 50.0);
}
