use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use deploy_platform::{
    agent_protocol::{NodeCapacity, RegisterNodeCommand},
    api::router_with_auth,
    auth::AuthService,
    builder::{BuildOutput, ImageBuilder},
    db::Database,
    detector::StaticSource,
    model::RunningContainer,
    node_registry::NodeRegistry,
    runtime::Runtime,
    scheduler::{PlacementRequest, Scheduler},
    service::{DeploymentService, PlatformService},
    store::StateStore,
    Result,
};
use http_body_util::BodyExt;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
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
struct FakeRuntime;

impl Runtime for FakeRuntime {
    fn start_static(&mut self, _: &Path, _: &str, _: &[String]) -> Result<RunningContainer> {
        Ok(RunningContainer {
            id: Uuid::new_v4(),
            port: 43123,
            url: "http://127.0.0.1:43123".into(),
        })
    }

    fn stop(&mut self, _: Uuid) -> Result<()> {
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
        image_path: fake_img,
    };
    let runtime = FakeRuntime;
    Arc::new(DeploymentService::new(
        StateStore::at(root),
        runtime,
        builder,
    ))
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
async fn test_unauthenticated_and_forbidden_node_endpoints() {
    let db = setup_test_db().await;
    let auth = AuthService::new(db.clone());
    let service = test_service();
    let app = router_with_auth(service, Some(auth.clone()), Some(db.clone()));

    // 1. Unauthenticated request to /v1/nodes returns 401
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/nodes")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);

    // 2. User with only "project:read" scope gets 403 Forbidden
    let user = auth
        .register(
            &format!("user-nopriv-{}@example.com", Uuid::new_v4().simple()),
            "Password123!",
            "Regular User",
        )
        .await
        .unwrap();

    let non_admin_token = auth
        .issue_api_token(
            user.id,
            "read-only-token",
            vec!["project:read".to_string()],
            None,
        )
        .await
        .unwrap();

    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/nodes")
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", non_admin_token.raw_token),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn test_list_and_get_node_endpoints() {
    let db = setup_test_db().await;
    let auth = AuthService::new(db.clone());
    let service = test_service();
    let app = router_with_auth(service, Some(auth.clone()), Some(db.clone()));

    let user = auth
        .register(
            &format!("admin-{}@example.com", Uuid::new_v4().simple()),
            "Password123!",
            "Admin User",
        )
        .await
        .unwrap();

    let admin_session = auth
        .create_session(user.id, time::Duration::days(1))
        .await
        .unwrap();

    // Register two nodes
    let registry = NodeRegistry::new();
    let node1_id = Uuid::new_v4();
    registry
        .register_node(
            db.pool(),
            &RegisterNodeCommand {
                node_id: node1_id,
                hostname: format!("worker-node-1-{}", Uuid::new_v4().simple()),
                endpoint: "http://10.0.0.1:50051".into(),
                capacity: NodeCapacity {
                    cpu_millicores: 4000,
                    memory_bytes: 8 * 1024 * 1024 * 1024,
                    max_releases: 20,
                },
                token_hash: "hash1".into(),
            },
        )
        .await
        .unwrap();

    let node2_id = Uuid::new_v4();
    registry
        .register_node(
            db.pool(),
            &RegisterNodeCommand {
                node_id: node2_id,
                hostname: format!("worker-node-2-{}", Uuid::new_v4().simple()),
                endpoint: "http://10.0.0.2:50051".into(),
                capacity: NodeCapacity {
                    cpu_millicores: 8000,
                    memory_bytes: 16 * 1024 * 1024 * 1024,
                    max_releases: 40,
                },
                token_hash: "hash2".into(),
            },
        )
        .await
        .unwrap();

    // List nodes
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/nodes")
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", admin_session.token),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let body = res.into_body().collect().await.unwrap().to_bytes();
    let nodes: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert!(nodes.iter().any(|n| n["id"] == node1_id.to_string()));
    assert!(nodes.iter().any(|n| n["id"] == node2_id.to_string()));

    // Get specific node
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/nodes/{}", node1_id))
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", admin_session.token),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let body = res.into_body().collect().await.unwrap().to_bytes();
    let detail: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(detail["node"]["id"], node1_id.to_string());
    assert!(detail["placements"].is_array());

    // Non-existent node returns 404
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/nodes/{}", Uuid::new_v4()))
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", admin_session.token),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_drain_and_enable_node_endpoints() {
    let db = setup_test_db().await;
    let auth = AuthService::new(db.clone());
    let service = test_service();
    let app = router_with_auth(service, Some(auth.clone()), Some(db.clone()));

    let user = auth
        .register(
            &format!("admin-control-{}@example.com", Uuid::new_v4().simple()),
            "Password123!",
            "Admin User",
        )
        .await
        .unwrap();

    let admin_session = auth
        .create_session(user.id, time::Duration::days(1))
        .await
        .unwrap();

    // Register node
    let registry = NodeRegistry::new();
    let node_id = Uuid::new_v4();
    registry
        .register_node(
            db.pool(),
            &RegisterNodeCommand {
                node_id,
                hostname: format!("drainable-node-{}", Uuid::new_v4().simple()),
                endpoint: "http://10.0.0.5:50051".into(),
                capacity: NodeCapacity {
                    cpu_millicores: 4000,
                    memory_bytes: 8 * 1024 * 1024 * 1024,
                    max_releases: 20,
                },
                token_hash: "hashdrain".into(),
            },
        )
        .await
        .unwrap();

    // Drain node
    let drain_req = serde_json::json!({
        "draining": true,
        "reason": "Routine kernel upgrade"
    });
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/nodes/{}/drain", node_id))
                .header(header::CONTENT_TYPE, "application/json")
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", admin_session.token),
                )
                .body(Body::from(drain_req.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let body = res.into_body().collect().await.unwrap().to_bytes();
    let updated: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(updated["is_draining"], true);
    assert_eq!(updated["status"], "draining");

    // Undrain node
    let undrain_req = serde_json::json!({
        "draining": false
    });
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/nodes/{}/drain", node_id))
                .header(header::CONTENT_TYPE, "application/json")
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", admin_session.token),
                )
                .body(Body::from(undrain_req.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let body = res.into_body().collect().await.unwrap().to_bytes();
    let updated: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(updated["is_draining"], false);
    assert_eq!(updated["status"], "online");

    // Disable node
    let disable_req = serde_json::json!({
        "enabled": false,
        "reason": "Hardware fault suspected"
    });
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/nodes/{}/enable", node_id))
                .header(header::CONTENT_TYPE, "application/json")
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", admin_session.token),
                )
                .body(Body::from(disable_req.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let body = res.into_body().collect().await.unwrap().to_bytes();
    let updated: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(updated["is_enabled"], false);

    // Re-enable node
    let enable_req = serde_json::json!({
        "enabled": true
    });
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/nodes/{}/enable", node_id))
                .header(header::CONTENT_TYPE, "application/json")
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", admin_session.token),
                )
                .body(Body::from(enable_req.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let body = res.into_body().collect().await.unwrap().to_bytes();
    let updated: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(updated["is_enabled"], true);
}

#[tokio::test]
async fn test_node_detail_includes_scheduled_placement() {
    let db = setup_test_db().await;
    let auth = AuthService::new(db.clone());
    let service = test_service();
    let app = router_with_auth(service, Some(auth.clone()), Some(db.clone()));

    let user = auth
        .register(
            &format!("admin-placement-{}@example.com", Uuid::new_v4().simple()),
            "Password123!",
            "Admin User",
        )
        .await
        .unwrap();

    let admin_session = auth
        .create_session(user.id, time::Duration::days(1))
        .await
        .unwrap();

    let registry = NodeRegistry::new();
    let node_id = Uuid::new_v4();
    registry
        .register_node(
            db.pool(),
            &RegisterNodeCommand {
                node_id,
                hostname: format!("placement-node-{}", Uuid::new_v4().simple()),
                endpoint: "http://10.0.0.9:50051".into(),
                capacity: NodeCapacity {
                    cpu_millicores: 4000,
                    memory_bytes: 8 * 1024 * 1024 * 1024,
                    max_releases: 20,
                },
                token_hash: "hashplace".into(),
            },
        )
        .await
        .unwrap();

    let scheduler = Scheduler::new();
    let release_id = Uuid::new_v4();
    let placement_req = PlacementRequest {
        release_id,
        deployment_id: Uuid::new_v4(),
        project_id: Uuid::new_v4(),
        required_cpu_millicores: 500,
        required_memory_bytes: 512 * 1024 * 1024,
        candidate_nodes: Some(HashSet::from([node_id])),
        anti_affinity_nodes: HashSet::new(),
    };

    let decision = scheduler
        .schedule_release(db.pool(), &placement_req, Duration::from_secs(60))
        .await
        .unwrap();
    assert_eq!(decision.selected_node_id, node_id);

    // Fetch node detail via API
    let res = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/nodes/{}", node_id))
                .header(
                    header::AUTHORIZATION,
                    format!("Bearer {}", admin_session.token),
                )
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    let body = res.into_body().collect().await.unwrap().to_bytes();
    let detail: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(detail["node"]["id"], node_id.to_string());
    let placements = detail["placements"].as_array().unwrap();
    assert!(placements.iter().any(|p| p["release_id"] == release_id.to_string()));
}
