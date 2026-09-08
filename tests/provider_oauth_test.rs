use axum::{
    body::Body,
    http::{header, Request, StatusCode},
};
use deploy_platform::{
    auth::{hash_password, AuthService},
    db::Database,
    providers::{
        crypto::{decrypt_token, default_encryption_key},
        Provider, ProviderClient, RemoteRepository,
    },
};
use http_body_util::BodyExt;
use std::sync::Arc;
use time::OffsetDateTime;
use tower::ServiceExt;
use url::Url;
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

struct MockProviderClient;

#[async_trait::async_trait]
impl ProviderClient for MockProviderClient {
    fn provider(&self) -> Provider {
        Provider::GitHub
    }
    fn begin_authorization(&self, state: &str, redirect_uri: &str) -> String {
        format!(
            "https://github.com/login/oauth/authorize?state={state}&redirect_uri={redirect_uri}"
        )
    }
    async fn complete_authorization(
        &self,
        code: &str,
        _redirect_uri: &str,
    ) -> deploy_platform::Result<deploy_platform::providers::OAuthTokenResponse> {
        if code == "valid-auth-code" {
            Ok(deploy_platform::providers::OAuthTokenResponse {
                access_token: "mock-access-token-12345".into(),
                refresh_token: None,
                expires_in: None,
                external_user_id: "gh-user-999".into(),
            })
        } else {
            anyhow::bail!("invalid authorization code")
        }
    }
    async fn list_repositories(
        &self,
        access_token: &str,
    ) -> deploy_platform::Result<Vec<RemoteRepository>> {
        if access_token == "mock-access-token-12345" {
            Ok(vec![RemoteRepository {
                external_id: "repo-100".into(),
                full_name: "test-user/my-repo".into(),
                clone_url: Url::parse("https://github.com/test-user/my-repo.git").unwrap(),
                default_branch: "main".into(),
            }])
        } else {
            anyhow::bail!("unauthorized access token")
        }
    }
    async fn get_commit(
        &self,
        _access_token: &str,
        _repo_id: &str,
        _commit_sha: &str,
    ) -> deploy_platform::Result<deploy_platform::providers::RemoteCommit> {
        unimplemented!()
    }
    async fn create_webhook(
        &self,
        _access_token: &str,
        _repo_id: &str,
        _webhook_url: &str,
        _secret: &str,
    ) -> deploy_platform::Result<String> {
        unimplemented!()
    }
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

#[tokio::test]
async fn oauth_flow_csrf_check_callback_and_repository_listing() {
    let db = setup_test_db().await;
    let auth = AuthService::new(db.clone());
    let service = Arc::new(DummyService);
    let mock_client = Arc::new(MockProviderClient);

    let app = deploy_platform::api::router_with_providers(
        service,
        Some(auth.clone()),
        Some(db.clone()),
        vec![mock_client],
    );

    // 1. Create a user and session
    let user_id = Uuid::new_v4();
    let email = format!("oauth-user-{}@example.com", Uuid::new_v4().simple());
    let pw_hash = hash_password("password").unwrap();
    db.users()
        .create_user(
            user_id,
            &email,
            &pw_hash,
            "OAuth User",
            OffsetDateTime::now_utc(),
        )
        .await
        .unwrap();
    let session = auth
        .create_session(user_id, time::Duration::hours(1))
        .await
        .unwrap();
    let cookie_header = format!("dp_session={}", session.token);

    // 2. GET /v1/providers/github/connect returns redirect URL with state
    let req = Request::builder()
        .uri("/v1/providers/github/connect")
        .header(header::COOKIE, &cookie_header)
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let connect_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    let redirect_url = connect_json["url"].as_str().unwrap();
    assert!(redirect_url.contains("state="));

    // Extract state from URL
    let parsed_url = Url::parse(redirect_url).unwrap();
    let state = parsed_url
        .query_pairs()
        .find(|(k, _)| k == "state")
        .unwrap()
        .1
        .to_string();

    // 3. Callback with invalid / mismatched state fails with 400
    let invalid_callback_req = Request::builder()
        .uri("/v1/providers/github/callback?code=valid-auth-code&state=invalid-tampered-state")
        .header(header::COOKIE, &cookie_header)
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(invalid_callback_req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // 4. Callback with valid code and valid state succeeds
    let valid_callback_req = Request::builder()
        .uri(format!(
            "/v1/providers/github/callback?code=valid-auth-code&state={state}"
        ))
        .header(header::COOKIE, &cookie_header)
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(valid_callback_req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 5. Verify token is stored encrypted in database
    let conn = db
        .providers()
        .get_connection(user_id, Provider::GitHub)
        .await
        .unwrap()
        .expect("connection should exist");
    assert_ne!(conn.access_token_encrypted, "mock-access-token-12345");
    let key = default_encryption_key();
    let decrypted = decrypt_token(&conn.access_token_encrypted, &key).unwrap();
    assert_eq!(decrypted, "mock-access-token-12345");

    // 6. GET /v1/providers/github/repositories lists synced repositories
    let repos_req = Request::builder()
        .uri("/v1/providers/github/repositories")
        .header(header::COOKIE, &cookie_header)
        .body(Body::empty())
        .unwrap();
    let resp = app.clone().oneshot(repos_req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let repos_json: Vec<serde_json::Value> = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(repos_json.len(), 1);
    assert_eq!(repos_json[0]["full_name"], "test-user/my-repo");
}
