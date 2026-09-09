use std::collections::HashMap;
use std::time::Duration;
use uuid::Uuid;

use deploy_platform::db::Database;
use deploy_platform::health::{
    wait_until_healthy, HealthPolicy, MockHealthProbeClient, ProbeResponse,
};
use deploy_platform::releases::{
    MockContainerStopper, ReleaseOrchestrationPlan, ReleaseRecoveryWorker, ReleaseStatus,
};
use deploy_platform::traffic::{DrainResult, MockTrafficRouter};

async fn setup_test_db() -> Database {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/deploy_platform".into());
    let db = Database::connect(&url)
        .await
        .expect("failed to connect to test database");
    db.migrate().await.expect("failed to run migrations");
    db
}

async fn create_project_and_deployment(db: &Database) -> (Uuid, Uuid) {
    let user_id = Uuid::new_v4();
    let project_id = Uuid::new_v4();
    let deployment_id = Uuid::new_v4();

    sqlx::query(
        "INSERT INTO users (id, email, password_hash, name, created_at)
         VALUES ($1, $2, 'hash', 'Test User', NOW())",
    )
    .bind(user_id)
    .bind(format!(
        "health-test-{}@example.com",
        Uuid::new_v4().simple()
    ))
    .execute(db.pool())
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO projects (id, user_id, name, source_dir, base_image, server_command, created_at)
         VALUES ($1, $2, $3, '/tmp', '/tmp', ARRAY['echo'], NOW())",
    )
    .bind(project_id)
    .bind(user_id)
    .bind(format!("health-proj-{}", Uuid::new_v4().simple()))
    .execute(db.pool())
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO deployments (id, project_id, framework, status, created_at)
         VALUES ($1, $2, 'static', 'running', NOW())",
    )
    .bind(deployment_id)
    .bind(project_id)
    .execute(db.pool())
    .await
    .unwrap();

    (project_id, deployment_id)
}

#[tokio::test]
async fn test_healthy_replacement_orchestration() {
    let db = setup_test_db().await;
    let (project_id, deployment_id) = create_project_and_deployment(&db).await;
    let mut controller = db.releases();
    let mut router = MockTrafficRouter::new();
    let mut stopper = MockContainerStopper::new();

    // 1. Initial active release (Release 1)
    let plan1 = ReleaseOrchestrationPlan {
        deployment_id,
        project_id,
        environment: "production".into(),
        container_id: "c-initial".into(),
        port: 8081,
        url: "http://127.0.0.1:8081".into(),
        health_policy: HealthPolicy {
            attempts: 3,
            interval: Duration::from_millis(10),
            timeout: Duration::from_millis(100),
            ..Default::default()
        },
        drain_timeout: Duration::from_millis(50),
    };

    let client1 = MockHealthProbeClient::with_success();
    let res1 = controller
        .orchestrate_release(plan1, &client1, &mut router, &mut stopper)
        .await
        .unwrap();

    assert_eq!(res1.active_release.status, ReleaseStatus::Active);
    assert!(res1.previous_release.is_none());

    let initial_active = controller
        .get_active_release(project_id, "production")
        .await
        .unwrap()
        .expect("initial release should be active");
    assert_eq!(initial_active.id, res1.active_release.id);

    // 2. Replacement release (Release 2)
    let plan2 = ReleaseOrchestrationPlan {
        deployment_id,
        project_id,
        environment: "production".into(),
        container_id: "c-replacement".into(),
        port: 8082,
        url: "http://127.0.0.1:8082".into(),
        health_policy: HealthPolicy {
            attempts: 3,
            interval: Duration::from_millis(10),
            timeout: Duration::from_millis(100),
            ..Default::default()
        },
        drain_timeout: Duration::from_millis(50),
    };

    let client2 = MockHealthProbeClient::with_success();
    let res2 = controller
        .orchestrate_release(plan2, &client2, &mut router, &mut stopper)
        .await
        .unwrap();

    // Release 2 is active
    assert_eq!(res2.active_release.status, ReleaseStatus::Active);
    assert_eq!(res2.drain_result, Some(DrainResult::CompletedGracefully));

    // Previous release was drained and stopped
    let prev = res2
        .previous_release
        .expect("previous release should exist");
    assert_eq!(prev.id, res1.active_release.id);
    assert_eq!(prev.status, ReleaseStatus::Stopped);

    // Old container was stopped
    assert!(stopper
        .stopped
        .lock()
        .unwrap()
        .contains(&"c-initial".to_string()));

    // Active route now points to release 2
    let current_active = controller
        .get_active_release(project_id, "production")
        .await
        .unwrap()
        .expect("current active release should exist");
    assert_eq!(current_active.id, res2.active_release.id);
}

#[tokio::test]
async fn test_health_check_non_2xx_preserves_old_release() {
    let db = setup_test_db().await;
    let (project_id, deployment_id) = create_project_and_deployment(&db).await;
    let mut controller = db.releases();
    let mut router = MockTrafficRouter::new();
    let mut stopper = MockContainerStopper::new();

    // 1. Initial healthy release
    let plan1 = ReleaseOrchestrationPlan {
        deployment_id,
        project_id,
        environment: "production".into(),
        container_id: "c-old-stable".into(),
        port: 8081,
        url: "http://127.0.0.1:8081".into(),
        health_policy: HealthPolicy {
            attempts: 2,
            interval: Duration::from_millis(10),
            timeout: Duration::from_millis(100),
            ..Default::default()
        },
        drain_timeout: Duration::from_millis(50),
    };

    let client1 = MockHealthProbeClient::with_success();
    let res1 = controller
        .orchestrate_release(plan1, &client1, &mut router, &mut stopper)
        .await
        .unwrap();
    assert_eq!(res1.active_release.status, ReleaseStatus::Active);

    // 2. Faulty release returning HTTP 500
    let plan2 = ReleaseOrchestrationPlan {
        deployment_id,
        project_id,
        environment: "production".into(),
        container_id: "c-bad-replacement".into(),
        port: 8082,
        url: "http://127.0.0.1:8082".into(),
        health_policy: HealthPolicy {
            attempts: 2,
            interval: Duration::from_millis(10),
            timeout: Duration::from_millis(100),
            ..Default::default()
        },
        drain_timeout: Duration::from_millis(50),
    };

    let client2 = MockHealthProbeClient::with_status_sequence(vec![500, 500]);
    let err = controller
        .orchestrate_release(plan2, &client2, &mut router, &mut stopper)
        .await
        .unwrap_err();

    assert!(
        err.to_string().contains("release health check failed"),
        "unexpected error: {err}"
    );

    // Bad replacement container was stopped
    assert!(stopper
        .stopped
        .lock()
        .unwrap()
        .contains(&"c-bad-replacement".to_string()));

    // CRITICAL: Old stable release is STILL active and untouched!
    let current_active = controller
        .get_active_release(project_id, "production")
        .await
        .unwrap()
        .expect("active release should still be present");
    assert_eq!(current_active.id, res1.active_release.id);
    assert_eq!(current_active.status, ReleaseStatus::Active);
}

#[tokio::test]
async fn test_health_check_timeout_fails_and_preserves_old_release() {
    let db = setup_test_db().await;
    let (project_id, deployment_id) = create_project_and_deployment(&db).await;
    let mut controller = db.releases();
    let mut router = MockTrafficRouter::new();
    let mut stopper = MockContainerStopper::new();

    // 1. Initial healthy release
    let plan1 = ReleaseOrchestrationPlan {
        deployment_id,
        project_id,
        environment: "production".into(),
        container_id: "c-healthy-original".into(),
        port: 8081,
        url: "http://127.0.0.1:8081".into(),
        health_policy: HealthPolicy {
            attempts: 2,
            interval: Duration::from_millis(10),
            timeout: Duration::from_millis(50),
            ..Default::default()
        },
        drain_timeout: Duration::from_millis(50),
    };

    let client1 = MockHealthProbeClient::with_success();
    let res1 = controller
        .orchestrate_release(plan1, &client1, &mut router, &mut stopper)
        .await
        .unwrap();

    // 2. Hanging/timeout release
    let plan2 = ReleaseOrchestrationPlan {
        deployment_id,
        project_id,
        environment: "production".into(),
        container_id: "c-timeout-candidate".into(),
        port: 8082,
        url: "http://127.0.0.1:8082".into(),
        health_policy: HealthPolicy {
            attempts: 2,
            interval: Duration::from_millis(10),
            timeout: Duration::from_millis(50),
            ..Default::default()
        },
        drain_timeout: Duration::from_millis(50),
    };

    let client2 = MockHealthProbeClient::with_responses(vec![
        Err("request timed out".into()),
        Err("request timed out".into()),
    ]);
    let err = controller
        .orchestrate_release(plan2, &client2, &mut router, &mut stopper)
        .await
        .unwrap_err();

    assert!(err.to_string().contains("release health check failed"));

    // Old release still serving traffic
    let current_active = controller
        .get_active_release(project_id, "production")
        .await
        .unwrap()
        .expect("active release should still be present");
    assert_eq!(current_active.id, res1.active_release.id);
    assert_eq!(current_active.status, ReleaseStatus::Active);
}

#[tokio::test]
async fn test_health_policy_assertions_and_retry_recovery() {
    // 1. Retry recovery: 2 failures then success
    let client = MockHealthProbeClient::with_failures_then_success(2);
    let policy = HealthPolicy {
        attempts: 3,
        interval: Duration::from_millis(10),
        timeout: Duration::from_millis(50),
        ..Default::default()
    };

    let result = wait_until_healthy(&client, "http://127.0.0.1:8080", &policy).await;
    assert!(result.is_healthy);
    assert_eq!(result.attempts.len(), 3);
    assert!(!result.attempts[0].is_healthy);
    assert!(!result.attempts[1].is_healthy);
    assert!(result.attempts[2].is_healthy);

    // 2. Custom header and body content assertions
    let mut expected_headers = HashMap::new();
    expected_headers.insert("x-server-version".into(), "2.0.0".into());

    let policy_strict = HealthPolicy {
        attempts: 1,
        interval: Duration::from_millis(10),
        timeout: Duration::from_millis(50),
        expected_status: 200,
        expected_body_contains: Some("healthy-ready".into()),
        expected_headers,
        ..Default::default()
    };

    // Fails body assertion
    let client_bad_body = MockHealthProbeClient::with_responses(vec![Ok(ProbeResponse {
        status: 200,
        headers: HashMap::from([("x-server-version".into(), "2.0.0".into())]),
        body: "bad-body".into(),
    })]);
    let bad_res =
        wait_until_healthy(&client_bad_body, "http://127.0.0.1:8080", &policy_strict).await;
    assert!(!bad_res.is_healthy);
    assert!(bad_res
        .final_error
        .unwrap()
        .contains("response body does not contain expected substring"));

    // Passes both body and header assertions
    let client_good = MockHealthProbeClient::with_responses(vec![Ok(ProbeResponse {
        status: 200,
        headers: HashMap::from([("x-server-version".into(), "2.0.0".into())]),
        body: "service is healthy-ready!".into(),
    })]);
    let good_res = wait_until_healthy(&client_good, "http://127.0.0.1:8080", &policy_strict).await;
    assert!(good_res.is_healthy);
}

#[tokio::test]
async fn test_recovery_worker_reconciles_interrupted_releases() {
    let db = setup_test_db().await;
    let (project_id, deployment_id) = create_project_and_deployment(&db).await;
    let mut controller = db.releases();
    let mut stopper = MockContainerStopper::new();
    let mut router = MockTrafficRouter::new();

    // 1. Starting release
    let r_starting = controller
        .create_release(
            deployment_id,
            project_id,
            "production",
            Some("c-start-crashed".into()),
            Some(8081),
            None,
        )
        .await
        .unwrap();

    // 2. HealthChecking release
    let r_hc = controller
        .create_release(
            deployment_id,
            project_id,
            "production",
            Some("c-hc-crashed".into()),
            Some(8082),
            None,
        )
        .await
        .unwrap();
    let r_hc = controller
        .transition(r_hc.id, r_hc.version, ReleaseStatus::HealthChecking, None)
        .await
        .unwrap();

    // 3. Unrouted Ready release (interrupted before activation)
    let r_ready = controller
        .create_release(
            deployment_id,
            project_id,
            "production",
            Some("c-ready-crashed".into()),
            Some(8083),
            None,
        )
        .await
        .unwrap();
    let r_ready = controller
        .transition(
            r_ready.id,
            r_ready.version,
            ReleaseStatus::HealthChecking,
            None,
        )
        .await
        .unwrap();
    let r_ready = controller
        .transition(r_ready.id, r_ready.version, ReleaseStatus::Ready, None)
        .await
        .unwrap();

    // 4. Draining release (interrupted during drain)
    let r_drain = controller
        .create_release(
            deployment_id,
            project_id,
            "production",
            Some("c-drain-crashed".into()),
            Some(8084),
            None,
        )
        .await
        .unwrap();
    let r_drain = controller
        .transition(
            r_drain.id,
            r_drain.version,
            ReleaseStatus::HealthChecking,
            None,
        )
        .await
        .unwrap();
    let r_drain = controller
        .transition(r_drain.id, r_drain.version, ReleaseStatus::Ready, None)
        .await
        .unwrap();
    let r_drain = controller
        .transition(r_drain.id, r_drain.version, ReleaseStatus::Active, None)
        .await
        .unwrap();
    let r_drain = controller
        .transition(r_drain.id, r_drain.version, ReleaseStatus::Draining, None)
        .await
        .unwrap();

    // Run recovery worker
    let mut worker = ReleaseRecoveryWorker::new(controller);
    let reconciled = worker
        .run_recovery(Some(&mut stopper), Some(&mut router))
        .await
        .unwrap();

    let reconciled_ids: Vec<Uuid> = reconciled.iter().map(|r| r.id).collect();
    assert!(reconciled_ids.contains(&r_starting.id));
    assert!(reconciled_ids.contains(&r_hc.id));
    assert!(reconciled_ids.contains(&r_ready.id));
    assert!(reconciled_ids.contains(&r_drain.id));

    let mut controller = db.releases();
    assert_eq!(
        controller.get_release(r_starting.id).await.unwrap().status,
        ReleaseStatus::Failed
    );
    assert_eq!(
        controller.get_release(r_hc.id).await.unwrap().status,
        ReleaseStatus::Failed
    );
    assert_eq!(
        controller.get_release(r_ready.id).await.unwrap().status,
        ReleaseStatus::Failed
    );
    assert_eq!(
        controller.get_release(r_drain.id).await.unwrap().status,
        ReleaseStatus::Stopped
    );

    // Verify containers stopped
    let stopped = stopper.stopped.lock().unwrap();
    assert!(stopped.contains(&"c-start-crashed".to_string()));
    assert!(stopped.contains(&"c-hc-crashed".to_string()));
    assert!(stopped.contains(&"c-ready-crashed".to_string()));
    assert!(stopped.contains(&"c-drain-crashed".to_string()));
}
