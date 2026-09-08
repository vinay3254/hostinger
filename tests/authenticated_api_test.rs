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
    fs::write(source.join("index.html"), "<h1>Hello Authenticated</h1>").unwrap();

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
        .expect("failed to connect to test database");
    db.migrate().await.expect("failed to run migrations");
    db
}

#[tokio::test]
async fn unauthenticated_requests_return_401() {
    let db = setup_test_db().await;
    let auth = AuthService::new(db.clone());
    let (service, source, base_img) = test_service();
    let app = router_with_auth(service, Some(auth), Some(db));

    // GET /v1/projects
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/projects")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // POST /v1/projects
    let body = serde_json::json!({
        "name": "unauth-project",
        "source_dir": source,
        "base_image": base_img,
    });
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/projects")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // GET /v1/projects/:id
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/projects/{}", Uuid::new_v4()))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn authenticated_ownership_and_cross_user_isolation_403() {
    let db = setup_test_db().await;
    let auth = AuthService::new(db.clone());
    let (service, source, base_img) = test_service();
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
        .create_session(user_a.id, time::Duration::days(1))
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
        .create_session(user_b.id, time::Duration::days(1))
        .await
        .unwrap();

    // User A creates project
    let req_body = serde_json::json!({
        "name": "site-a",
        "source_dir": source,
        "base_image": base_img,
    });
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/projects")
                .header(header::AUTHORIZATION, format!("Bearer {}", session_a.token))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(req_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let project_a: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let project_a_id = project_a["id"].as_str().unwrap();

    // User A can access project A
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/projects/{project_a_id}"))
                .header(header::AUTHORIZATION, format!("Bearer {}", session_a.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // User B tries to access project A -> 403 Forbidden
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/projects/{project_a_id}"))
                .header(header::AUTHORIZATION, format!("Bearer {}", session_b.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // User B tries to deploy project A -> 403 Forbidden
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/projects/{project_a_id}/deployments"))
                .header(header::AUTHORIZATION, format!("Bearer {}", session_b.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // User A deploys project A -> 201 Created
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/projects/{project_a_id}/deployments"))
                .header(header::AUTHORIZATION, format!("Bearer {}", session_a.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let deployment_a: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let deployment_a_id = deployment_a["id"].as_str().unwrap();

    // User B tries to get deployment logs -> 403 Forbidden
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/deployments/{deployment_a_id}/logs"))
                .header(header::AUTHORIZATION, format!("Bearer {}", session_b.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // User B tries to stop deployment A -> 403 Forbidden
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/deployments/{deployment_a_id}/stop"))
                .header(header::AUTHORIZATION, format!("Bearer {}", session_b.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // User A lists projects -> sees project A
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/projects")
                .header(header::AUTHORIZATION, format!("Bearer {}", session_a.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let list_a: Vec<serde_json::Value> = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(list_a.len(), 1);

    // User B lists projects -> sees 0 projects
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/projects")
                .header(header::AUTHORIZATION, format!("Bearer {}", session_b.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let list_b: Vec<serde_json::Value> = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(list_b.len(), 0);
}

#[tokio::test]
async fn mutations_create_audit_events() {
    let db = setup_test_db().await;
    let auth = AuthService::new(db.clone());
    let (service, source, base_img) = test_service();
    let app = router_with_auth(service, Some(auth.clone()), Some(db.clone()));

    let user = auth
        .register(
            &format!("audit-user-{}@example.com", Uuid::new_v4().simple()),
            "Password123!",
            "Audit User",
        )
        .await
        .unwrap();
    let session = auth
        .create_session(user.id, time::Duration::days(1))
        .await
        .unwrap();

    // 1. Create project
    let req_body = serde_json::json!({
        "name": "audit-site",
        "source_dir": source,
        "base_image": base_img,
    });
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/projects")
                .header(header::AUTHORIZATION, format!("Bearer {}", session.token))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(req_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let proj: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let proj_id = proj["id"].as_str().unwrap();

    // 2. Deploy project
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/projects/{proj_id}/deployments"))
                .header(header::AUTHORIZATION, format!("Bearer {}", session.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let dep: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    let dep_id = dep["id"].as_str().unwrap();

    // 3. Stop deployment
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/deployments/{dep_id}/stop"))
                .header(header::AUTHORIZATION, format!("Bearer {}", session.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Verify audit events
    let mut audits = db.audits();
    let events = audits.list_by_user(user.id).await.unwrap();
    let actions: Vec<String> = events.iter().map(|e| e.action.clone()).collect();

    assert!(actions.iter().any(|a| a == "project.create"));
    assert!(actions.iter().any(|a| a == "deployment.create"));
    assert!(actions.iter().any(|a| a == "deployment.stop"));
}
