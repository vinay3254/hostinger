use std::time::Duration;
use uuid::Uuid;

use deploy_platform::auth::AuthContext;
use deploy_platform::db::Database;
use deploy_platform::health::{HealthPolicy, MockHealthProbeClient};
use deploy_platform::releases::{MockContainerStopper, ReleaseStatus};
use deploy_platform::rollback::{execute_rollback, select_rollback_target, RollbackCommand};
use deploy_platform::traffic::MockTrafficRouter;

async fn setup_test_db() -> Database {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/deploy_platform".into());
    let db = Database::connect(&url)
        .await
        .expect("failed to connect to test database");
    db.migrate().await.expect("failed to run migrations");
    db
}

async fn create_user_and_project(db: &Database) -> (Uuid, Uuid) {
    let user_id = Uuid::new_v4();
    let project_id = Uuid::new_v4();

    sqlx::query(
        "INSERT INTO users (id, email, password_hash, name, created_at)
         VALUES ($1, $2, 'hash', 'Test User', NOW())",
    )
    .bind(user_id)
    .bind(format!("rb-user-{}@example.com", Uuid::new_v4().simple()))
    .execute(db.pool())
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO projects (id, user_id, name, source_dir, base_image, server_command, created_at)
         VALUES ($1, $2, $3, '/tmp', '/tmp', ARRAY['echo'], NOW())",
    )
    .bind(project_id)
    .bind(user_id)
    .bind(format!("rb-proj-{}", Uuid::new_v4().simple()))
    .execute(db.pool())
    .await
    .unwrap();

    (user_id, project_id)
}

async fn create_deployment_and_release(
    db: &Database,
    project_id: Uuid,
    status: &str,
    release_status: ReleaseStatus,
    commit_sha: Option<&str>,
    image_path: Option<&str>,
    port: i32,
) -> (Uuid, Uuid) {
    let deployment_id = Uuid::new_v4();
    let release_id = Uuid::new_v4();
    let now = time::OffsetDateTime::now_utc();

    sqlx::query(
        "INSERT INTO deployments (id, project_id, framework, status, image_path, commit_sha, target, created_at)
         VALUES ($1, $2, 'static', $3, $4, $5, 'production', $6)",
    )
    .bind(deployment_id)
    .bind(project_id)
    .bind(status)
    .bind(image_path)
    .bind(commit_sha)
    .bind(now)
    .execute(db.pool())
    .await
    .unwrap();

    sqlx::query(
        "INSERT INTO releases (id, deployment_id, project_id, environment, status, version, container_id, port, url, created_at, updated_at)
         VALUES ($1, $2, $3, 'production', $4, 1, $5, $6, $7, $8, $8)",
    )
    .bind(release_id)
    .bind(deployment_id)
    .bind(project_id)
    .bind(release_status.as_str())
    .bind(format!("c-{}", release_id.simple()))
    .bind(port)
    .bind(format!("http://127.0.0.1:{port}"))
    .bind(now)
    .execute(db.pool())
    .await
    .unwrap();

    (deployment_id, release_id)
}

#[tokio::test]
async fn test_rollback_no_predecessor_fails() {
    let db = setup_test_db().await;
    let (_user_id, project_id) = create_user_and_project(&db).await;

    let (dep_id, _) = create_deployment_and_release(
        &db,
        project_id,
        "running",
        ReleaseStatus::Active,
        Some("c1"),
        Some("/img1"),
        8081,
    )
    .await;

    let err = select_rollback_target(db.pool(), project_id, "production", dep_id)
        .await
        .unwrap_err();
    assert!(
        err.to_string()
            .contains("no healthy predecessor release found"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn test_rollback_ignores_failed_predecessors() {
    let db = setup_test_db().await;
    let (_user_id, project_id) = create_user_and_project(&db).await;

    // Deployment 1: Healthy & stopped
    let (dep1_id, rel1_id) = create_deployment_and_release(
        &db,
        project_id,
        "stopped",
        ReleaseStatus::Stopped,
        Some("sha-healthy-1"),
        Some("/img-healthy-1"),
        8081,
    )
    .await;

    // Small delay to ensure timestamp progression
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Deployment 2: Failed predecessor
    let (_dep2_id, _rel2_id) = create_deployment_and_release(
        &db,
        project_id,
        "failed",
        ReleaseStatus::Failed,
        Some("sha-bad-2"),
        Some("/img-bad-2"),
        8082,
    )
    .await;

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Deployment 3: Current active deployment
    let (dep3_id, _) = create_deployment_and_release(
        &db,
        project_id,
        "running",
        ReleaseStatus::Active,
        Some("sha-current-3"),
        Some("/img-current-3"),
        8083,
    )
    .await;

    // Target selection MUST skip the failed Deployment 2 and pick Deployment 1
    let target = select_rollback_target(db.pool(), project_id, "production", dep3_id)
        .await
        .unwrap();

    assert_eq!(target.deployment_id, dep1_id);
    assert_eq!(target.release_id, rel1_id);
    assert_eq!(target.commit_sha.as_deref(), Some("sha-healthy-1"));
}

#[tokio::test]
async fn test_rollback_exact_artifact_reuse_and_cause() {
    let db = setup_test_db().await;
    let (user_id, project_id) = create_user_and_project(&db).await;

    // Deployment 1: Initial healthy deployment
    let (dep1_id, rel1_id) = create_deployment_and_release(
        &db,
        project_id,
        "stopped",
        ReleaseStatus::Stopped,
        Some("commit-v1-stable"),
        Some("/artifacts/v1.tar.gz"),
        8081,
    )
    .await;

    tokio::time::sleep(Duration::from_millis(50)).await;

    // Deployment 2: Current running deployment
    let (dep2_id, rel2_id) = create_deployment_and_release(
        &db,
        project_id,
        "running",
        ReleaseStatus::Active,
        Some("commit-v2-buggy"),
        Some("/artifacts/v2.tar.gz"),
        8082,
    )
    .await;

    // Set deployment 2 as active route
    let mut router = MockTrafficRouter::new();
    let rel2 = db.releases().get_release(rel2_id).await.unwrap();
    db.releases()
        .activate_traffic(&rel2, &mut router)
        .await
        .unwrap();

    let auth = AuthContext {
        user_id,
        session_id: None,
        scopes: vec!["production:operate".into()],
    };

    let cmd = RollbackCommand {
        project_id,
        environment: "production".into(),
        current_deployment_id: dep2_id,
        actor_id: user_id,
        health_policy: Some(HealthPolicy {
            attempts: 2,
            interval: Duration::from_millis(10),
            timeout: Duration::from_millis(50),
            ..Default::default()
        }),
        drain_timeout: Some(Duration::from_millis(50)),
    };

    let health_client = MockHealthProbeClient::with_success();
    let mut stopper = MockContainerStopper::new();

    let result = execute_rollback(&db, &auth, cmd, &health_client, &mut router, &mut stopper)
        .await
        .unwrap();

    assert_eq!(result.target.deployment_id, dep1_id);
    assert_eq!(result.target.release_id, rel1_id);

    // Verify new deployment in DB
    let row = sqlx::query(
        "SELECT id, project_id, status, commit_sha, image_path, cause, rollback_from_deployment_id
         FROM deployments WHERE id = $1",
    )
    .bind(result.new_deployment_id)
    .fetch_one(db.pool())
    .await
    .unwrap();

    let cause: String = sqlx::Row::get(&row, "cause");
    let rollback_from: Option<Uuid> = sqlx::Row::get(&row, "rollback_from_deployment_id");
    let commit_sha: Option<String> = sqlx::Row::get(&row, "commit_sha");
    let image_path: Option<String> = sqlx::Row::get(&row, "image_path");

    assert_eq!(cause, "rollback");
    assert_eq!(rollback_from, Some(dep2_id));
    assert_eq!(commit_sha.as_deref(), Some("commit-v1-stable"));
    assert_eq!(image_path.as_deref(), Some("/artifacts/v1.tar.gz"));

    // Active route now points to new release
    let active_route = db
        .releases()
        .get_active_release(project_id, "production")
        .await
        .unwrap()
        .expect("active release must exist");
    assert_eq!(active_route.id, result.active_release.id);
    assert_eq!(active_route.status, ReleaseStatus::Active);

    // Previous active release was drained
    let prev = result
        .previous_release
        .expect("previous release must exist");
    assert_eq!(prev.id, rel2_id);
    assert_eq!(prev.status, ReleaseStatus::Stopped);
}

#[tokio::test]
async fn test_rollback_production_permission_enforcement() {
    let db = setup_test_db().await;
    let (user_id, project_id) = create_user_and_project(&db).await;

    let (_dep1_id, _) = create_deployment_and_release(
        &db,
        project_id,
        "stopped",
        ReleaseStatus::Stopped,
        Some("c1"),
        Some("/img1"),
        8081,
    )
    .await;
    let (dep2_id, _) = create_deployment_and_release(
        &db,
        project_id,
        "running",
        ReleaseStatus::Active,
        Some("c2"),
        Some("/img2"),
        8082,
    )
    .await;

    // 1. User with only "deploy:write" (not production operator)
    let non_prod_auth = AuthContext {
        user_id,
        session_id: None,
        scopes: vec!["deploy:write".into()],
    };

    let cmd = RollbackCommand {
        project_id,
        environment: "production".into(),
        current_deployment_id: dep2_id,
        actor_id: user_id,
        health_policy: None,
        drain_timeout: None,
    };

    let client = MockHealthProbeClient::with_success();
    let mut router = MockTrafficRouter::new();
    let mut stopper = MockContainerStopper::new();

    let err = execute_rollback(
        &db,
        &non_prod_auth,
        cmd.clone(),
        &client,
        &mut router,
        &mut stopper,
    )
    .await
    .unwrap_err();

    assert!(
        err.to_string()
            .contains("only production operators can rollback production traffic"),
        "unexpected error: {err}"
    );

    // 2. Different non-owner user
    let stranger_auth = AuthContext {
        user_id: Uuid::new_v4(),
        session_id: None,
        scopes: vec!["production:operate".into()],
    };

    let err_stranger =
        execute_rollback(&db, &stranger_auth, cmd, &client, &mut router, &mut stopper)
            .await
            .unwrap_err();

    assert!(
        err_stranger
            .to_string()
            .contains("user does not have access to this project"),
        "unexpected error: {err_stranger}"
    );
}

#[tokio::test]
async fn test_rollback_audit_event_recorded() {
    let db = setup_test_db().await;
    let (user_id, project_id) = create_user_and_project(&db).await;

    let (_dep1_id, _) = create_deployment_and_release(
        &db,
        project_id,
        "stopped",
        ReleaseStatus::Stopped,
        Some("c1"),
        Some("/img1"),
        8081,
    )
    .await;
    let (dep2_id, _) = create_deployment_and_release(
        &db,
        project_id,
        "running",
        ReleaseStatus::Active,
        Some("c2"),
        Some("/img2"),
        8082,
    )
    .await;

    let auth = AuthContext {
        user_id,
        session_id: None,
        scopes: vec!["deploy:rollback".into()],
    };

    let cmd = RollbackCommand {
        project_id,
        environment: "production".into(),
        current_deployment_id: dep2_id,
        actor_id: user_id,
        health_policy: Some(HealthPolicy {
            attempts: 1,
            ..Default::default()
        }),
        drain_timeout: Some(Duration::from_millis(10)),
    };

    let client = MockHealthProbeClient::with_success();
    let mut router = MockTrafficRouter::new();
    let mut stopper = MockContainerStopper::new();

    let result = execute_rollback(&db, &auth, cmd, &client, &mut router, &mut stopper)
        .await
        .unwrap();

    // Verify audit event
    let events = db.audit().list_by_user(user_id).await.unwrap();
    let rollback_event = events
        .into_iter()
        .find(|e| e.id == result.audit_event_id)
        .expect("audit event must exist");

    assert_eq!(rollback_event.action, "deployment.rollback");
    assert_eq!(rollback_event.target_type, "deployment");
    assert_eq!(rollback_event.target_id, result.new_deployment_id);
    assert_eq!(
        rollback_event.metadata["project_id"].as_str().unwrap(),
        project_id.to_string()
    );
}
