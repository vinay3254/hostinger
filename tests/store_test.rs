use deploy_platform::{
    model::{Deployment, DeploymentStatus, Framework, PlatformState, Project},
    store::StateStore,
};
use std::path::PathBuf;
use tempfile::tempdir;
use time::OffsetDateTime;
use uuid::Uuid;

fn project() -> Project {
    Project {
        id: Uuid::new_v4(),
        name: "site".into(),
        user_id: None,
        source_dir: PathBuf::from("/tmp/site"),
        base_image: PathBuf::from("/tmp/base.tar.gz"),
        server_command: vec![
            "/bin/busybox".into(),
            "httpd".into(),
            "-p".into(),
            "{PORT}".into(),
        ],
        created_at: OffsetDateTime::now_utc(),
        active_deployment: None,
    }
}

#[test]
fn save_and_load_round_trip_atomically() {
    let dir = tempdir().unwrap();
    let store = StateStore::at(dir.path().to_path_buf());
    let mut state = PlatformState::empty();
    state.projects.push(project());
    store.save(&state).unwrap();
    assert_eq!(store.load().unwrap(), state);
    assert!(!store.state_path().with_extension("json.tmp").exists());
}

#[test]
fn lookup_missing_uuid_returns_a_clear_error() {
    let store = StateStore::at(tempdir().unwrap().path().to_path_buf());
    let error = store.project(Uuid::new_v4()).unwrap_err();
    assert!(error.to_string().contains("project not found"));
}

#[test]
fn build_directory_is_scoped_to_uuid() {
    let root = tempdir().unwrap();
    let store = StateStore::at(root.path().to_path_buf());
    let id = Uuid::new_v4();
    assert_eq!(
        store.build_dir(id),
        root.path().join("builds").join(id.to_string())
    );
}

#[test]
fn updating_deployment_persists_status() {
    let root = tempdir().unwrap();
    let store = StateStore::at(root.path().to_path_buf());
    let project = project();
    let deployment = Deployment {
        id: Uuid::new_v4(),
        project_id: project.id,
        framework: Framework::Static,
        status: DeploymentStatus::Pending,
        image_path: None,
        container_id: None,
        port: None,
        url: None,
        created_at: OffsetDateTime::now_utc(),
        finished_at: None,
        error: None,
    };
    store.insert_project(project.clone()).unwrap();
    store.insert_deployment(deployment.clone()).unwrap();
    let mut changed = deployment;
    let changed_id = changed.id;
    changed.status = DeploymentStatus::Failed;
    changed.error = Some("build failed".into());
    store.update_deployment(changed).unwrap();
    assert_eq!(
        store.deployment(changed_id).unwrap().status,
        DeploymentStatus::Failed
    );
}
