use deploy_platform::{
    auth::{hash_password, verify_password, AuthService},
    db::Database,
};
use time::{Duration, OffsetDateTime};
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
async fn password_hashing_and_verification() {
    let password = "SuperSecretPassword123!";
    let hash = hash_password(password).unwrap();
    assert_ne!(password, hash);
    assert!(verify_password(password, &hash).unwrap());
    assert!(!verify_password("wrong-password", &hash).unwrap());
}

#[tokio::test]
async fn session_creation_expiry_and_revocation() {
    let db = setup_test_db().await;
    let auth = AuthService::new(db.clone());

    // Create a user
    let user_id = Uuid::new_v4();
    let email = format!("auth-user-{}@example.com", Uuid::new_v4().simple());
    let pw_hash = hash_password("password").unwrap();
    db.users()
        .create_user(
            user_id,
            &email,
            &pw_hash,
            "Auth User",
            OffsetDateTime::now_utc(),
        )
        .await
        .unwrap();

    // Create session valid for 5 minutes
    let session = auth
        .create_session(user_id, Duration::minutes(5))
        .await
        .unwrap();
    assert!(!session.token.is_empty());

    // Verify session loads correctly
    let auth_ctx = auth.authenticate_session(&session.token).await.unwrap();
    assert_eq!(auth_ctx.user_id, user_id);
    assert_eq!(auth_ctx.session_id, Some(session.id));

    // Revoke session
    auth.revoke_session(session.id).await.unwrap();
    let revoked_res = auth.authenticate_session(&session.token).await;
    assert!(revoked_res.is_err());
}

#[tokio::test]
async fn api_token_lifecycle_one_time_raw_and_revocation() {
    let db = setup_test_db().await;
    let auth = AuthService::new(db.clone());

    let user_id = Uuid::new_v4();
    let email = format!("token-user-{}@example.com", Uuid::new_v4().simple());
    let pw_hash = hash_password("password").unwrap();
    db.users()
        .create_user(
            user_id,
            &email,
            &pw_hash,
            "Token User",
            OffsetDateTime::now_utc(),
        )
        .await
        .unwrap();

    // Issue API token with scopes
    let issued = auth
        .issue_api_token(
            user_id,
            "cli-token",
            vec!["deploy:write".into(), "project:read".into()],
            None,
        )
        .await
        .unwrap();

    assert!(!issued.raw_token.is_empty());
    assert!(!issued.prefix.is_empty());

    // Authenticate with the raw token
    let ctx = auth
        .authenticate_api_token(&issued.raw_token)
        .await
        .unwrap();
    assert_eq!(ctx.user_id, user_id);
    assert!(ctx.has_scope("deploy:write"));
    assert!(ctx.has_scope("project:read"));
    assert!(!ctx.has_scope("admin:all"));

    // Revoke token
    auth.revoke_api_token(user_id, issued.id).await.unwrap();
    let err = auth.authenticate_api_token(&issued.raw_token).await;
    assert!(err.is_err());
}

#[tokio::test]
async fn expired_session_is_rejected() {
    let db = setup_test_db().await;
    let auth = AuthService::new(db.clone());

    let user_id = Uuid::new_v4();
    let email = format!("exp-user-{}@example.com", Uuid::new_v4().simple());
    let pw_hash = hash_password("password").unwrap();
    db.users()
        .create_user(
            user_id,
            &email,
            &pw_hash,
            "Exp User",
            OffsetDateTime::now_utc(),
        )
        .await
        .unwrap();

    // Negative duration means already expired
    let session = auth
        .create_session(user_id, Duration::seconds(-10))
        .await
        .unwrap();

    let res = auth.authenticate_session(&session.token).await;
    assert!(res.is_err());
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
async fn auth_http_endpoints_session_and_tokens_flow() {
    use axum::{
        body::Body,
        http::{header, Request, StatusCode},
    };
    use deploy_platform::api::router_with_auth;
    use http_body_util::BodyExt;
    use std::sync::Arc;
    use tower::ServiceExt;

    let db = setup_test_db().await;
    let auth = AuthService::new(db.clone());
    let service = Arc::new(DummyService);
    let app = router_with_auth(service, Some(auth.clone()), Some(db.clone()));

    // 1. Unauthenticated /v1/me returns 401
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/me")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 2. Register / Create a user
    let email = format!("route-user-{}@example.com", Uuid::new_v4().simple());
    let password = "SuperSecretPassword123!";
    let reg_body = serde_json::json!({
        "email": email,
        "password": password,
        "name": "Route User"
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/auth/register")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(reg_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let set_cookie = resp
        .headers()
        .get(header::SET_COOKIE)
        .expect("cookie should be set")
        .to_str()
        .unwrap()
        .to_string();
    assert!(set_cookie.contains("dp_session="));

    // 3. Login with valid credentials
    let login_body = serde_json::json!({
        "email": email,
        "password": password
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/auth/session")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(login_body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let login_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    let session_token = login_json["token"].as_str().unwrap().to_string();
    assert!(!session_token.is_empty());

    // 4. GET /v1/me with session cookie
    let cookie_header = format!("dp_session={session_token}");
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/me")
                .header(header::COOKIE, &cookie_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let me_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(me_json["email"], email);

    // 5. GET /v1/me with Bearer token
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/me")
                .header(header::AUTHORIZATION, format!("Bearer {session_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // 6. POST /v1/me/api-tokens
    let token_req = serde_json::json!({
        "name": "cli-test-token",
        "scopes": ["deploy:write", "project:read"]
    });
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/me/api-tokens")
                .header(header::AUTHORIZATION, format!("Bearer {session_token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(token_req.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let token_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    let raw_token = token_json["raw_token"].as_str().unwrap().to_string();
    let token_id = token_json["id"].as_str().unwrap().to_string();
    assert!(raw_token.starts_with("dp_"));

    // 7. GET /v1/me/api-tokens lists the token without exposing raw_token
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/me/api-tokens")
                .header(header::AUTHORIZATION, format!("Bearer {session_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let tokens_list: Vec<serde_json::Value> = serde_json::from_slice(&body_bytes).unwrap();
    assert_eq!(tokens_list.len(), 1);
    assert_eq!(tokens_list[0]["name"], "cli-test-token");
    assert!(tokens_list[0].get("raw_token").is_none());

    // 8. GET /v1/me using the CLI API token
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/me")
                .header(header::AUTHORIZATION, format!("Bearer {raw_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body_bytes = resp.into_body().collect().await.unwrap().to_bytes();
    let me_token_json: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    let scopes = me_token_json["scopes"].as_array().unwrap();
    assert!(scopes.iter().any(|s| s == "deploy:write"));

    // 9. Revoke the API token
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/me/api-tokens/{token_id}"))
                .header(header::AUTHORIZATION, format!("Bearer {session_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // 10. Attempting to use the revoked API token returns 401
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/me")
                .header(header::AUTHORIZATION, format!("Bearer {raw_token}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // 11. DELETE /v1/auth/session revokes session and clears cookie
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/v1/auth/session")
                .header(header::COOKIE, &cookie_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // 12. Subsequent request with the revoked session returns 401
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/me")
                .header(header::COOKIE, &cookie_header)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
