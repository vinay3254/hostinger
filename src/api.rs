use crate::auth::{
    ApiTokenSummary, AuditService, AuthContext, AuthService, IssuedToken, ProjectAccess,
};
use crate::config::PlatformConfig;
use crate::db::Database;
use crate::model::{CreateProjectInput, Deployment, Project};
use crate::providers::{crypto, Provider, ProviderClient};
use crate::repository::{ProjectSourceDetails, UpsertConnectionInput};
use crate::service::PlatformService;
use crate::source_events::SourceEventRecord;
use axum::{
    extract::{FromRef, FromRequestParts, Path, Query, State},
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
    pub providers: Vec<Arc<dyn ProviderClient>>,
    pub queue: Option<crate::queue::BuildQueue>,
}

impl ApiState {
    pub fn get_provider(&self, provider: Provider) -> Option<Arc<dyn ProviderClient>> {
        self.providers
            .iter()
            .find(|p| p.provider() == provider)
            .cloned()
    }
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

    if let Some(db) = &state.db {
        if !auth_ctx.user_id.is_nil() {
            let _ = db
                .projects()
                .create_project(crate::repository::CreateProjectRecord {
                    id: project.id,
                    user_id: auth_ctx.user_id,
                    name: project.name.clone(),
                    source_dir: project.source_dir.clone(),
                    base_image: project.base_image.clone(),
                    server_command: project.server_command.clone(),
                    created_at: project.created_at,
                })
                .await;
        }
    }

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

    if let Some(queue) = &state.queue {
        let deployment_id = Uuid::new_v4();
        let now = time::OffsetDateTime::now_utc();
        if let Some(db) = &state.db {
            let _ = sqlx::query(
                "INSERT INTO deployments (id, project_id, framework, status, created_at)
                 VALUES ($1, $2, 'static', 'queued', $3)",
            )
            .bind(deployment_id)
            .bind(id)
            .bind(now)
            .execute(db.pool())
            .await;
        }

        queue
            .enqueue_job(deployment_id, id, crate::queue::BuildPriority::Production)
            .await
            .map_err(|e| ApiError::Internal(e.to_string()))?;

        let deployment = Deployment {
            id: deployment_id,
            project_id: id,
            framework: crate::model::Framework::Static,
            status: crate::model::DeploymentStatus::Queued,
            image_path: None,
            container_id: None,
            port: None,
            url: None,
            error: None,
            created_at: now,
            finished_at: None,
        };

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

        Ok((StatusCode::ACCEPTED, Json(deployment)))
    } else {
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

async fn cancel_deployment(
    State(state): State<ApiState>,
    _auth_ctx: AuthContext,
    Path(deployment_id): Path<String>,
) -> Result<Json<Deployment>, ApiError> {
    let id = parse_id(&deployment_id)?;
    if let Some(queue) = &state.queue {
        if let Ok(Some(job)) = queue.get_job_by_deployment(id).await {
            let _ = queue.cancel(job.id).await;
        }
    }
    if let Some(db) = &state.db {
        let _ = sqlx::query("UPDATE deployments SET status = 'cancelled' WHERE id = $1")
            .bind(id)
            .execute(db.pool())
            .await;
    }
    let deployment = state.service.deployment(id).unwrap_or(Deployment {
        id,
        project_id: Uuid::nil(),
        framework: crate::model::Framework::Static,
        status: crate::model::DeploymentStatus::Cancelled,
        image_path: None,
        container_id: None,
        port: None,
        url: None,
        error: None,
        created_at: time::OffsetDateTime::now_utc(),
        finished_at: Some(time::OffsetDateTime::now_utc()),
    });
    Ok(Json(deployment))
}

async fn retry_deployment(
    State(state): State<ApiState>,
    _auth_ctx: AuthContext,
    Path(deployment_id): Path<String>,
) -> Result<(StatusCode, Json<Deployment>), ApiError> {
    let id = parse_id(&deployment_id)?;
    let now = time::OffsetDateTime::now_utc();
    if let Some(queue) = &state.queue {
        if let Some(db) = &state.db {
            let proj_id_opt: Option<Uuid> =
                sqlx::query_scalar("SELECT project_id FROM deployments WHERE id = $1")
                    .bind(id)
                    .fetch_optional(db.pool())
                    .await
                    .map_err(|e| ApiError::Internal(e.to_string()))?;

            if let Some(proj_id) = proj_id_opt {
                let _ = sqlx::query(
                    "UPDATE deployments SET status = 'queued', error = NULL WHERE id = $1",
                )
                .bind(id)
                .execute(db.pool())
                .await;

                let _ = queue
                    .enqueue_job(id, proj_id, crate::queue::BuildPriority::Production)
                    .await;
            }
        }
    }
    let deployment = Deployment {
        id,
        project_id: Uuid::nil(),
        framework: crate::model::Framework::Static,
        status: crate::model::DeploymentStatus::Queued,
        image_path: None,
        container_id: None,
        port: None,
        url: None,
        error: None,
        created_at: now,
        finished_at: None,
    };
    Ok((StatusCode::ACCEPTED, Json(deployment)))
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

#[derive(serde::Deserialize)]
pub struct OAuthConnectQuery {
    pub redirect_uri: Option<String>,
}

#[derive(serde::Deserialize)]
pub struct OAuthCallbackQuery {
    pub code: String,
    pub state: String,
    pub redirect_uri: Option<String>,
}

async fn provider_connect(
    State(state): State<ApiState>,
    auth_ctx: AuthContext,
    Path(provider_str): Path<String>,
    Query(query): Query<OAuthConnectQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let provider: Provider = provider_str
        .parse()
        .map_err(|e: anyhow::Error| ApiError::BadRequest(e.to_string()))?;
    let client = state
        .get_provider(provider)
        .ok_or_else(|| ApiError::BadRequest(format!("provider {provider} is not configured")))?;
    let db = state
        .db
        .clone()
        .ok_or_else(|| ApiError::Internal("database not configured".into()))?;

    let state_token = format!(
        "{}_{}",
        Uuid::new_v4().simple(),
        hex::encode(rand::random::<[u8; 16]>())
    );
    let redirect_uri = query
        .redirect_uri
        .unwrap_or_else(|| format!("http://localhost:3000/api/providers/{provider}/callback"));

    let now = time::OffsetDateTime::now_utc();
    let expires_at = now + time::Duration::minutes(5);
    db.providers()
        .save_oauth_state(
            &state_token,
            auth_ctx.user_id,
            provider,
            Some(&redirect_uri),
            expires_at,
            now,
        )
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let auth_url = client.begin_authorization(&state_token, &redirect_uri);
    Ok(Json(serde_json::json!({
        "url": auth_url,
        "state": state_token
    })))
}

async fn provider_callback(
    State(state): State<ApiState>,
    auth_ctx: AuthContext,
    Path(provider_str): Path<String>,
    Query(query): Query<OAuthCallbackQuery>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let provider: Provider = provider_str
        .parse()
        .map_err(|e: anyhow::Error| ApiError::BadRequest(e.to_string()))?;
    let client = state
        .get_provider(provider)
        .ok_or_else(|| ApiError::BadRequest(format!("provider {provider} is not configured")))?;
    let db = state
        .db
        .clone()
        .ok_or_else(|| ApiError::Internal("database not configured".into()))?;

    let state_record = db
        .providers()
        .consume_oauth_state(&query.state)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?
        .ok_or_else(|| ApiError::BadRequest("invalid or expired oauth state".into()))?;

    if state_record.user_id != auth_ctx.user_id || state_record.provider != provider {
        return Err(ApiError::BadRequest("oauth state mismatch".into()));
    }

    let redirect_uri = query
        .redirect_uri
        .or(state_record.redirect_url)
        .unwrap_or_else(|| format!("http://localhost:3000/api/providers/{provider}/callback"));

    let token_resp = client
        .complete_authorization(&query.code, &redirect_uri)
        .await
        .map_err(|e| ApiError::BadRequest(format!("oauth authorization failed: {e}")))?;

    let enc_key = crypto::default_encryption_key();
    let enc_access_token = crypto::encrypt_token(&token_resp.access_token, &enc_key)
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    let enc_refresh_token = match &token_resp.refresh_token {
        Some(t) => Some(
            crypto::encrypt_token(t, &enc_key).map_err(|e| ApiError::Internal(e.to_string()))?,
        ),
        None => None,
    };

    let now = time::OffsetDateTime::now_utc();
    let expires_at = token_resp
        .expires_in
        .map(|sec| now + time::Duration::seconds(sec));

    let conn = db
        .providers()
        .upsert_connection(UpsertConnectionInput {
            id: Uuid::new_v4(),
            user_id: auth_ctx.user_id,
            provider,
            external_user_id: token_resp.external_user_id.clone(),
            access_token_encrypted: enc_access_token,
            refresh_token_encrypted: enc_refresh_token,
            expires_at,
            created_at: now,
            updated_at: now,
        })
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    // Sync repositories
    if let Ok(repos) = client.list_repositories(&token_resp.access_token).await {
        let _ = db.providers().save_repositories(conn.id, &repos, now).await;
    }

    let audit = AuditService::new(state.db.clone());
    let _ = audit
        .record(
            auth_ctx.user_id,
            "provider.connect",
            "provider_connection",
            conn.id,
            serde_json::json!({
                "provider": provider.to_string(),
                "external_user_id": token_resp.external_user_id
            }),
        )
        .await;

    Ok(Json(serde_json::json!({
        "status": "connected",
        "provider": provider.to_string(),
        "external_user_id": token_resp.external_user_id
    })))
}

async fn list_provider_repositories(
    State(state): State<ApiState>,
    auth_ctx: AuthContext,
    Path(provider_str): Path<String>,
) -> Result<Json<Vec<crate::repository::ProviderRepositoryRecord>>, ApiError> {
    let provider: Provider = provider_str
        .parse()
        .map_err(|e: anyhow::Error| ApiError::BadRequest(e.to_string()))?;
    let db = state
        .db
        .clone()
        .ok_or_else(|| ApiError::Internal("database not configured".into()))?;

    let repos = db
        .providers()
        .list_repositories(auth_ctx.user_id, provider)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    Ok(Json(repos))
}

#[derive(serde::Deserialize)]
pub struct UpdateProjectSourceRequest {
    pub repository_id: Uuid,
    pub target_branch: Option<String>,
}

async fn get_project_source(
    State(state): State<ApiState>,
    auth_ctx: AuthContext,
    Path(project_id): Path<String>,
) -> Result<Json<ProjectSourceDetails>, ApiError> {
    let id = parse_id(&project_id)?;
    let project = state.service.project(id).map_err(map_service_err)?;
    let access = ProjectAccess::new(&auth_ctx, project.user_id);
    if !access.can_read() {
        return Err(ApiError::Forbidden(
            "access to project source denied".into(),
        ));
    }
    let db = state
        .db
        .as_ref()
        .ok_or_else(|| ApiError::Internal("database not configured".into()))?;
    let mut projects_repo = db.projects();
    let details = projects_repo
        .get_project_source(id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(Json(details))
}

async fn update_project_source(
    State(state): State<ApiState>,
    auth_ctx: AuthContext,
    Path(project_id): Path<String>,
    Json(req): Json<UpdateProjectSourceRequest>,
) -> Result<Json<ProjectSourceDetails>, ApiError> {
    let id = parse_id(&project_id)?;
    let project = state.service.project(id).map_err(map_service_err)?;
    let access = ProjectAccess::new(&auth_ctx, project.user_id);
    if !access.can_mutate() {
        return Err(ApiError::Forbidden("operation on project denied".into()));
    }
    let db = state
        .db
        .as_ref()
        .ok_or_else(|| ApiError::Internal("database not configured".into()))?;
    let generated_secret = hex::encode(rand::random::<[u8; 24]>());
    let target_branch = req.target_branch.unwrap_or_else(|| "main".to_string());

    let mut projects_repo = db.projects();
    projects_repo
        .set_project_source(id, req.repository_id, &target_branch, &generated_secret)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let details = projects_repo
        .get_project_source(id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let audit = AuditService::new(state.db.clone());
    let _ = audit
        .record(
            auth_ctx.user_id,
            "project.source.update",
            "project",
            id,
            serde_json::json!({
                "repository_id": req.repository_id,
                "target_branch": target_branch,
            }),
        )
        .await;

    Ok(Json(details))
}

async fn disconnect_project_source(
    State(state): State<ApiState>,
    auth_ctx: AuthContext,
    Path(project_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let id = parse_id(&project_id)?;
    let project = state.service.project(id).map_err(map_service_err)?;
    let access = ProjectAccess::new(&auth_ctx, project.user_id);
    if !access.can_mutate() {
        return Err(ApiError::Forbidden("operation on project denied".into()));
    }
    let db = state
        .db
        .as_ref()
        .ok_or_else(|| ApiError::Internal("database not configured".into()))?;
    let mut projects_repo = db.projects();
    projects_repo
        .disconnect_project_source(id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let audit = AuditService::new(state.db.clone());
    let _ = audit
        .record(
            auth_ctx.user_id,
            "project.source.disconnect",
            "project",
            id,
            serde_json::json!({}),
        )
        .await;

    Ok(StatusCode::NO_CONTENT)
}

async fn list_project_source_events(
    State(state): State<ApiState>,
    auth_ctx: AuthContext,
    Path(project_id): Path<String>,
) -> Result<Json<Vec<SourceEventRecord>>, ApiError> {
    let id = parse_id(&project_id)?;
    let project = state.service.project(id).map_err(map_service_err)?;
    let access = ProjectAccess::new(&auth_ctx, project.user_id);
    if !access.can_read() {
        return Err(ApiError::Forbidden(
            "access to project source events denied".into(),
        ));
    }
    let db = state
        .db
        .as_ref()
        .ok_or_else(|| ApiError::Internal("database not configured".into()))?;
    let mut events_repo = db.source_events();
    let events = events_repo
        .list_by_project(id)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(Json(events))
}

pub fn router(service: Arc<dyn PlatformService>) -> Router {
    router_with_auth(service, None, None)
}

pub fn router_with_auth(
    service: Arc<dyn PlatformService>,
    auth: Option<AuthService>,
    db: Option<Database>,
) -> Router {
    router_with_providers(service, auth, db, default_providers())
}

pub fn default_providers() -> Vec<Arc<dyn ProviderClient>> {
    let mut providers: Vec<Arc<dyn ProviderClient>> = Vec::new();
    if let (Ok(id), Ok(sec)) = (
        std::env::var("GITHUB_CLIENT_ID"),
        std::env::var("GITHUB_CLIENT_SECRET"),
    ) {
        providers.push(Arc::new(crate::providers::GitHubClient::new(id, sec)));
    }
    if let (Ok(id), Ok(sec)) = (
        std::env::var("GITLAB_CLIENT_ID"),
        std::env::var("GITLAB_CLIENT_SECRET"),
    ) {
        providers.push(Arc::new(crate::providers::GitLabClient::new(id, sec)));
    }
    providers
}

pub fn router_with_providers(
    service: Arc<dyn PlatformService>,
    auth: Option<AuthService>,
    db: Option<Database>,
    providers: Vec<Arc<dyn ProviderClient>>,
) -> Router {
    router_with_queue(service, auth, db, providers, None)
}

pub fn router_with_queue(
    service: Arc<dyn PlatformService>,
    auth: Option<AuthService>,
    db: Option<Database>,
    providers: Vec<Arc<dyn ProviderClient>>,
    queue: Option<crate::queue::BuildQueue>,
) -> Router {
    let state = ApiState {
        service,
        auth,
        db,
        providers,
        queue,
    };
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
        .route(
            "/v1/deployments/:deployment_id/cancel",
            post(cancel_deployment),
        )
        .route(
            "/v1/deployments/:deployment_id/retry",
            post(retry_deployment),
        )
        .route("/v1/providers/:provider/connect", get(provider_connect))
        .route("/v1/providers/:provider/callback", get(provider_callback))
        .route(
            "/v1/providers/:provider/repositories",
            get(list_provider_repositories),
        )
        .route(
            "/v1/projects/:project_id/source",
            get(get_project_source)
                .post(update_project_source)
                .delete(disconnect_project_source),
        )
        .route(
            "/v1/projects/:project_id/source/events",
            get(list_project_source_events),
        )
        .route(
            "/v1/webhooks/:provider/:project_id",
            post(crate::webhooks::handle_webhook),
        )
        .with_state(state)
}
