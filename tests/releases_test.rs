use std::time::Duration;
use uuid::Uuid;

use deploy_platform::db::Database;
use deploy_platform::releases::ReleaseStatus;
use deploy_platform::traffic::{DrainResult, MockTrafficRouter, TrafficRouter};

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
        "release-test-{}@example.com",
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
    .bind(format!("release-proj-{}", Uuid::new_v4().simple()))
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
async fn test_release_lifecycle_valid_transitions() {
    let db = setup_test_db().await;
    let (project_id, deployment_id) = create_project_and_deployment(&db).await;
    let mut controller = db.releases();

    // 1. Create release -> Starting, version = 1
    let rel = controller
        .create_release(
            deployment_id,
            project_id,
            "production",
            Some("container-1".into()),
            Some(8080),
            Some("http://127.0.0.1:8080".into()),
        )
        .await
        .unwrap();

    assert_eq!(rel.status, ReleaseStatus::Starting);
    assert_eq!(rel.version, 1);

    // 2. Starting -> HealthChecking
    let rel = controller
        .transition(
            rel.id,
            rel.version,
            ReleaseStatus::HealthChecking,
            Some("health check initiated"),
        )
        .await
        .unwrap();
    assert_eq!(rel.status, ReleaseStatus::HealthChecking);
    assert_eq!(rel.version, 2);

    // 3. HealthChecking -> Ready
    let rel = controller
        .transition(
            rel.id,
            rel.version,
            ReleaseStatus::Ready,
            Some("health check passed"),
        )
        .await
        .unwrap();
    assert_eq!(rel.status, ReleaseStatus::Ready);
    assert_eq!(rel.version, 3);

    // 4. Ready -> Active
    let rel = controller
        .transition(
            rel.id,
            rel.version,
            ReleaseStatus::Active,
            Some("traffic switched"),
        )
        .await
        .unwrap();
    assert_eq!(rel.status, ReleaseStatus::Active);
    assert_eq!(rel.version, 4);

    // 5. Active -> Draining
    let rel = controller
        .transition(
            rel.id,
            rel.version,
            ReleaseStatus::Draining,
            Some("replaced by newer release"),
        )
        .await
        .unwrap();
    assert_eq!(rel.status, ReleaseStatus::Draining);
    assert_eq!(rel.version, 5);

    // 6. Draining -> Stopped
    let rel = controller
        .transition(
            rel.id,
            rel.version,
            ReleaseStatus::Stopped,
            Some("drain complete"),
        )
        .await
        .unwrap();
    assert_eq!(rel.status, ReleaseStatus::Stopped);
    assert_eq!(rel.version, 6);

    // Verify append-only transitions
    let transitions = controller
        .list_transitions_for_release(rel.id)
        .await
        .unwrap();
    assert_eq!(transitions.len(), 5);
    assert_eq!(transitions[0].from_status, ReleaseStatus::Starting);
    assert_eq!(transitions[0].to_status, ReleaseStatus::HealthChecking);
    assert_eq!(transitions[4].from_status, ReleaseStatus::Draining);
    assert_eq!(transitions[4].to_status, ReleaseStatus::Stopped);
}

#[tokio::test]
async fn test_release_invalid_transitions_rejected() {
    let db = setup_test_db().await;
    let (project_id, deployment_id) = create_project_and_deployment(&db).await;
    let mut controller = db.releases();

    let rel = controller
        .create_release(
            deployment_id,
            project_id,
            "production",
            Some("c1".into()),
            Some(8080),
            None,
        )
        .await
        .unwrap();

    // Starting cannot jump directly to Active
    let err = controller
        .transition(rel.id, rel.version, ReleaseStatus::Active, None)
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("invalid release status transition"),
        "unexpected error: {err}"
    );

    // Starting can transition to Failed
    let failed = controller
        .transition(
            rel.id,
            rel.version,
            ReleaseStatus::Failed,
            Some("container crashed"),
        )
        .await
        .unwrap();
    assert_eq!(failed.status, ReleaseStatus::Failed);

    // Failed cannot transition to Ready
    let err = controller
        .transition(failed.id, failed.version, ReleaseStatus::Ready, None)
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("invalid release status transition"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn test_stale_release_version_rejection() {
    let db = setup_test_db().await;
    let (project_id, deployment_id) = create_project_and_deployment(&db).await;
    let mut controller = db.releases();

    let rel = controller
        .create_release(
            deployment_id,
            project_id,
            "production",
            Some("c1".into()),
            Some(8080),
            None,
        )
        .await
        .unwrap();

    // Passing wrong version (e.g. 99 instead of 1) should be rejected
    let err = controller
        .transition(rel.id, 99, ReleaseStatus::HealthChecking, None)
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("stale release version or concurrent modification"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn test_traffic_router_and_duplicate_activation() {
    let db = setup_test_db().await;
    let (project_id, deployment_id) = create_project_and_deployment(&db).await;
    let mut controller = db.releases();
    let mut router = MockTrafficRouter::new();

    // Release 1
    let rel1 = controller
        .create_release(
            deployment_id,
            project_id,
            "production",
            Some("c1".into()),
            Some(8081),
            Some("http://127.0.0.1:8081".into()),
        )
        .await
        .unwrap();
    let rel1 = controller
        .transition(rel1.id, rel1.version, ReleaseStatus::HealthChecking, None)
        .await
        .unwrap();
    let rel1 = controller
        .transition(rel1.id, rel1.version, ReleaseStatus::Ready, None)
        .await
        .unwrap();

    // Activate release 1: no previous release
    let prev = controller
        .activate_traffic(&rel1, &mut router)
        .await
        .unwrap();
    assert!(prev.is_none());

    let active = controller
        .get_active_release(project_id, "production")
        .await
        .unwrap()
        .expect("active release should exist");
    assert_eq!(active.id, rel1.id);
    assert_eq!(active.status, ReleaseStatus::Active);

    // Duplicate activation of rel1 is idempotent
    let prev_dup = controller
        .activate_traffic(&active, &mut router)
        .await
        .unwrap();
    assert!(prev_dup.is_none());

    // Release 2 on the same project & environment
    let rel2 = controller
        .create_release(
            deployment_id,
            project_id,
            "production",
            Some("c2".into()),
            Some(8082),
            Some("http://127.0.0.1:8082".into()),
        )
        .await
        .unwrap();
    let rel2 = controller
        .transition(rel2.id, rel2.version, ReleaseStatus::HealthChecking, None)
        .await
        .unwrap();
    let rel2 = controller
        .transition(rel2.id, rel2.version, ReleaseStatus::Ready, None)
        .await
        .unwrap();

    // Activate release 2: replaces release 1
    let prev2 = controller
        .activate_traffic(&rel2, &mut router)
        .await
        .unwrap();
    assert_eq!(prev2.expect("should return prev release").id, rel1.id);

    let active2 = controller
        .get_active_release(project_id, "production")
        .await
        .unwrap()
        .expect("active release should exist");
    assert_eq!(active2.id, rel2.id);
    assert_eq!(active2.status, ReleaseStatus::Active);

    // Verify router calls
    assert_eq!(router.prepared_routes.lock().unwrap().len(), 3);
    assert_eq!(router.active_routes.lock().unwrap().len(), 1);

    // Drain old release
    let route1 = router.prepare(&rel1).unwrap();
    let drain_res = router.drain(&route1, Duration::from_secs(5)).unwrap();
    assert_eq!(drain_res, DrainResult::CompletedGracefully);
}

#[tokio::test]
async fn test_crash_recovery_reconciliation() {
    let db = setup_test_db().await;
    let (project_id, deployment_id) = create_project_and_deployment(&db).await;
    let mut controller = db.releases();

    // Create releases in various states
    // 1. Starting -> should become Failed
    let rel_starting = controller
        .create_release(
            deployment_id,
            project_id,
            "production",
            Some("c-start".into()),
            Some(8080),
            None,
        )
        .await
        .unwrap();

    // 2. HealthChecking -> should become Failed
    let rel_hc = controller
        .create_release(
            deployment_id,
            project_id,
            "production",
            Some("c-hc".into()),
            Some(8081),
            None,
        )
        .await
        .unwrap();
    let rel_hc = controller
        .transition(
            rel_hc.id,
            rel_hc.version,
            ReleaseStatus::HealthChecking,
            None,
        )
        .await
        .unwrap();

    // 3. Draining -> should become Stopped
    let rel_drain = controller
        .create_release(
            deployment_id,
            project_id,
            "production",
            Some("c-drain".into()),
            Some(8082),
            None,
        )
        .await
        .unwrap();
    let rel_drain = controller
        .transition(
            rel_drain.id,
            rel_drain.version,
            ReleaseStatus::HealthChecking,
            None,
        )
        .await
        .unwrap();
    let rel_drain = controller
        .transition(rel_drain.id, rel_drain.version, ReleaseStatus::Ready, None)
        .await
        .unwrap();
    let rel_drain = controller
        .transition(rel_drain.id, rel_drain.version, ReleaseStatus::Active, None)
        .await
        .unwrap();
    let rel_drain = controller
        .transition(
            rel_drain.id,
            rel_drain.version,
            ReleaseStatus::Draining,
            None,
        )
        .await
        .unwrap();

    // 4. Ready -> should NOT be affected
    let rel_ready = controller
        .create_release(
            deployment_id,
            project_id,
            "production",
            Some("c-ready".into()),
            Some(8083),
            None,
        )
        .await
        .unwrap();
    let rel_ready = controller
        .transition(
            rel_ready.id,
            rel_ready.version,
            ReleaseStatus::HealthChecking,
            None,
        )
        .await
        .unwrap();
    let rel_ready = controller
        .transition(rel_ready.id, rel_ready.version, ReleaseStatus::Ready, None)
        .await
        .unwrap();

    // Reconcile
    let reconciled = controller.reconcile_crashed_releases().await.unwrap();
    let reconciled_ids: Vec<Uuid> = reconciled.iter().map(|r| r.id).collect();
    assert!(reconciled_ids.contains(&rel_starting.id));
    assert!(reconciled_ids.contains(&rel_hc.id));
    assert!(reconciled_ids.contains(&rel_drain.id));
    assert!(!reconciled_ids.contains(&rel_ready.id));

    let updated_starting = controller.get_release(rel_starting.id).await.unwrap();
    assert_eq!(updated_starting.status, ReleaseStatus::Failed);

    let updated_hc = controller.get_release(rel_hc.id).await.unwrap();
    assert_eq!(updated_hc.status, ReleaseStatus::Failed);

    let updated_drain = controller.get_release(rel_drain.id).await.unwrap();
    assert_eq!(updated_drain.status, ReleaseStatus::Stopped);

    let updated_ready = controller.get_release(rel_ready.id).await.unwrap();
    assert_eq!(updated_ready.status, ReleaseStatus::Ready);
}
