use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use deploy_platform::{auth::hash_password, db::Database, providers::SourceEventKind};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::sync::Arc;
use time::OffsetDateTime;
use tower::ServiceExt;
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

struct DummyService;
impl deploy_platform::service::PlatformService for DummyService {
    fn create_project(
        &self,
        _: deploy_platform::model::CreateProjectInput,
    ) -> deploy_platform::Result<deploy_platform::model::Project> {
        unimplemented!()
    }
    fn project(&self, _: Uuid) -> deploy_platform::Result<deploy_platform::model::Project> {
        unimplemented!()
    }
    fn projects(&self) -> deploy_platform::Result<Vec<deploy_platform::model::Project>> {
        unimplemented!()
    }
    fn deploy(&self, _: Uuid) -> deploy_platform::Result<deploy_platform::model::Deployment> {
        unimplemented!()
    }
    fn deployments(
        &self,
        _: Uuid,
    ) -> deploy_platform::Result<Vec<deploy_platform::model::Deployment>> {
        unimplemented!()
    }
    fn deployment(&self, _: Uuid) -> deploy_platform::Result<deploy_platform::model::Deployment> {
        unimplemented!()
    }
    fn stop(&self, _: Uuid) -> deploy_platform::Result<deploy_platform::model::Deployment> {
        unimplemented!()
    }
    fn logs(&self, _: Uuid) -> deploy_platform::Result<String> {
        unimplemented!()
    }
}

fn compute_github_signature(secret: &str, body: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(body);
    let result = mac.finalize().into_bytes();
    format!("sha256={}", hex::encode(result))
}

#[tokio::test]
async fn webhook_verification_and_normalization_flow() {
    let db = setup_test_db().await;
    let service = Arc::new(DummyService);
    let app = deploy_platform::api::router_with_auth(service, None, Some(db.clone()));

    // 1. Create a user and a project with a webhook_secret
    let user_id = Uuid::new_v4();
    let email = format!("wh-user-{}@example.com", Uuid::new_v4().simple());
    let pw_hash = hash_password("password").unwrap();
    db.users()
        .create_user(
            user_id,
            &email,
            &pw_hash,
            "Webhook User",
            OffsetDateTime::now_utc(),
        )
        .await
        .unwrap();

    let project_id = Uuid::new_v4();
    let webhook_secret = "whsec_super_secret_signing_key_42";
    let now = OffsetDateTime::now_utc();

    // Insert project directly with webhook_secret
    sqlx::query(
        "INSERT INTO projects (id, user_id, name, source_dir, base_image, server_command, webhook_secret, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)"
    )
    .bind(project_id)
    .bind(user_id)
    .bind(format!("proj-{}", Uuid::new_v4().simple()))
    .bind("/tmp/source")
    .bind("/tmp/base.tar.gz")
    .bind(&["/bin/busybox".to_string(), "httpd".to_string(), "-p".to_string(), "{PORT}".to_string()])
    .bind(webhook_secret)
    .bind(now)
    .execute(db.pool())
    .await
    .unwrap();

    let payload = serde_json::json!({
        "ref": "refs/heads/main",
        "after": "aabbccddeeff11223344556677889900aabbccdd",
        "repository": {
            "id": 998877,
            "clone_url": "https://github.com/my-org/my-project.git",
            "default_branch": "main"
        }
    });
    let payload_bytes = payload.to_string().into_bytes();
    let delivery_id = format!("del-{}", Uuid::new_v4());

    // 2. Missing signature returns 401
    let req = Request::builder()
        .method("POST")
        .uri(format!("/v1/webhooks/github/{project_id}"))
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-github-event", "push")
        .header("x-github-delivery", &delivery_id)
        .body(Body::from(payload_bytes.clone()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 3. Invalid signature returns 401
    let req = Request::builder()
        .method("POST")
        .uri(format!("/v1/webhooks/github/{project_id}"))
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-github-event", "push")
        .header("x-github-delivery", &delivery_id)
        .header(
            "x-hub-signature-256",
            "sha256=invalid0000000000000000000000000000000000000000000000000000000000",
        )
        .body(Body::from(payload_bytes.clone()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 4. Valid signature returns 200 OK and persists source event
    let valid_sig = compute_github_signature(webhook_secret, &payload_bytes);
    let req = Request::builder()
        .method("POST")
        .uri(format!("/v1/webhooks/github/{project_id}"))
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-github-event", "push")
        .header("x-github-delivery", &delivery_id)
        .header("x-hub-signature-256", &valid_sig)
        .body(Body::from(payload_bytes.clone()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Verify source event was saved
    let events = db
        .source_events()
        .list_by_project(project_id)
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].delivery_id, delivery_id);
    assert_eq!(events[0].kind, SourceEventKind::Push);
    assert_eq!(
        events[0].commit_sha,
        "aabbccddeeff11223344556677889900aabbccdd"
    );
    assert_eq!(events[0].branch.as_deref(), Some("main"));

    // 5. Duplicate delivery ID returns 202 ACCEPTED and does not duplicate event
    let req = Request::builder()
        .method("POST")
        .uri(format!("/v1/webhooks/github/{project_id}"))
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-github-event", "push")
        .header("x-github-delivery", &delivery_id)
        .header("x-hub-signature-256", &valid_sig)
        .body(Body::from(payload_bytes.clone()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    let events_after = db
        .source_events()
        .list_by_project(project_id)
        .await
        .unwrap();
    assert_eq!(events_after.len(), 1);
}
