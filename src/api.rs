use crate::auth::{
    ApiTokenSummary, AuditService, AuthContext, AuthService, IssuedToken, ProjectAccess,
};
use crate::config::PlatformConfig;
use crate::db::Database;
use crate::model::{CreateProjectInput, Deployment, Project};
use crate::service::PlatformService;
use axum::{
    extract::{FromRef, FromRequestParts, Path, State},
    http::{request::Parts, StatusCode},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
    Json, Router,
};
use axum_extra::extract::cookie::{Cookie, CookieJar, SameSite};
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone)]
pub struct ApiState {
    pub service: Arc<dyn PlatformService>,
    pub auth: Option<AuthService>,
    pub db: Option<Database>,
}

#[derive(serde::Deserialize)]
pub struct CreateProjectRequest {
    pub name: String,
    pub source_dir: PathBuf,
    pub base_image: PathBuf,
}

#[derive(serde::Serialize)]
pub struct ErrorResponse {
    pub error: String,
}

#[derive(serde::Serialize)]
pub struct LogsResponse {
    pub logs: String,
}

pub enum ApiError {
    BadRequest(String),
    Unauthorized(String),
    Forbidden(String),
    NotFound(String),
    UnprocessableEntity(String),
    #[allow(dead_code)]
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            Self::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            Self::Unauthorized(msg) => (StatusCode::UNAUTHORIZED, msg),
            Self::Forbidden(msg) => (StatusCode::FORBIDDEN, msg),
            Self::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            Self::UnprocessableEntity(msg) => (StatusCode::UNPROCESSABLE_ENTITY, msg),
            Self::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        };
        (status, Json(ErrorResponse { error: msg })).into_response()
    }
}

#[axum::async_trait]
impl<S> FromRequestParts<S> for AuthContext
where
    S: Send + Sync,
    ApiState: FromRef<S>,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let api_state = ApiState::from_ref(state);
        let auth_service = match &api_state.auth {
            Some(auth) => auth,
            None => {
                return Ok(AuthContext {
                    user_id: Uuid::nil(),
                    session_id: None,
                    scopes: vec!["*".into()],
                });
            }
        };

        if let Some(auth_val) = parts.headers.get(axum::http::header::AUTHORIZATION) {
            if let Ok(auth_str) = auth_val.to_str() {
                if let Some(token) = auth_str.strip_prefix("Bearer ") {
                    let token = token.trim();
                    if token.starts_with("dp_") {
                        return auth_service
                            .authenticate_api_token(token)
                            .await
                            .map_err(|e| {
                                ApiError::Unauthorized(format!("invalid api token: {e}"))
                            });
                    } else {
                        return auth_service
                            .authenticate_session(token)
                            .await
                            .map_err(|e| ApiError::Unauthorized(format!("invalid session: {e}")));
                    }
                }
            }
        }

        if let Some(cookie_val) = parts.headers.get(axum::http::header::COOKIE) {
            if let Ok(cookie_str) = cookie_val.to_str() {
                for c in cookie_str.split(';') {
                    let c = c.trim();
                    if let Some(token) = c.strip_prefix("dp_session=") {
                        return auth_service
                            .authenticate_session(token)
                            .await
                            .map_err(|e| ApiError::Unauthorized(format!("invalid session: {e}")));
                    }
                }
            }
        }

        Err(ApiError::Unauthorized("authentication required".into()))
    }
}

#[derive(serde::Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(serde::Deserialize)]
pub struct RegisterRequest {
    pub email: String,
    pub password: String,
    pub name: String,
}

#[derive(serde::Serialize)]
pub struct UserSummary {
    pub id: Uuid,
    pub email: String,
    pub name: String,
}

#[derive(serde::Serialize)]
pub struct SessionResponse {
    pub user: UserSummary,
    pub token: String,
    pub expires_at: String,
}

#[derive(serde::Serialize)]
pub struct MeResponse {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub scopes: Vec<String>,
}

#[derive(serde::Deserialize)]
pub struct CreateApiTokenRequest {
    pub name: String,
    pub scopes: Option<Vec<String>>,
}

fn parse_id(param: &str) -> Result<Uuid, ApiError> {
    Uuid::parse_str(param).map_err(|e| ApiError::BadRequest(format!("invalid UUID: {e}")))
}

fn map_service_err(e: anyhow::Error) -> ApiError {
    let msg = format!("{e:#}");
    if msg.contains("not found") {
        ApiError::NotFound(msg)
    } else {
        ApiError::UnprocessableEntity(msg)
    }
}

async fn healthz() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn list_projects(
    State(state): State<ApiState>,
    auth_ctx: AuthContext,
) -> Result<Json<Vec<Project>>, ApiError> {
    let all = state.service.projects().map_err(map_service_err)?;
    let user_projects: Vec<Project> = all
        .into_iter()
        .filter(|p| ProjectAccess::new(&auth_ctx, p.user_id).can_read())
        .collect();
    Ok(Json(user_projects))
}

async fn create_project(
    State(state): State<ApiState>,
    auth_ctx: AuthContext,
    Json(req): Json<CreateProjectRequest>,
) -> Result<(StatusCode, Json<Project>), ApiError> {
    if !auth_ctx.has_scope("*") && !auth_ctx.has_scope("project:write") {
        return Err(ApiError::Forbidden(
            "missing required scope: project:write".into(),
        ));
    }

    let server_command = match PlatformConfig::from_env() {
        Ok(c) => c.server_command,
        Err(_) => vec![
            "/bin/busybox".into(),
            "httpd".into(),
            "-f".into(),
            "-p".into(),
            "{PORT}".into(),
            "-h".into(),
            "/srv/app".into(),
        ],
    };
    let input = CreateProjectInput {
        name: req.name,
        source_dir: req.source_dir,
        base_image: req.base_image,
        server_command,
        user_id: if auth_ctx.user_id.is_nil() {
            None
        } else {
            Some(auth_ctx.user_id)
        },
    };
    let project = state
        .service
        .create_project(input)
        .map_err(map_service_err)?;

    let audit = AuditService::new(state.db.clone());
    let _ = audit
        .record(
            auth_ctx.user_id,
            "project.create",
            "project",
            project.id,
            serde_json::json!({ "name": project.name }),
        )
        .await;

    Ok((StatusCode::CREATED, Json(project)))
}

async fn get_project(
    State(state): State<ApiState>,
    auth_ctx: AuthContext,
    Path(project_id): Path<String>,
) -> Result<Json<Project>, ApiError> {
    let id = parse_id(&project_id)?;
    let project = state.service.project(id).map_err(map_service_err)?;
    let access = ProjectAccess::new(&auth_ctx, project.user_id);
    if !access.can_read() {
        return Err(ApiError::Forbidden("access to project denied".into()));
    }
    Ok(Json(project))
}

async fn list_deployments(
    State(state): State<ApiState>,
    auth_ctx: AuthContext,
    Path(project_id): Path<String>,
) -> Result<Json<Vec<Deployment>>, ApiError> {
    let id = parse_id(&project_id)?;
    let project = state.service.project(id).map_err(map_service_err)?;
    let access = ProjectAccess::new(&auth_ctx, project.user_id);
    if !access.can_read() {
        return Err(ApiError::Forbidden(
            "access to project deployments denied".into(),
        ));
    }
    let list = state.service.deployments(id).map_err(map_service_err)?;
    Ok(Json(list))
}

async fn create_deployment(
    State(state): State<ApiState>,
    auth_ctx: AuthContext,
    Path(project_id): Path<String>,
) -> Result<(StatusCode, Json<Deployment>), ApiError> {
    let id = parse_id(&project_id)?;
    let project = state.service.project(id).map_err(map_service_err)?;
    let access = ProjectAccess::new(&auth_ctx, project.user_id);
    if !access.can_operate() {
        return Err(ApiError::Forbidden("operation on project denied".into()));
    }
    let deployment = state.service.deploy(id).map_err(map_service_err)?;

    let audit = AuditService::new(state.db.clone());
    let _ = audit
        .record(
            auth_ctx.user_id,
            "deployment.create",
            "deployment",
            deployment.id,
            serde_json::json!({ "project_id": project.id, "status": deployment.status }),
        )
        .await;

    Ok((StatusCode::CREATED, Json(deployment)))
}

async fn get_deployment(
    State(state): State<ApiState>,
    auth_ctx: AuthContext,
    Path(deployment_id): Path<String>,
) -> Result<Json<Deployment>, ApiError> {
    let id = parse_id(&deployment_id)?;
    let deployment = state.service.deployment(id).map_err(map_service_err)?;
    let project = state
        .service
        .project(deployment.project_id)
        .map_err(map_service_err)?;
    let access = ProjectAccess::new(&auth_ctx, project.user_id);
    if !access.can_read() {
        return Err(ApiError::Forbidden("access to deployment denied".into()));
    }
    Ok(Json(deployment))
}

async fn get_deployment_logs(
    State(state): State<ApiState>,
    auth_ctx: AuthContext,
    Path(deployment_id): Path<String>,
) -> Result<Json<LogsResponse>, ApiError> {
    let id = parse_id(&deployment_id)?;
    let deployment = state.service.deployment(id).map_err(map_service_err)?;
    let project = state
        .service
        .project(deployment.project_id)
        .map_err(map_service_err)?;
    let access = ProjectAccess::new(&auth_ctx, project.user_id);
    if !access.can_read() {
        return Err(ApiError::Forbidden(
            "access to deployment logs denied".into(),
        ));
    }
    let logs = state.service.logs(id).map_err(map_service_err)?;
    Ok(Json(LogsResponse { logs }))
}

async fn stop_deployment(
    State(state): State<ApiState>,
    auth_ctx: AuthContext,
    Path(deployment_id): Path<String>,
) -> Result<Json<Deployment>, ApiError> {
    let id = parse_id(&deployment_id)?;
    let deployment = state.service.deployment(id).map_err(map_service_err)?;
    let project = state
        .service
        .project(deployment.project_id)
        .map_err(map_service_err)?;
    let access = ProjectAccess::new(&auth_ctx, project.user_id);
    if !access.can_operate() {
        return Err(ApiError::Forbidden("stopping deployment denied".into()));
    }
    let deployment = state.service.stop(id).map_err(map_service_err)?;

    let audit = AuditService::new(state.db.clone());
    let _ = audit
        .record(
            auth_ctx.user_id,
            "deployment.stop",
            "deployment",
            deployment.id,
            serde_json::json!({ "project_id": project.id, "status": deployment.status }),
        )
        .await;

    Ok(Json(deployment))
}

async fn login_session(
    State(state): State<ApiState>,
    jar: CookieJar,
    Json(req): Json<LoginRequest>,
) -> Result<(CookieJar, (StatusCode, Json<SessionResponse>)), ApiError> {
    let auth = state
        .auth
        .as_ref()
        .ok_or_else(|| ApiError::Internal("auth service not configured".into()))?;

    let (user, session) = auth
        .login(&req.email, &req.password, time::Duration::days(7))
        .await
        .map_err(|e| ApiError::Unauthorized(e.to_string()))?;

    let cookie = Cookie::build(("dp_session", session.token.clone()))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(time::Duration::days(7))
        .build();

    let jar = jar.add(cookie);
    let resp = SessionResponse {
        user: UserSummary {
            id: user.id,
            email: user.email,
            name: user.name,
        },
        token: session.token,
        expires_at: session
            .expires_at
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default(),
    };

    Ok((jar, (StatusCode::OK, Json(resp))))
}

async fn register_session(
    State(state): State<ApiState>,
    jar: CookieJar,
    Json(req): Json<RegisterRequest>,
) -> Result<(CookieJar, (StatusCode, Json<SessionResponse>)), ApiError> {
    let auth = state
        .auth
        .as_ref()
        .ok_or_else(|| ApiError::Internal("auth service not configured".into()))?;

    let user = auth
        .register(&req.email, &req.password, &req.name)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let session = auth
        .create_session(user.id, time::Duration::days(7))
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let cookie = Cookie::build(("dp_session", session.token.clone()))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(time::Duration::days(7))
        .build();

    let jar = jar.add(cookie);
    let resp = SessionResponse {
        user: UserSummary {
            id: user.id,
            email: user.email,
            name: user.name,
        },
        token: session.token,
        expires_at: session
            .expires_at
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default(),
    };

    Ok((jar, (StatusCode::CREATED, Json(resp))))
}

async fn logout_session(
    State(state): State<ApiState>,
    auth_ctx: AuthContext,
    jar: CookieJar,
) -> Result<(CookieJar, StatusCode), ApiError> {
    if let Some(session_id) = auth_ctx.session_id {
        if let Some(auth) = &state.auth {
            let _ = auth.revoke_session(session_id).await;
        }
    }

    let cookie = Cookie::build(("dp_session", ""))
        .path("/")
        .http_only(true)
        .same_site(SameSite::Lax)
        .max_age(time::Duration::ZERO)
        .build();

    let jar = jar.add(cookie);
    Ok((jar, StatusCode::NO_CONTENT))
}

async fn get_current_user(
    State(state): State<ApiState>,
    auth_ctx: AuthContext,
) -> Result<Json<MeResponse>, ApiError> {
    if let Some(db) = &state.db {
        let mut users = db.users();
        let user = users
            .get_by_id(auth_ctx.user_id)
            .await
            .map_err(|_| ApiError::NotFound("user not found".into()))?;
        Ok(Json(MeResponse {
            id: user.id,
            email: user.email,
            name: user.name,
            scopes: auth_ctx.scopes,
        }))
    } else {
        Ok(Json(MeResponse {
            id: auth_ctx.user_id,
            email: "dev@deployplatform.local".into(),
            name: "Dev User".into(),
            scopes: auth_ctx.scopes,
        }))
    }
}

async fn create_api_token(
    State(state): State<ApiState>,
    auth_ctx: AuthContext,
    Json(req): Json<CreateApiTokenRequest>,
) -> Result<(StatusCode, Json<IssuedToken>), ApiError> {
    let auth = state
        .auth
        .as_ref()
        .ok_or_else(|| ApiError::Internal("auth service not configured".into()))?;

    let scopes = req.scopes.unwrap_or_else(|| vec!["*".into()]);
    let issued = auth
        .issue_api_token(auth_ctx.user_id, &req.name, scopes, None)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let audit = AuditService::new(state.db.clone());
    let _ = audit
        .record(
            auth_ctx.user_id,
            "api_token.create",
            "api_token",
            issued.id,
            serde_json::json!({ "name": issued.name, "prefix": issued.prefix }),
        )
        .await;

    Ok((StatusCode::CREATED, Json(issued)))
}

async fn list_user_api_tokens(
    State(state): State<ApiState>,
    auth_ctx: AuthContext,
) -> Result<Json<Vec<ApiTokenSummary>>, ApiError> {
    let auth = state
        .auth
        .as_ref()
        .ok_or_else(|| ApiError::Internal("auth service not configured".into()))?;

    let list = auth
        .list_api_tokens(auth_ctx.user_id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    Ok(Json(list))
}

async fn revoke_api_token(
    State(state): State<ApiState>,
    auth_ctx: AuthContext,
    Path(token_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = parse_id(&token_id)?;
    let auth = state
        .auth
        .as_ref()
        .ok_or_else(|| ApiError::Internal("auth service not configured".into()))?;

    auth.revoke_api_token(auth_ctx.user_id, id)
        .await
        .map_err(|e| ApiError::NotFound(e.to_string()))?;

    let audit = AuditService::new(state.db.clone());
    let _ = audit
        .record(
            auth_ctx.user_id,
            "api_token.revoke",
            "api_token",
            id,
            serde_json::json!({}),
        )
        .await;

    Ok(StatusCode::NO_CONTENT)
}

pub fn router(service: Arc<dyn PlatformService>) -> Router {
    router_with_auth(service, None, None)
}

pub fn router_with_auth(
    service: Arc<dyn PlatformService>,
    auth: Option<AuthService>,
    db: Option<Database>,
) -> Router {
    let state = ApiState { service, auth, db };
    Router::new()
        .route("/healthz", get(healthz))
        .route(
            "/v1/auth/session",
            post(login_session).delete(logout_session),
        )
        .route("/v1/auth/register", post(register_session))
        .route("/v1/me", get(get_current_user))
        .route(
            "/v1/me/api-tokens",
            get(list_user_api_tokens).post(create_api_token),
        )
        .route("/v1/me/api-tokens/:token_id", delete(revoke_api_token))
        .route("/v1/projects", get(list_projects).post(create_project))
        .route("/v1/projects/:project_id", get(get_project))
        .route(
            "/v1/projects/:project_id/deployments",
            get(list_deployments).post(create_deployment),
        )
        .route("/v1/deployments/:deployment_id", get(get_deployment))
        .route(
            "/v1/deployments/:deployment_id/logs",
            get(get_deployment_logs),
        )
        .route("/v1/deployments/:deployment_id/stop", post(stop_deployment))
        .with_state(state)
}
