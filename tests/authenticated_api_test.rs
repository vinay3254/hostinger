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

async fn setup_app() -> (axum::Router, Database, StateStore, tempfile::TempDir) {
    let db = setup_test_db().await;
    let auth = AuthService::new(db.clone());
    let (service, _source, _base_img) = test_service();
    let app = router_with_auth(service, Some(auth), Some(db.clone()));
    let temp = tempdir().unwrap();
    let store = StateStore::at(temp.path().to_path_buf());
    (app, db, store, temp)
}

async fn create_test_user(db: &Database, email: &str) -> deploy_platform::repository::UserRecord {
    let auth = AuthService::new(db.clone());
    auth.register(email, "Password123!", "Test User")
        .await
        .unwrap()
}

async fn create_test_session(db: &Database, user_id: Uuid) -> deploy_platform::auth::SessionInfo {
    let auth = AuthService::new(db.clone());
    auth.create_session(user_id, time::Duration::days(1))
        .await
        .unwrap()
}

async fn create_test_project(db: &Database, user_id: Uuid, name: &str) -> Uuid {
    let mut repo = db.projects();
    let project = repo
        .create_project(deploy_platform::repository::CreateProjectRecord {
            id: Uuid::new_v4(),
            user_id,
            name: name.to_string(),
            source_dir: PathBuf::from("/tmp"),
            base_image: PathBuf::from("/tmp"),
            server_command: vec!["echo".into()],
            created_at: time::OffsetDateTime::now_utc(),
        })
        .await
        .unwrap();
    project.id
}

#[tokio::test]
async fn preview_api_endpoints_and_permissions() {
    let (app, db, _store, _temp) = setup_app().await;

    // Register User A and User B
    let user_a = create_test_user(
        &db,
        &format!("user-a-{}-pr@example.com", Uuid::new_v4().simple()),
    )
    .await;
    let session_a = create_test_session(&db, user_a.id).await;

    let user_b = create_test_user(
        &db,
        &format!("user-b-{}-pr@example.com", Uuid::new_v4().simple()),
    )
    .await;
    let session_b = create_test_session(&db, user_b.id).await;

    let proj_a_id = create_test_project(
        &db,
        user_a.id,
        &format!("project-preview-{}", Uuid::new_v4().simple()),
    )
    .await;

    // 1. Unauthenticated list previews -> 401
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/projects/{proj_a_id}/previews"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // 2. User B (unauthorized) access to User A's project previews -> 403
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/projects/{proj_a_id}/previews"))
                .header(header::AUTHORIZATION, format!("Bearer {}", session_b.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // 3. User A lists empty previews -> 200 OK
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/projects/{proj_a_id}/previews"))
                .header(header::AUTHORIZATION, format!("Bearer {}", session_a.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let previews: Vec<serde_json::Value> = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(previews.len(), 0);

    // 4. Create a preview in DB for Project A
    let mut repo = deploy_platform::previews::PreviewRepository::new(db.pool().into());
    let input = deploy_platform::previews::UpsertPreviewInput {
        project_id: proj_a_id,
        provider: "github".to_string(),
        pr_number: 77,
        head_sha: "rc-77-sha".to_string(),
        base_branch: "main".to_string(),
        head_branch: "feature/preview".to_string(),
        deployment_id: None,
        hostname: "project-preview-a-pr-77.preview.local".to_string(),
        status: deploy_platform::previews::PreviewStatus::Ready,
    };
    let preview = repo.upsert(&input).await.unwrap();

    // 5. User A lists previews -> 200 OK, finds preview
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/projects/{proj_a_id}/previews"))
                .header(header::AUTHORIZATION, format!("Bearer {}", session_a.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let previews: Vec<serde_json::Value> = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(previews.len(), 1);
    assert_eq!(previews[0]["pr_number"], 77);

    // 6. User A gets preview detail -> 200 OK
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/previews/{}", preview.id))
                .header(header::AUTHORIZATION, format!("Bearer {}", session_a.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let p_detail: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(p_detail["id"], preview.id.to_string());

    // 7. User B gets preview detail -> 403 Forbidden
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri(format!("/v1/previews/{}", preview.id))
                .header(header::AUTHORIZATION, format!("Bearer {}", session_b.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // 8. User A promotes preview to production -> 202 Accepted
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/previews/{}/promote", preview.id))
                .header(header::AUTHORIZATION, format!("Bearer {}", session_a.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::ACCEPTED);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let prod_dep: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(prod_dep["commit_sha"], "rc-77-sha");

    // 9. User A stops preview -> 200 OK
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/previews/{}/stop", preview.id))
                .header(header::AUTHORIZATION, format!("Bearer {}", session_a.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let stopped: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(stopped["status"], "closed");

    // 10. Promoting a closed preview returns 400 Bad Request
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/previews/{}/promote", preview.id))
                .header(header::AUTHORIZATION, format!("Bearer {}", session_a.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn project_cache_clear_api_test() {
    let (app, db, _store, _temp) = setup_app().await;

    // Register User A and User B
    let user_a = create_test_user(
        &db,
        &format!("user-a-{}-cache@example.com", Uuid::new_v4().simple()),
    )
    .await;
    let session_a = create_test_session(&db, user_a.id).await;

    let user_b = create_test_user(
        &db,
        &format!("user-b-{}-cache@example.com", Uuid::new_v4().simple()),
    )
    .await;
    let session_b = create_test_session(&db, user_b.id).await;

    // Create Project for User A
    let project_id = create_test_project(
        &db,
        user_a.id,
        &format!("project-cache-{}", Uuid::new_v4().simple()),
    )
    .await;

    // Seed a valid cache entry for this project
    let mut cache_repo = db.cache();
    let entry = cache_repo
        .store(&deploy_platform::cache::StoreCacheEntryInput {
            cache_key: "test-cache-key-1".to_string(),
            project_id,
            artifact_checksum: "a".repeat(64),
            size_bytes: 1234,
            storage_path: "/tmp/fake".to_string(),
            toolchain: "default".to_string(),
        })
        .await
        .unwrap();
    assert!(!entry.is_invalidated);

    // 1. Unauthenticated -> 401 Unauthorized
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/projects/{}/cache/clear", project_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // 2. User B (different owner) -> 403 Forbidden
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/projects/{}/cache/clear", project_id))
                .header(header::AUTHORIZATION, format!("Bearer {}", session_b.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // 3. User A (owner) -> 200 OK
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/projects/{}/cache/clear", project_id))
                .header(header::AUTHORIZATION, format!("Bearer {}", session_a.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // 4. Verify cache entry is now invalidated
    let valid_cache = cache_repo
        .get_valid(project_id, "test-cache-key-1")
        .await
        .unwrap();
    assert!(valid_cache.is_none());

    // 5. Verify audit log entry
    let mut audits = db.audits();
    let events = audits.list_by_user(user_a.id).await.unwrap();
    assert!(events.iter().any(|e| e.action == "project.cache.clear"));
}

#[tokio::test]
async fn deployment_logs_endpoints_and_permissions() {
    let (app, db, _store, _temp) = setup_app().await;

    let user_a = create_test_user(
        &db,
        &format!("user-a-{}-logs@example.com", Uuid::new_v4().simple()),
    )
    .await;
    let session_a = create_test_session(&db, user_a.id).await;

    let user_b = create_test_user(
        &db,
        &format!("user-b-{}-logs@example.com", Uuid::new_v4().simple()),
    )
    .await;
    let session_b = create_test_session(&db, user_b.id).await;

    let project_id = create_test_project(
        &db,
        user_a.id,
        &format!("project-logs-{}", Uuid::new_v4().simple()),
    )
    .await;

    let deployment_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO deployments (id, project_id, framework, status, created_at)
         VALUES ($1, $2, 'static', 'running', NOW())",
    )
    .bind(deployment_id)
    .bind(project_id)
    .execute(db.pool())
    .await
    .unwrap();

    // Append some log lines
    let mut logs_repo = db.logs();
    logs_repo
        .append(
            deployment_id,
            project_id,
            1,
            "stdout",
            "Starting application...",
            time::OffsetDateTime::now_utc(),
        )
        .await
        .unwrap();
    logs_repo
        .append(
            deployment_id,
            project_id,
            2,
            "stderr",
            "Warning: check config",
            time::OffsetDateTime::now_utc(),
        )
        .await
        .unwrap();

    // 1. Unauthenticated -> 401
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/deployments/{}/logs", deployment_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // 2. User B (not owner) -> 403
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/deployments/{}/logs", deployment_id))
                .header(header::AUTHORIZATION, format!("Bearer {}", session_b.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // 3. User A (owner) -> 200 OK
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/deployments/{}/logs", deployment_id))
                .header(header::AUTHORIZATION, format!("Bearer {}", session_a.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let logs_resp: deploy_platform::logs::LogsResponse = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(logs_resp.lines.len(), 2);
    assert_eq!(logs_resp.lines[0].message, "Starting application...");
    assert_eq!(logs_resp.lines[1].stream, "stderr");

    // 4. Download logs -> 200 OK text/plain
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/deployments/{}/logs/download", deployment_id))
                .header(header::AUTHORIZATION, format!("Bearer {}", session_a.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let content_type = res.headers().get("content-type").unwrap().to_str().unwrap();
    assert!(content_type.contains("text/plain"));
    let raw_logs = res.into_body().collect().await.unwrap().to_bytes();
    let log_text = String::from_utf8(raw_logs.to_vec()).unwrap();
    assert!(log_text.contains("Starting application..."));
    assert!(log_text.contains("Warning: check config"));
}

#[tokio::test]
async fn releases_and_rollback_endpoints_and_permissions() {
    let (app, db, _store, _temp) = setup_app().await;

    let user_a = create_test_user(
        &db,
        &format!("user-a-{}-rel@example.com", Uuid::new_v4().simple()),
    )
    .await;
    let session_a = create_test_session(&db, user_a.id).await;

    let user_b = create_test_user(
        &db,
        &format!("user-b-{}-rel@example.com", Uuid::new_v4().simple()),
    )
    .await;
    let session_b = create_test_session(&db, user_b.id).await;

    let project_id = create_test_project(
        &db,
        user_a.id,
        &format!("project-rel-{}", Uuid::new_v4().simple()),
    )
    .await;

    // Create deployment 1 (healthy predecessor)
    let dep1_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO deployments (id, project_id, framework, status, image_path, commit_sha, target, created_at)
         VALUES ($1, $2, 'static', 'stopped', '/img/v1.tar', 'commit-v1-stable', 'production', NOW())",
    )
    .bind(dep1_id)
    .bind(project_id)
    .execute(db.pool())
    .await
    .unwrap();

    let mut releases_ctrl = db.releases();
    let rel1 = releases_ctrl
        .create_release(
            dep1_id,
            project_id,
            "production",
            Some("c1".into()),
            Some(8081),
            Some("http://127.0.0.1:8081".into()),
        )
        .await
        .unwrap();
    let rel1 = releases_ctrl
        .transition(
            rel1.id,
            rel1.version,
            deploy_platform::releases::ReleaseStatus::HealthChecking,
            None,
        )
        .await
        .unwrap();
    let rel1 = releases_ctrl
        .transition(
            rel1.id,
            rel1.version,
            deploy_platform::releases::ReleaseStatus::Ready,
            None,
        )
        .await
        .unwrap();
    let rel1 = releases_ctrl
        .transition(
            rel1.id,
            rel1.version,
            deploy_platform::releases::ReleaseStatus::Active,
            None,
        )
        .await
        .unwrap();
    let rel1 = releases_ctrl
        .transition(
            rel1.id,
            rel1.version,
            deploy_platform::releases::ReleaseStatus::Stopped,
            None,
        )
        .await
        .unwrap();

    // Create deployment 2 (current running deployment)
    let dep2_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO deployments (id, project_id, framework, status, image_path, commit_sha, target, created_at)
         VALUES ($1, $2, 'static', 'running', '/img/v2.tar', 'commit-v2-broken', 'production', NOW())",
    )
    .bind(dep2_id)
    .bind(project_id)
    .execute(db.pool())
    .await
    .unwrap();

    let rel2 = releases_ctrl
        .create_release(
            dep2_id,
            project_id,
            "production",
            Some("c2".into()),
            Some(8082),
            Some("http://127.0.0.1:8082".into()),
        )
        .await
        .unwrap();

    // 1. GET /v1/deployments/:id/releases - unauthenticated -> 401
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/deployments/{}/releases", dep2_id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // 2. GET /v1/deployments/:id/releases - non-owner (user B) -> 403
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/deployments/{}/releases", dep2_id))
                .header(header::AUTHORIZATION, format!("Bearer {}", session_b.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // 3. GET /v1/deployments/:id/releases - owner (user A) -> 200 OK
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/deployments/{}/releases", dep2_id))
                .header(header::AUTHORIZATION, format!("Bearer {}", session_a.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let releases: Vec<deploy_platform::releases::Release> = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(releases.len(), 1);
    assert_eq!(releases[0].id, rel2.id);

    // 4. GET /v1/releases/:id/events - owner (user A) -> 200 OK
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/releases/{}/events", rel1.id))
                .header(header::AUTHORIZATION, format!("Bearer {}", session_a.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let events: Vec<deploy_platform::releases::ReleaseTransitionRecord> =
        serde_json::from_slice(&bytes).unwrap();
    assert_eq!(events.len(), 4);

    // 5. POST /v1/deployments/:id/rollback - limited token (only project:read) -> 403
    let auth_svc = AuthService::new(db.clone());
    let limited_token = auth_svc
        .issue_api_token(
            user_a.id,
            "read-only-token",
            vec!["project:read".into()],
            None,
        )
        .await
        .unwrap();

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/deployments/{}/rollback", dep2_id))
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", limited_token.raw_token),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);

    // 6. POST /v1/deployments/:id/rollback - full operator (session A with *) -> 201 Created
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/deployments/{}/rollback", dep2_id))
                .header(header::AUTHORIZATION, format!("Bearer {}", session_a.token))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let rollback_resp: deploy_platform::rollback::RollbackResult =
        serde_json::from_slice(&bytes).unwrap();
    assert_eq!(rollback_resp.target.deployment_id, dep1_id);
    assert_eq!(
        rollback_resp.target.commit_sha.as_deref(),
        Some("commit-v1-stable")
    );
}
