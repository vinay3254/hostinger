use deploy_platform::{
    db::Database,
    model::{DeploymentStatus, Framework},
    repository::{CreateDeploymentRecord, CreateProjectRecord},
};
use time::OffsetDateTime;
use uuid::Uuid;

async fn setup_test_db() -> Database {
    let url = std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://postgres:postgres@localhost:5432/deploy_platform".into());
    let db = Database::connect(&url)
        .await
        .expect("failed to connect to test database");
    db.migrate().await.expect("failed to run migrations");
    db
}

#[tokio::test]
async fn project_and_deployment_round_trip() {
    let db = setup_test_db().await;

    // Create test user
    let user_id = Uuid::new_v4();
    let email = format!("user-{}@example.com", Uuid::new_v4().simple());
    db.users()
        .create_user(
            user_id,
            &email,
            "hash",
            "Test User",
            OffsetDateTime::now_utc(),
        )
        .await
        .unwrap();

    // Create project
    let project_id = Uuid::new_v4();
    let project_name = format!("project-{}", Uuid::new_v4().simple());
    let project = db
        .projects()
        .create_project(CreateProjectRecord {
            id: project_id,
            user_id,
            name: project_name.clone(),
            source_dir: "/tmp/source".into(),
            base_image: "/tmp/base.tar.gz".into(),
            server_command: vec!["server".into(), "{PORT}".into()],
            created_at: OffsetDateTime::now_utc(),
        })
        .await
        .unwrap();

    assert_eq!(project.id, project_id);
    assert_eq!(project.name, project_name);

    let fetched = db.projects().get_project(project_id).await.unwrap();
    assert_eq!(fetched.id, project_id);
    assert_eq!(fetched.user_id, user_id);

    // Create deployment
    let deployment_id = Uuid::new_v4();
    let deployment = db
        .deployments()
        .create_deployment(CreateDeploymentRecord {
            id: deployment_id,
            project_id,
            framework: Framework::Static,
            status: DeploymentStatus::Pending,
            image_path: None,
            container_id: None,
            port: None,
            url: None,
            created_at: OffsetDateTime::now_utc(),
        })
        .await
        .unwrap();

    assert_eq!(deployment.id, deployment_id);
    assert_eq!(deployment.status, DeploymentStatus::Pending);

    // Update deployment status
    db.deployments()
        .update_deployment_status(
            deployment_id,
            DeploymentStatus::Running,
            Some(43123),
            Some("http://127.0.0.1:43123".into()),
            Some(OffsetDateTime::now_utc()),
            None,
        )
        .await
        .unwrap();

    let updated = db
        .deployments()
        .get_deployment(deployment_id)
        .await
        .unwrap();
    assert_eq!(updated.status, DeploymentStatus::Running);
    assert_eq!(updated.port, Some(43123));
}

#[tokio::test]
async fn rejects_duplicate_project_name_for_same_user() {
    let db = setup_test_db().await;

    let user_id = Uuid::new_v4();
    let email = format!("user-{}@example.com", Uuid::new_v4().simple());
    db.users()
        .create_user(
            user_id,
            &email,
            "hash",
            "Test User",
            OffsetDateTime::now_utc(),
        )
        .await
        .unwrap();

    let name = format!("dup-{}", Uuid::new_v4().simple());
    db.projects()
        .create_project(CreateProjectRecord {
            id: Uuid::new_v4(),
            user_id,
            name: name.clone(),
            source_dir: "/tmp/source".into(),
            base_image: "/tmp/base.tar.gz".into(),
            server_command: vec!["server".into(), "{PORT}".into()],
            created_at: OffsetDateTime::now_utc(),
        })
        .await
        .unwrap();

    let dup_res = db
        .projects()
        .create_project(CreateProjectRecord {
            id: Uuid::new_v4(),
            user_id,
            name: name.clone(),
            source_dir: "/tmp/source".into(),
            base_image: "/tmp/base.tar.gz".into(),
            server_command: vec!["server".into(), "{PORT}".into()],
            created_at: OffsetDateTime::now_utc(),
        })
        .await;

    assert!(dup_res.is_err());
}

#[tokio::test]
async fn transaction_rollback_leaves_no_partial_records() {
    let db = setup_test_db().await;

    let user_id = Uuid::new_v4();
    let email = format!("user-{}@example.com", Uuid::new_v4().simple());
    db.users()
        .create_user(
            user_id,
            &email,
            "hash",
            "Test User",
            OffsetDateTime::now_utc(),
        )
        .await
        .unwrap();

    let project_id = Uuid::new_v4();
    let project_name = format!("tx-{}", Uuid::new_v4().simple());

    let mut tx = db.begin().await.unwrap();
    tx.projects()
        .create_project(CreateProjectRecord {
            id: project_id,
            user_id,
            name: project_name.clone(),
            source_dir: "/tmp/source".into(),
            base_image: "/tmp/base.tar.gz".into(),
            server_command: vec!["server".into(), "{PORT}".into()],
            created_at: OffsetDateTime::now_utc(),
        })
        .await
        .unwrap();

    // Roll back transaction
    tx.rollback().await.unwrap();

    // Verify project does not exist
    let fetch_res = db.projects().get_project(project_id).await;
    assert!(fetch_res.is_err());
}
