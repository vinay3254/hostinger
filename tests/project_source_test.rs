use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use deploy_platform::{
    auth::{hash_password, AuthService},
    db::Database,
    providers::Provider,
    repository::UpsertConnectionInput,
};
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

struct DummyService {
    project: deploy_platform::model::Project,
}

impl deploy_platform::service::PlatformService for DummyService {
    fn create_project(
        &self,
        _: deploy_platform::model::CreateProjectInput,
    ) -> deploy_platform::Result<deploy_platform::model::Project> {
        unimplemented!()
    }
    fn project(&self, id: Uuid) -> deploy_platform::Result<deploy_platform::model::Project> {
        if id == self.project.id {
            Ok(self.project.clone())
        } else {
            anyhow::bail!("project not found: {id}")
        }
    }
    fn projects(&self) -> deploy_platform::Result<Vec<deploy_platform::model::Project>> {
        Ok(vec![self.project.clone()])
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

#[tokio::test]
async fn project_source_settings_lifecycle() {
    let db = setup_test_db().await;
    let auth = AuthService::new(db.clone());

    let user_id = Uuid::new_v4();
    let email = format!("src-user-{}@example.com", Uuid::new_v4().simple());
    let pw_hash = hash_password("password").unwrap();
    db.users()
        .create_user(
            user_id,
            &email,
            &pw_hash,
            "Source User",
            OffsetDateTime::now_utc(),
        )
        .await
        .unwrap();

    let session = auth
        .create_session(user_id, time::Duration::hours(1))
        .await
        .unwrap();

    let project_id = Uuid::new_v4();
    let now = OffsetDateTime::now_utc();
    let project = deploy_platform::model::Project {
        id: project_id,
        name: "test-src-proj".into(),
        user_id: Some(user_id),
        source_dir: std::path::PathBuf::from("/tmp/test"),
        base_image: std::path::PathBuf::from("/tmp/base.tar.gz"),
        server_command: vec!["/bin/sh".into()],
        created_at: now,
        active_deployment: None,
    };

    // Insert project in DB
    sqlx::query(
        "INSERT INTO projects (id, user_id, name, source_dir, base_image, server_command, created_at) VALUES ($1, $2, $3, $4, $5, $6, $7)"
    )
    .bind(project_id)
    .bind(user_id)
    .bind(&project.name)
    .bind("/tmp/test")
    .bind("/tmp/base.tar.gz")
    .bind(&["/bin/sh".to_string()])
    .bind(now)
    .execute(db.pool())
    .await
    .unwrap();

    // Create provider connection & repository in DB
    let conn_id = Uuid::new_v4();
    db.providers()
        .upsert_connection(UpsertConnectionInput {
            id: conn_id,
            user_id,
            provider: Provider::GitHub,
            external_user_id: "gh-user-123".into(),
            access_token_encrypted: "enc_tok".into(),
            refresh_token_encrypted: None,
            expires_at: None,
            created_at: now,
            updated_at: now,
        })
        .await
        .unwrap();

    let repo = deploy_platform::providers::RemoteRepository {
        external_id: "998877".into(),
        full_name: "my-org/my-repo".into(),
        clone_url: "https://github.com/my-org/my-repo.git".parse().unwrap(),
        default_branch: "main".into(),
    };
    db.providers()
        .save_repositories(conn_id, &[repo], now)
        .await
        .unwrap();

    // Fetch the actual saved repo id from DB
    let repos = db
        .providers()
        .list_repositories(user_id, Provider::GitHub)
        .await
        .unwrap();
    let saved_repo = repos.first().unwrap();

    let service = Arc::new(DummyService {
        project: project.clone(),
    });
    let app = deploy_platform::api::router_with_auth(service, Some(auth), Some(db.clone()));

    // 1. GET /v1/projects/:id/source initially has no repository
    let req = Request::builder()
        .method("GET")
        .uri(format!("/v1/projects/{project_id}/source"))
        .header(header::AUTHORIZATION, format!("Bearer {}", session.token))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let info: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(info["repository"].is_null());
    assert_eq!(info["has_webhook_secret"], false);

    // 2. POST /v1/projects/:id/source connects the repository
    let connect_payload = serde_json::json!({
        "repository_id": saved_repo.id,
        "target_branch": "develop"
    });
    let req = Request::builder()
        .method("POST")
        .uri(format!("/v1/projects/{project_id}/source"))
        .header(header::AUTHORIZATION, format!("Bearer {}", session.token))
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(connect_payload.to_string()))
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let updated: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(updated["repository"]["id"], saved_repo.id.to_string());
    assert_eq!(updated["target_branch"], "develop");
    assert_eq!(updated["provider"], "github");
    assert_eq!(updated["has_webhook_secret"], true);
    assert_eq!(
        updated["webhook_url"],
        format!("/v1/webhooks/github/{project_id}")
    );

    // 3. GET /v1/projects/:id/source/events returns events list
    let req = Request::builder()
        .method("GET")
        .uri(format!("/v1/projects/{project_id}/source/events"))
        .header(header::AUTHORIZATION, format!("Bearer {}", session.token))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let events: Vec<serde_json::Value> = serde_json::from_slice(&body).unwrap();
    assert_eq!(events.len(), 0);

    // 4. DELETE /v1/projects/:id/source disconnects
    let req = Request::builder()
        .method("DELETE")
        .uri(format!("/v1/projects/{project_id}/source"))
        .header(header::AUTHORIZATION, format!("Bearer {}", session.token))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // 5. Verify disconnected
    let req = Request::builder()
        .method("GET")
        .uri(format!("/v1/projects/{project_id}/source"))
        .header(header::AUTHORIZATION, format!("Bearer {}", session.token))
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    let info_after: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert!(info_after["repository"].is_null());
}
