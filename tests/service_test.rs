use deploy_platform::{
    builder::{BuildOutput, ImageBuilder},
    detector::StaticSource,
    model::{CreateProjectInput, DeploymentStatus, RunningContainer},
    runtime::Runtime,
    service::{DeploymentService, PlatformService},
    store::StateStore,
    Result,
};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tempfile::tempdir;
use uuid::Uuid;

#[derive(Clone)]
struct FakeBuilder {
    should_fail: bool,
    image_path: PathBuf,
}

impl ImageBuilder for FakeBuilder {
    fn build(&self, _: &Path, _: Uuid, _: &StaticSource, _: &Path) -> Result<BuildOutput> {
        if self.should_fail {
            Err(anyhow::anyhow!("build failed"))
        } else {
            Ok(BuildOutput {
                rootfs_path: PathBuf::from("/tmp/fake-rootfs"),
                image_path: self.image_path.clone(),
            })
        }
    }
}

#[derive(Default)]
struct FakeRuntimeInner {
    started: usize,
    stopped: Vec<Uuid>,
    logs: String,
    fail_start: bool,
    fail_stop: bool,
}

#[derive(Default, Clone)]
struct FakeRuntime {
    inner: std::sync::Arc<Mutex<FakeRuntimeInner>>,
}

impl Runtime for FakeRuntime {
    fn start_static(&mut self, _: &Path, _: &str, _: &[String]) -> Result<RunningContainer> {
        let mut inner = self.inner.lock().unwrap();
        inner.started += 1;
        if inner.fail_start {
            return Err(anyhow::anyhow!("runtime start failed"));
        }
        Ok(RunningContainer {
            id: Uuid::new_v4(),
            port: 43123,
            url: "http://127.0.0.1:43123".into(),
        })
    }

    fn stop(&mut self, id: Uuid) -> Result<()> {
        let mut inner = self.inner.lock().unwrap();
        inner.stopped.push(id);
        if inner.fail_stop {
            return Err(anyhow::anyhow!("runtime stop failed"));
        }
        Ok(())
    }

    fn logs(&self, _: Uuid) -> Result<String> {
        let inner = self.inner.lock().unwrap();
        Ok(inner.logs.clone())
    }
}

fn project_input(source: &Path, base_image: &Path) -> CreateProjectInput {
    CreateProjectInput {
        name: "site".into(),
        source_dir: source.to_path_buf(),
        base_image: base_image.to_path_buf(),
        server_command: vec![
            "/bin/busybox".into(),
            "httpd".into(),
            "-f".into(),
            "-p".into(),
            "{PORT}".into(),
            "-h".into(),
            "/srv/app".into(),
        ],
        user_id: None,
    }
}

fn test_service(
    root: &Path,
    builder: FakeBuilder,
    runtime: FakeRuntime,
) -> DeploymentService<FakeRuntime, FakeBuilder> {
    DeploymentService::new(StateStore::at(root.to_path_buf()), runtime, builder)
}

#[test]
fn deploy_transitions_to_running_and_sets_active_deployment() {
    let root = tempdir().unwrap();
    let source = root.path().join("site");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("index.html"), "hello").unwrap();
    let base_image = root.path().join("base.tar.gz");
    fs::write(&base_image, b"dummy").unwrap();

    let fake_img = root.path().join("fake-image.tar.gz");
    fs::write(&fake_img, b"fake").unwrap();

    let project = project_input(&source, &base_image);
    let service = test_service(
        root.path(),
        FakeBuilder {
            should_fail: false,
            image_path: fake_img,
        },
        FakeRuntime::default(),
    );
    let saved = service.create_project(project).unwrap();
    let deployment = service.deploy(saved.id).unwrap();
    assert_eq!(deployment.status, DeploymentStatus::Running);
    assert_eq!(deployment.url.as_deref(), Some("http://127.0.0.1:43123"));
    assert_eq!(
        service.project(saved.id).unwrap().active_deployment,
        Some(deployment.id)
    );
}

#[test]
fn build_failure_leaves_prior_active_deployment_unchanged() {
    let root = tempdir().unwrap();
    let source = root.path().join("site");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("index.html"), "hello").unwrap();
    let base_image = root.path().join("base.tar.gz");
    fs::write(&base_image, b"dummy").unwrap();
    let fake_img = root.path().join("fake-image.tar.gz");
    fs::write(&fake_img, b"fake").unwrap();

    let project = project_input(&source, &base_image);
    let mut builder = FakeBuilder {
        should_fail: false,
        image_path: fake_img,
    };
    let service = test_service(root.path(), builder.clone(), FakeRuntime::default());
    let saved = service.create_project(project).unwrap();
    let d1 = service.deploy(saved.id).unwrap();
    assert_eq!(d1.status, DeploymentStatus::Running);
    assert_eq!(
        service.project(saved.id).unwrap().active_deployment,
        Some(d1.id)
    );

    // Now make builder fail
    builder.should_fail = true;
    let service2 = test_service(root.path(), builder, FakeRuntime::default());
    let d2_res = service2.deploy(saved.id);
    assert!(d2_res.is_err());

    let deployments = service2.deployments(saved.id).unwrap();
    assert_eq!(deployments.len(), 2);
    let failed = deployments.iter().find(|d| d.id != d1.id).unwrap();
    assert_eq!(failed.status, DeploymentStatus::Failed);
    assert!(failed.error.as_ref().unwrap().contains("build failed"));
    assert_eq!(
        service2.project(saved.id).unwrap().active_deployment,
        Some(d1.id)
    );
}

#[test]
fn runtime_failure_records_failed_and_retains_error() {
    let root = tempdir().unwrap();
    let source = root.path().join("site");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("index.html"), "hello").unwrap();
    let base_image = root.path().join("base.tar.gz");
    fs::write(&base_image, b"dummy").unwrap();
    let fake_img = root.path().join("fake-image.tar.gz");
    fs::write(&fake_img, b"fake").unwrap();

    let project = project_input(&source, &base_image);
    let runtime = FakeRuntime::default();
    runtime.inner.lock().unwrap().fail_start = true;

    let service = test_service(
        root.path(),
        FakeBuilder {
            should_fail: false,
            image_path: fake_img,
        },
        runtime,
    );
    let saved = service.create_project(project).unwrap();
    let res = service.deploy(saved.id);
    assert!(res.is_err());

    let deployments = service.deployments(saved.id).unwrap();
    assert_eq!(deployments.len(), 1);
    assert_eq!(deployments[0].status, DeploymentStatus::Failed);
    assert!(deployments[0]
        .error
        .as_ref()
        .unwrap()
        .contains("runtime start failed"));
}

#[test]
fn stopping_running_deployment_records_stopped() {
    let root = tempdir().unwrap();
    let source = root.path().join("site");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("index.html"), "hello").unwrap();
    let base_image = root.path().join("base.tar.gz");
    fs::write(&base_image, b"dummy").unwrap();
    let fake_img = root.path().join("fake-image.tar.gz");
    fs::write(&fake_img, b"fake").unwrap();

    let project = project_input(&source, &base_image);
    let runtime = FakeRuntime::default();
    let service = test_service(
        root.path(),
        FakeBuilder {
            should_fail: false,
            image_path: fake_img,
        },
        runtime.clone(),
    );
    let saved = service.create_project(project).unwrap();
    let d = service.deploy(saved.id).unwrap();
    assert_eq!(d.status, DeploymentStatus::Running);

    let stopped_d = service.stop(d.id).unwrap();
    assert_eq!(stopped_d.status, DeploymentStatus::Stopped);
    assert_eq!(service.project(saved.id).unwrap().active_deployment, None);
    assert_eq!(runtime.inner.lock().unwrap().stopped.len(), 1);
}

#[test]
fn logs_delegates_to_runtime() {
    let root = tempdir().unwrap();
    let source = root.path().join("site");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("index.html"), "hello").unwrap();
    let base_image = root.path().join("base.tar.gz");
    fs::write(&base_image, b"dummy").unwrap();
    let fake_img = root.path().join("fake-image.tar.gz");
    fs::write(&fake_img, b"fake").unwrap();

    let project = project_input(&source, &base_image);
    let runtime = FakeRuntime::default();
    runtime.inner.lock().unwrap().logs = "server started on port 43123".into();

    let service = test_service(
        root.path(),
        FakeBuilder {
            should_fail: false,
            image_path: fake_img,
        },
        runtime,
    );
    let saved = service.create_project(project).unwrap();
    let d = service.deploy(saved.id).unwrap();
    let logs = service.logs(d.id).unwrap();
    assert_eq!(logs, "server started on port 43123");
}
