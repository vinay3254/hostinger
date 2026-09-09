use deploy_platform::{
    artifacts::ArtifactStore,
    build_executor::{BuildExecutor, BuildResult, LogSink, MinidockBuildExecutor},
    db::Database,
    framework::BuildPlan,
    queue::{BuildPriority, BuildQueue},
    worker::{BuildWorker, BuildWorkerOutcome},
};
use sqlx::Row;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tempfile::tempdir;
use uuid::Uuid;

async fn get_test_db_and_queue(prefix: &str) -> (Database, BuildQueue) {
    let database_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/deploy_platform".into());
    let db = Database::connect(&database_url)
        .await
        .expect("failed to connect to test postgres");
    let _ = db.migrate().await;

    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".into());
    let redis = redis::Client::open(redis_url).expect("failed to connect to test redis");

    let queue = BuildQueue::with_prefix(
        db.clone(),
        redis,
        "test_workers".to_string(),
        prefix.to_string(),
    )
    .await
    .unwrap();

    (db, queue)
}

async fn create_test_project_and_deployment(db: &Database, source_dir: &Path) -> (Uuid, Uuid) {
    let user_id = Uuid::new_v4();
    let email = format!("user-{}@example.com", Uuid::new_v4());
    sqlx::query("INSERT INTO users (id, email, password_hash, name, created_at) VALUES ($1, $2, 'hash', 'Test', NOW())")
        .bind(user_id)
        .bind(&email)
        .execute(db.pool())
        .await
        .unwrap();

    let project_id = Uuid::new_v4();
    let project_name = format!("proj-{}", Uuid::new_v4());
    sqlx::query(
        "INSERT INTO projects (id, user_id, name, source_dir, base_image, server_command, created_at)
         VALUES ($1, $2, $3, $4, '/tmp/base.tar.gz', ARRAY['/bin/server', '{PORT}'], NOW())",
    )
    .bind(project_id)
    .bind(user_id)
    .bind(&project_name)
    .bind(source_dir.to_string_lossy().to_string())
    .execute(db.pool())
    .await
    .unwrap();

    let deployment_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO deployments (id, project_id, framework, status, created_at)
         VALUES ($1, $2, 'static', 'queued', NOW())",
    )
    .bind(deployment_id)
    .bind(project_id)
    .execute(db.pool())
    .await
    .unwrap();

    (project_id, deployment_id)
}

#[tokio::test]
async fn test_worker_success_lifecycle() {
    let prefix = format!("worker_test_succ_{}", Uuid::new_v4().simple());
    let (db, queue) = get_test_db_and_queue(&prefix).await;

    let src_dir = tempdir().unwrap();
    fs::write(src_dir.path().join("index.html"), "<h1>Worker Test</h1>").unwrap();

    let (project_id, deployment_id) = create_test_project_and_deployment(&db, src_dir.path()).await;

    queue
        .enqueue_job(deployment_id, project_id, BuildPriority::Production)
        .await
        .unwrap();

    let artifacts_dir = tempdir().unwrap();
    let artifact_store = ArtifactStore::new(artifacts_dir.path());
    let workspace_dir = tempdir().unwrap();
    let executor = Arc::new(MinidockBuildExecutor::new(artifacts_dir.path()));

    let worker = BuildWorker::new(
        "worker-1",
        db.pool().clone(),
        queue,
        executor,
        artifact_store,
        workspace_dir.path().to_path_buf(),
    );

    let outcome = worker.run_once().await.unwrap();
    assert!(matches!(outcome, BuildWorkerOutcome::Completed(_)));

    let dep_row = sqlx::query("SELECT status, image_path FROM deployments WHERE id = $1")
        .bind(deployment_id)
        .fetch_one(db.pool())
        .await
        .unwrap();
    let status: String = dep_row.get("status");
    assert_eq!(status, "running");

    let attempt_row = sqlx::query("SELECT status FROM build_attempts WHERE job_id = (SELECT id FROM build_jobs WHERE deployment_id = $1)")
        .bind(deployment_id)
        .fetch_one(db.pool())
        .await
        .unwrap();
    let attempt_status: String = attempt_row.get("status");
    assert_eq!(attempt_status, "succeeded");

    let count_events: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM build_events WHERE deployment_id = $1")
            .bind(deployment_id)
            .fetch_one(db.pool())
            .await
            .unwrap();
    assert!(count_events >= 2);
}

#[tokio::test]
async fn test_worker_user_build_failure_is_terminal() {
    let prefix = format!("worker_test_fail_{}", Uuid::new_v4().simple());
    let (db, queue) = get_test_db_and_queue(&prefix).await;

    let src_dir = tempdir().unwrap();
    fs::write(src_dir.path().join("package.json"), "NOT VALID JSON").unwrap();

    let (project_id, deployment_id) = create_test_project_and_deployment(&db, src_dir.path()).await;

    queue
        .enqueue_job(deployment_id, project_id, BuildPriority::Preview)
        .await
        .unwrap();

    let artifacts_dir = tempdir().unwrap();
    let artifact_store = ArtifactStore::new(artifacts_dir.path());
    let workspace_dir = tempdir().unwrap();
    let executor = Arc::new(MinidockBuildExecutor::new(artifacts_dir.path()));

    let worker = BuildWorker::new(
        "worker-fail",
        db.pool().clone(),
        queue,
        executor,
        artifact_store,
        workspace_dir.path().to_path_buf(),
    );

    let outcome = worker.run_once().await.unwrap();
    assert!(matches!(outcome, BuildWorkerOutcome::Failed(_)));

    let dep_row = sqlx::query("SELECT status, error FROM deployments WHERE id = $1")
        .bind(deployment_id)
        .fetch_one(db.pool())
        .await
        .unwrap();
    let status: String = dep_row.get("status");
    let error: Option<String> = dep_row.get("error");
    assert_eq!(status, "failed");
    assert!(error.unwrap().contains("invalid package.json"));

    let job_row = sqlx::query("SELECT status FROM build_jobs WHERE deployment_id = $1")
        .bind(deployment_id)
        .fetch_one(db.pool())
        .await
        .unwrap();
    let job_status: String = job_row.get("status");
    assert_eq!(job_status, "failed");
}

struct FlakyExecutor {
    fail_count: Arc<AtomicU32>,
    artifacts_dir: PathBuf,
}

impl BuildExecutor for FlakyExecutor {
    fn execute(
        &self,
        _plan: &BuildPlan,
        _workspace: &Path,
        _log: &mut dyn LogSink,
    ) -> anyhow::Result<BuildResult> {
        let count = self.fail_count.fetch_add(1, Ordering::SeqCst);
        if count == 0 {
            anyhow::bail!("transient network timeout to package registry");
        }
        let artifact = self.artifacts_dir.join("flaky.tar.gz");
        fs::write(&artifact, "fake").unwrap();
        Ok(BuildResult {
            artifact_path: artifact,
            cache_key: "flaky-key".into(),
            duration: Duration::from_millis(5),
        })
    }
}

#[tokio::test]
async fn test_worker_retryable_failure_and_backoff() {
    let prefix = format!("worker_test_retry_{}", Uuid::new_v4().simple());
    let (db, queue) = get_test_db_and_queue(&prefix).await;

    let src_dir = tempdir().unwrap();
    fs::write(src_dir.path().join("index.html"), "<h1>Flaky</h1>").unwrap();

    let (project_id, deployment_id) = create_test_project_and_deployment(&db, src_dir.path()).await;

    queue
        .enqueue_job(deployment_id, project_id, BuildPriority::Production)
        .await
        .unwrap();

    let artifacts_dir = tempdir().unwrap();
    let artifact_store = ArtifactStore::new(artifacts_dir.path());
    let workspace_dir = tempdir().unwrap();

    let fail_count = Arc::new(AtomicU32::new(0));
    let executor = Arc::new(FlakyExecutor {
        fail_count: fail_count.clone(),
        artifacts_dir: artifacts_dir.path().to_path_buf(),
    });

    let worker = BuildWorker::new(
        "worker-retry",
        db.pool().clone(),
        queue,
        executor,
        artifact_store,
        workspace_dir.path().to_path_buf(),
    );

    let outcome1 = worker.run_once().await.unwrap();
    assert!(matches!(outcome1, BuildWorkerOutcome::Retried(_)));

    let attempt_row = sqlx::query("SELECT status, error_message FROM build_attempts WHERE attempt_number = 1 AND job_id = (SELECT id FROM build_jobs WHERE deployment_id = $1)")
        .bind(deployment_id)
        .fetch_one(db.pool())
        .await
        .unwrap();
    let attempt_status: String = attempt_row.get("status");
    assert_eq!(attempt_status, "failed");

    let job_row = sqlx::query("SELECT status, attempt FROM build_jobs WHERE deployment_id = $1")
        .bind(deployment_id)
        .fetch_one(db.pool())
        .await
        .unwrap();
    let job_status: String = job_row.get("status");
    let attempt: i32 = job_row.get("attempt");
    assert_eq!(job_status, "queued");
    assert_eq!(attempt, 1);
}

#[tokio::test]
async fn test_worker_cancellation_aborts_execution() {
    let prefix = format!("worker_test_cancel_{}", Uuid::new_v4().simple());
    let (db, queue) = get_test_db_and_queue(&prefix).await;

    let src_dir = tempdir().unwrap();
    fs::write(src_dir.path().join("index.html"), "<h1>Cancel</h1>").unwrap();

    let (project_id, deployment_id) = create_test_project_and_deployment(&db, src_dir.path()).await;

    let job = queue
        .enqueue_job(deployment_id, project_id, BuildPriority::Production)
        .await
        .unwrap();

    queue.cancel(job.id).await.unwrap();

    let artifacts_dir = tempdir().unwrap();
    let artifact_store = ArtifactStore::new(artifacts_dir.path());
    let workspace_dir = tempdir().unwrap();
    let executor = Arc::new(MinidockBuildExecutor::new(artifacts_dir.path()));

    let worker = BuildWorker::new(
        "worker-cancel",
        db.pool().clone(),
        queue,
        executor,
        artifact_store,
        workspace_dir.path().to_path_buf(),
    );

    let outcome = worker.run_once().await.unwrap();
    assert!(matches!(
        outcome,
        BuildWorkerOutcome::Cancelled(_) | BuildWorkerOutcome::Idle
    ));
}

#[tokio::test]
async fn test_api_deploy_enqueues_and_returns_202() {
    use axum::body::Body;
    use axum::http::{header, Request, StatusCode};
    use deploy_platform::{
        api::router_with_queue, auth::AuthService, builder::MinidockImageBuilder,
        runtime::MinidockRuntime, service::DeploymentService, store::StateStore,
    };
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    let prefix = format!("api_queue_test_{}", Uuid::new_v4().simple());
    let (db, queue) = get_test_db_and_queue(&prefix).await;

    let temp = tempdir().unwrap();
    let src = temp.path().join("src");
    fs::create_dir_all(&src).unwrap();
    fs::write(src.join("index.html"), "<h1>Async Build</h1>").unwrap();

    let store = StateStore::at(temp.path().join("state.json"));
    let runtime = MinidockRuntime {
        minidock_store: minidock::StateStore::at(temp.path().join("minidock")),
    };
    let service = Arc::new(DeploymentService::new(store, runtime, MinidockImageBuilder));
    let auth = AuthService::new(db.clone());

    // Register test user
    let user_email = format!("async_user_{}@example.com", Uuid::new_v4().simple());
    let user = auth
        .register(&user_email, "Password123!", "Async User")
        .await
        .unwrap();
    let session = auth
        .create_session(user.id, time::Duration::days(1))
        .await
        .unwrap();

    let app = router_with_queue(
        service.clone(),
        Some(auth),
        Some(db.clone()),
        vec![],
        Some(queue.clone()),
    );

    let base = temp.path().join("base.tar.gz");
    fs::write(&base, b"dummy base").unwrap();

    // Create project
    let create_proj_req = Request::builder()
        .method("POST")
        .uri("/v1/projects")
        .header(header::AUTHORIZATION, format!("Bearer {}", session.token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::json!({
                "name": format!("async-proj-{}", Uuid::new_v4().simple()),
                "source_dir": src.to_string_lossy().to_string(),
                "base_image": base.to_string_lossy().to_string()
            })
            .to_string(),
        ))
        .unwrap();

    let resp = app.clone().oneshot(create_proj_req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    let proj: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let project_id = proj["id"].as_str().unwrap();

    // Trigger deployment -> MUST return 202 ACCEPTED
    let deploy_req = Request::builder()
        .method("POST")
        .uri(format!("/v1/projects/{project_id}/deployments"))
        .header(header::AUTHORIZATION, format!("Bearer {}", session.token))
        .body(Body::empty())
        .unwrap();

    let resp = app.clone().oneshot(deploy_req).await.unwrap();
    let status = resp.status();
    let body = resp.into_body().collect().await.unwrap().to_bytes();
    if status != StatusCode::ACCEPTED {
        panic!(
            "unexpected status: {status}, body: {}",
            String::from_utf8_lossy(&body)
        );
    }
    assert_eq!(status, StatusCode::ACCEPTED);
    let dep: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(dep["status"], "queued");
    let deployment_id = Uuid::parse_str(dep["id"].as_str().unwrap()).unwrap();

    // Run worker to process the enqueued job
    let artifacts_dir = tempdir().unwrap();
    let artifact_store = ArtifactStore::new(artifacts_dir.path());
    let workspace_dir = tempdir().unwrap();
    let executor = Arc::new(MinidockBuildExecutor::new(artifacts_dir.path()));

    let worker = BuildWorker::new(
        "worker-async-api",
        db.pool().clone(),
        queue,
        executor,
        artifact_store,
        workspace_dir.path().to_path_buf(),
    );

    let outcome = worker.run_once().await.unwrap();
    assert!(matches!(outcome, BuildWorkerOutcome::Completed(_)));

    // Verify deployment status is running now
    let dep_row = sqlx::query("SELECT status FROM deployments WHERE id = $1")
        .bind(deployment_id)
        .fetch_one(db.pool())
        .await
        .unwrap();
    let status: String = dep_row.get("status");
    assert_eq!(status, "running");
}

struct CountingBuildExecutor {
    inner: MinidockBuildExecutor,
    count: AtomicU32,
}

impl CountingBuildExecutor {
    fn new(artifacts_dir: &Path) -> Self {
        Self {
            inner: MinidockBuildExecutor::new(artifacts_dir),
            count: AtomicU32::new(0),
        }
    }
}

impl BuildExecutor for CountingBuildExecutor {
    fn execute(
        &self,
        plan: &BuildPlan,
        source_dir: &Path,
        sink: &mut dyn LogSink,
    ) -> deploy_platform::Result<BuildResult> {
        self.count.fetch_add(1, Ordering::SeqCst);
        self.inner.execute(plan, source_dir, sink)
    }
}

#[tokio::test]
async fn test_worker_cache_hit_skips_build_execution_and_invalidation() {
    let prefix = format!("worker_cache_{}", Uuid::new_v4().simple());
    let (db, queue) = get_test_db_and_queue(&prefix).await;

    let src_dir = tempdir().unwrap();
    fs::write(src_dir.path().join("index.html"), "<h1>Cache Test</h1>").unwrap();

    let (project_id, deployment_id_1) =
        create_test_project_and_deployment(&db, src_dir.path()).await;

    queue
        .enqueue_job(deployment_id_1, project_id, BuildPriority::Production)
        .await
        .unwrap();

    let artifacts_dir = tempdir().unwrap();
    let artifact_store = ArtifactStore::new(artifacts_dir.path());
    let workspace_dir = tempdir().unwrap();
    let counting_executor = Arc::new(CountingBuildExecutor::new(artifacts_dir.path()));

    let worker = BuildWorker::new(
        "worker-cache-1",
        db.pool().clone(),
        queue.clone(),
        counting_executor.clone(),
        artifact_store.clone(),
        workspace_dir.path().to_path_buf(),
    );

    // Run 1: Cache miss -> executes build (count = 1)
    let outcome1 = worker.run_once().await.unwrap();
    assert!(matches!(outcome1, BuildWorkerOutcome::Completed(_)));
    assert_eq!(counting_executor.count.load(Ordering::SeqCst), 1);

    // Verify build.cache.checked event was emitted with hit: false
    let cache_event_1 = sqlx::query(
        "SELECT payload FROM build_events WHERE deployment_id = $1 AND event_type = 'build.cache.checked'",
    )
    .bind(deployment_id_1)
    .fetch_one(db.pool())
    .await
    .unwrap();
    let payload_1: serde_json::Value = cache_event_1.get("payload");
    assert_eq!(payload_1["hit"], false);

    // Deployment 2 with identical project & source
    let deployment_id_2 = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO deployments (id, project_id, framework, status, created_at)
         VALUES ($1, $2, 'static', 'queued', NOW())",
    )
    .bind(deployment_id_2)
    .bind(project_id)
    .execute(db.pool())
    .await
    .unwrap();

    queue
        .enqueue_job(deployment_id_2, project_id, BuildPriority::Production)
        .await
        .unwrap();

    // Run 2: Cache hit -> should skip execute (count remains 1!)
    let outcome2 = worker.run_once().await.unwrap();
    assert!(matches!(outcome2, BuildWorkerOutcome::Completed(_)));
    assert_eq!(counting_executor.count.load(Ordering::SeqCst), 1);

    // Verify build.cache.checked event was emitted with hit: true
    let cache_event_2 = sqlx::query(
        "SELECT payload FROM build_events WHERE deployment_id = $1 AND event_type = 'build.cache.checked'",
    )
    .bind(deployment_id_2)
    .fetch_one(db.pool())
    .await
    .unwrap();
    let payload_2: serde_json::Value = cache_event_2.get("payload");
    assert_eq!(payload_2["hit"], true);

    // Verify deployment 2 is running and has image_path populated
    let dep_2_row = sqlx::query("SELECT status, image_path FROM deployments WHERE id = $1")
        .bind(deployment_id_2)
        .fetch_one(db.pool())
        .await
        .unwrap();
    let status_2: String = dep_2_row.get("status");
    let image_path_2: Option<String> = dep_2_row.get("image_path");
    assert_eq!(status_2, "running");
    assert!(image_path_2.is_some());

    // Deployment 3: Force rebuild bypasses valid cache
    let deployment_id_3 = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO deployments (id, project_id, framework, status, created_at)
         VALUES ($1, $2, 'static', 'queued', NOW())",
    )
    .bind(deployment_id_3)
    .bind(project_id)
    .execute(db.pool())
    .await
    .unwrap();

    queue
        .enqueue_job_with_force_rebuild(
            deployment_id_3,
            project_id,
            BuildPriority::Production,
            true,
        )
        .await
        .unwrap();

    let outcome3 = worker.run_once().await.unwrap();
    assert!(matches!(outcome3, BuildWorkerOutcome::Completed(_)));
    assert_eq!(counting_executor.count.load(Ordering::SeqCst), 2);

    let cache_event_3 = sqlx::query(
        "SELECT payload FROM build_events WHERE deployment_id = $1 AND event_type = 'build.cache.checked'",
    )
    .bind(deployment_id_3)
    .fetch_one(db.pool())
    .await
    .unwrap();
    let payload_3: serde_json::Value = cache_event_3.get("payload");
    assert_eq!(payload_3["hit"], false);
    assert_eq!(payload_3["reason"], "force_rebuild");

    // Invalidation: clear project cache
    let mut cache_repo = deploy_platform::cache::BuildCacheRepository::new(db.pool().into());
    cache_repo.invalidate_project(project_id).await.unwrap();

    // Deployment 4 after cache invalidation
    let deployment_id_4 = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO deployments (id, project_id, framework, status, created_at)
         VALUES ($1, $2, 'static', 'queued', NOW())",
    )
    .bind(deployment_id_4)
    .bind(project_id)
    .execute(db.pool())
    .await
    .unwrap();

    queue
        .enqueue_job(deployment_id_4, project_id, BuildPriority::Production)
        .await
        .unwrap();

    // Run 4: Cache miss due to invalidation -> count increments to 3!
    let outcome4 = worker.run_once().await.unwrap();
    assert!(matches!(outcome4, BuildWorkerOutcome::Completed(_)));
    assert_eq!(counting_executor.count.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn test_worker_failed_publish_preserves_build_success() {
    let prefix = format!("worker_fail_pub_{}", Uuid::new_v4().simple());
    let (db, queue) = get_test_db_and_queue(&prefix).await;

    let src_dir = tempdir().unwrap();
    fs::write(src_dir.path().join("index.html"), "<h1>Resilient</h1>").unwrap();

    let (project_id, deployment_id) = create_test_project_and_deployment(&db, src_dir.path()).await;

    queue
        .enqueue_job(deployment_id, project_id, BuildPriority::Production)
        .await
        .unwrap();

    let artifacts_dir = tempdir().unwrap();
    let artifact_store = ArtifactStore::new(artifacts_dir.path());
    let workspace_dir = tempdir().unwrap();
    let counting_executor = Arc::new(CountingBuildExecutor::new(artifacts_dir.path()));

    let worker = BuildWorker::new(
        "worker-fail-pub-1",
        db.pool().clone(),
        queue.clone(),
        counting_executor.clone(),
        artifact_store.clone(),
        workspace_dir.path().to_path_buf(),
    );

    // Break cache insertion for this project only
    sqlx::query(
        r#"
        CREATE OR REPLACE FUNCTION fail_cache_insert() RETURNS trigger AS $$
        BEGIN
            RAISE EXCEPTION 'Simulated cache publish failure';
        END;
        $$ LANGUAGE plpgsql;
        "#,
    )
    .execute(db.pool())
    .await
    .unwrap();

    let trigger_sql = format!(
        r#"
        CREATE TRIGGER trigger_fail_cache_insert
        BEFORE INSERT ON build_cache
        FOR EACH ROW
        WHEN (NEW.project_id = '{project_id}')
        EXECUTE FUNCTION fail_cache_insert();
        "#
    );
    sqlx::query(&trigger_sql).execute(db.pool()).await.unwrap();

    // Execute worker: build succeeds even though cache publish throws exception!
    let outcome = worker.run_once().await.unwrap();
    assert!(matches!(outcome, BuildWorkerOutcome::Completed(_)));

    // Verify deployment completed successfully and is running
    let dep_row = sqlx::query("SELECT status, image_path FROM deployments WHERE id = $1")
        .bind(deployment_id)
        .fetch_one(db.pool())
        .await
        .unwrap();
    let status: String = dep_row.get("status");
    assert_eq!(status, "running");

    // Clean up trigger so other tests in same DB aren't affected
    let _ = sqlx::query("DROP TRIGGER IF EXISTS trigger_fail_cache_insert ON build_cache")
        .execute(db.pool())
        .await;
    let _ = sqlx::query("DROP FUNCTION IF EXISTS fail_cache_insert")
        .execute(db.pool())
        .await;
}
