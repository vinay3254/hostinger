use deploy_platform::{
    db::Database,
    queue::{BuildQueue, EnqueueJobInput, JobPriority, JobStatus},
};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

async fn setup_test_dbs() -> (Database, redis::Client) {
    let pg_url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/deploy_platform".into());
    let db = Database::connect(&pg_url)
        .await
        .expect("failed to connect to postgres");
    db.migrate().await.expect("failed to run migrations");

    let redis_url = std::env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".into());
    let redis_client = redis::Client::open(redis_url).expect("failed to connect to redis");

    (db, redis_client)
}

async fn create_test_project_and_deployment(db: &Database) -> (Uuid, Uuid) {
    let user_id = Uuid::new_v4();
    let email = format!("q-user-{}@example.com", Uuid::new_v4().simple());
    let now = OffsetDateTime::now_utc();
    db.users()
        .create_user(user_id, &email, "dummyhash", "Queue User", now)
        .await
        .unwrap();

    let project_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO projects (id, user_id, name, source_dir, base_image, server_command, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7)"
    )
    .bind(project_id)
    .bind(user_id)
    .bind(format!("proj-{}", Uuid::new_v4().simple()))
    .bind("/tmp/src")
    .bind("/tmp/base.tar.gz")
    .bind(&["/bin/sh".to_string()])
    .bind(now)
    .execute(db.pool())
    .await
    .unwrap();

    let deployment_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO deployments (id, project_id, framework, status, created_at) VALUES ($1, $2, $3, $4, $5)"
    )
    .bind(deployment_id)
    .bind(project_id)
    .bind("static")
    .bind("queued")
    .bind(now)
    .execute(db.pool())
    .await
    .unwrap();

    (project_id, deployment_id)
}

#[tokio::test]
async fn duplicate_enqueue_is_idempotent() {
    let (db, redis_client) = setup_test_dbs().await;
    let group = format!("test-group-{}", Uuid::new_v4().simple());
    let prefix = format!("test-queue-{}", Uuid::new_v4().simple());
    let queue = BuildQueue::with_prefix(db.clone(), redis_client, group, prefix)
        .await
        .unwrap();

    let (project_id, deployment_id) = create_test_project_and_deployment(&db).await;

    let input = EnqueueJobInput {
        deployment_id,
        project_id,
        priority: JobPriority::Production,
        max_attempts: 3,
    };

    let job1 = queue.enqueue(input.clone()).await.unwrap();
    let job2 = queue.enqueue(input).await.unwrap();

    assert_eq!(job1.id, job2.id);
    assert_eq!(job1.deployment_id, deployment_id);
    assert_eq!(job1.status, JobStatus::Queued);
}

#[tokio::test]
async fn priority_ordering_production_over_preview() {
    let (db, redis_client) = setup_test_dbs().await;
    let group = format!("test-group-{}", Uuid::new_v4().simple());
    let prefix = format!("test-queue-{}", Uuid::new_v4().simple());
    let queue = BuildQueue::with_prefix(db.clone(), redis_client, group, prefix)
        .await
        .unwrap();

    let (proj1, dep1) = create_test_project_and_deployment(&db).await;
    let (proj2, dep2) = create_test_project_and_deployment(&db).await;

    // Enqueue preview first, then production
    let preview_job = queue
        .enqueue(EnqueueJobInput {
            deployment_id: dep1,
            project_id: proj1,
            priority: JobPriority::Preview,
            max_attempts: 3,
        })
        .await
        .unwrap();

    let prod_job = queue
        .enqueue(EnqueueJobInput {
            deployment_id: dep2,
            project_id: proj2,
            priority: JobPriority::Production,
            max_attempts: 3,
        })
        .await
        .unwrap();

    // Claim should give production job first
    let claimed = queue
        .claim("worker-1", Duration::seconds(30))
        .await
        .unwrap();
    assert!(claimed.is_some());
    let claimed_job = claimed.unwrap();
    assert_eq!(claimed_job.job.id, prod_job.id);
    assert_eq!(claimed_job.job.priority, JobPriority::Production);

    // Ack production job
    queue
        .ack(
            claimed_job.job.id,
            "worker-1",
            &claimed_job.stream_name,
            &claimed_job.stream_id,
        )
        .await
        .unwrap();

    // Next claim gets preview job
    let claimed_preview = queue
        .claim("worker-1", Duration::seconds(30))
        .await
        .unwrap();
    assert!(claimed_preview.is_some());
    let claimed_preview_job = claimed_preview.unwrap();
    assert_eq!(claimed_preview_job.job.id, preview_job.id);
}

#[tokio::test]
async fn lease_renewal_and_expiry_requeue() {
    let (db, redis_client) = setup_test_dbs().await;
    let group = format!("test-group-{}", Uuid::new_v4().simple());
    let prefix = format!("test-queue-{}", Uuid::new_v4().simple());
    let queue = BuildQueue::with_prefix(db.clone(), redis_client, group, prefix)
        .await
        .unwrap();

    let (project_id, deployment_id) = create_test_project_and_deployment(&db).await;

    let job = queue
        .enqueue(EnqueueJobInput {
            deployment_id,
            project_id,
            priority: JobPriority::Production,
            max_attempts: 3,
        })
        .await
        .unwrap();

    // Claim with a short 1 second lease
    let claimed = queue
        .claim("worker-lease", Duration::seconds(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claimed.job.id, job.id);

    // Renew lease
    let renewed = queue
        .renew(job.id, "worker-lease", Duration::seconds(10))
        .await
        .unwrap();
    assert!(renewed);

    // Expire lease by force in DB
    sqlx::query(
        "UPDATE build_jobs SET lease_expires_at = NOW() - INTERVAL '10 seconds' WHERE id = $1",
    )
    .bind(job.id)
    .execute(db.pool())
    .await
    .unwrap();

    // Requeue expired
    let requeued_count = queue.requeue_expired().await.unwrap();
    assert!(requeued_count >= 1);

    // Should be claimable again with attempt incremented
    let reclaimed = queue
        .claim("worker-2", Duration::seconds(30))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reclaimed.job.id, job.id);
    assert_eq!(reclaimed.attempt_number, 2);
}

#[tokio::test]
async fn retry_and_dead_letter_transition() {
    let (db, redis_client) = setup_test_dbs().await;
    let group = format!("test-group-{}", Uuid::new_v4().simple());
    let prefix = format!("test-queue-{}", Uuid::new_v4().simple());
    let queue = BuildQueue::with_prefix(db.clone(), redis_client, group, prefix)
        .await
        .unwrap();

    let (project_id, deployment_id) = create_test_project_and_deployment(&db).await;

    let job = queue
        .enqueue(EnqueueJobInput {
            deployment_id,
            project_id,
            priority: JobPriority::Production,
            max_attempts: 2,
        })
        .await
        .unwrap();

    // Attempt 1: Fail retryable
    let claimed = queue
        .claim("worker-retry", Duration::seconds(30))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claimed.attempt_number, 1);
    queue
        .fail(
            job.id,
            "worker-retry",
            &claimed.stream_name,
            &claimed.stream_id,
            "network error",
            true,
        )
        .await
        .unwrap();

    // Attempt 2: Re-claimed
    let claimed2 = queue
        .claim("worker-retry", Duration::seconds(30))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(claimed2.attempt_number, 2);

    // Attempt 2 failure reaches max_attempts -> transitions to dead-letter (failed)
    queue
        .fail(
            job.id,
            "worker-retry",
            &claimed2.stream_name,
            &claimed2.stream_id,
            "unrecoverable error",
            true,
        )
        .await
        .unwrap();

    let job_record = queue.get_job(job.id).await.unwrap().unwrap();
    assert_eq!(job_record.status, JobStatus::Failed);
    assert_eq!(job_record.attempt, 2);
    assert_eq!(
        job_record.last_error.as_deref(),
        Some("unrecoverable error")
    );
}

#[tokio::test]
async fn cancellation_prevents_execution() {
    let (db, redis_client) = setup_test_dbs().await;
    let group = format!("test-group-{}", Uuid::new_v4().simple());
    let prefix = format!("test-queue-{}", Uuid::new_v4().simple());
    let queue = BuildQueue::with_prefix(db.clone(), redis_client, group, prefix)
        .await
        .unwrap();

    let (project_id, deployment_id) = create_test_project_and_deployment(&db).await;

    let job = queue
        .enqueue(EnqueueJobInput {
            deployment_id,
            project_id,
            priority: JobPriority::Production,
            max_attempts: 3,
        })
        .await
        .unwrap();

    // Cancel before claim
    queue.cancel(job.id).await.unwrap();

    let job_record = queue.get_job(job.id).await.unwrap().unwrap();
    assert_eq!(job_record.status, JobStatus::Cancelled);

    // Claim should not return the cancelled job
    let claimed = queue
        .claim("worker-cancel", Duration::seconds(10))
        .await
        .unwrap();
    assert!(claimed.is_none() || claimed.unwrap().job.id != job.id);
}
