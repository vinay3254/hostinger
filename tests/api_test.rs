use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use deploy_platform::{
    api::router,
    builder::{BuildOutput, ImageBuilder},
    detector::StaticSource,
    model::RunningContainer,
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
use tower::ServiceExt;
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

fn test_service() -> Arc<dyn PlatformService> {
    let root = tempfile::TempDir::keep(tempdir().unwrap());
    let fake_img = root.join("fake-image.tar.gz");
    fs::write(&fake_img, b"fake").unwrap();

    let builder = FakeBuilder {
        should_fail: false,
        image_path: fake_img,
    };
    let runtime = FakeRuntime::default();
    Arc::new(DeploymentService::new(
        StateStore::at(root),
        runtime,
        builder,
    ))
}

#[tokio::test]
async fn health_endpoint_returns_ok_json() {
    let response = router(test_service())
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&body[..], br#"{"status":"ok"}"#);
}

#[tokio::test]
async fn unknown_project_id_returns_json_not_found() {
    let response = router(test_service())
        .oneshot(
            Request::builder()
                .uri(format!("/v1/projects/{}/deployments", Uuid::new_v4()))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8_lossy(&body).contains("error"));
}

#[tokio::test]
async fn malformed_uuid_returns_bad_request() {
    let response = router(test_service())
        .oneshot(
            Request::builder()
                .uri("/v1/projects/not-a-uuid")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8_lossy(&body).contains("invalid UUID"));
}

#[tokio::test]
async fn create_project_and_deploy_lifecycle() {
    let temp = tempdir().unwrap();
    let source = temp.path().join("site");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("index.html"), "hello").unwrap();
    let base = temp.path().join("base.tar.gz");
    fs::write(&base, b"dummy").unwrap();

    let service = test_service();
    let app = router(service);

    // Create project
    let req_body = serde_json::json!({
        "name": "landing",
        "source_dir": source.to_str().unwrap(),
        "base_image": base.to_str().unwrap(),
    });
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/projects")
                .header("Content-Type", "application/json")
                .body(Body::from(req_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let project: serde_json::Value = serde_json::from_slice(&body).unwrap();
    let project_id = project["id"].as_str().unwrap();

    // Get project
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/projects/{project_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Deploy
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/projects/{project_id}/deployments"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let deployment: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(deployment["status"], "running");
    let deployment_id = deployment["id"].as_str().unwrap();

    // List deployments
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/projects/{project_id}/deployments"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Get deployment
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/deployments/{deployment_id}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Get logs
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/deployments/{deployment_id}/logs"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // Stop deployment
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/deployments/{deployment_id}/stop"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let stopped: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(stopped["status"], "stopped");
}
