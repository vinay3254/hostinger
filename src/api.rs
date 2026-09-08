use crate::config::PlatformConfig;
use crate::model::{CreateProjectInput, Deployment, Project};
use crate::service::PlatformService;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use std::path::PathBuf;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone)]
pub struct ApiState {
    pub service: Arc<dyn PlatformService>,
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
    NotFound(String),
    UnprocessableEntity(String),
    #[allow(dead_code)]
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, msg) = match self {
            Self::BadRequest(msg) => (StatusCode::BAD_REQUEST, msg),
            Self::NotFound(msg) => (StatusCode::NOT_FOUND, msg),
            Self::UnprocessableEntity(msg) => (StatusCode::UNPROCESSABLE_ENTITY, msg),
            Self::Internal(msg) => (StatusCode::INTERNAL_SERVER_ERROR, msg),
        };
        (status, Json(ErrorResponse { error: msg })).into_response()
    }
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

async fn create_project(
    State(state): State<ApiState>,
    Json(req): Json<CreateProjectRequest>,
) -> Result<(StatusCode, Json<Project>), ApiError> {
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
    };
    let project = state
        .service
        .create_project(input)
        .map_err(map_service_err)?;
    Ok((StatusCode::CREATED, Json(project)))
}

async fn get_project(
    State(state): State<ApiState>,
    Path(project_id): Path<String>,
) -> Result<Json<Project>, ApiError> {
    let id = parse_id(&project_id)?;
    let project = state.service.project(id).map_err(map_service_err)?;
    Ok(Json(project))
}

async fn list_deployments(
    State(state): State<ApiState>,
    Path(project_id): Path<String>,
) -> Result<Json<Vec<Deployment>>, ApiError> {
    let id = parse_id(&project_id)?;
    let list = state.service.deployments(id).map_err(map_service_err)?;
    Ok(Json(list))
}

async fn create_deployment(
    State(state): State<ApiState>,
    Path(project_id): Path<String>,
) -> Result<(StatusCode, Json<Deployment>), ApiError> {
    let id = parse_id(&project_id)?;
    let deployment = state.service.deploy(id).map_err(map_service_err)?;
    Ok((StatusCode::CREATED, Json(deployment)))
}

async fn get_deployment(
    State(state): State<ApiState>,
    Path(deployment_id): Path<String>,
) -> Result<Json<Deployment>, ApiError> {
    let id = parse_id(&deployment_id)?;
    let deployment = state.service.deployment(id).map_err(map_service_err)?;
    Ok(Json(deployment))
}

async fn get_deployment_logs(
    State(state): State<ApiState>,
    Path(deployment_id): Path<String>,
) -> Result<Json<LogsResponse>, ApiError> {
    let id = parse_id(&deployment_id)?;
    let logs = state.service.logs(id).map_err(map_service_err)?;
    Ok(Json(LogsResponse { logs }))
}

async fn stop_deployment(
    State(state): State<ApiState>,
    Path(deployment_id): Path<String>,
) -> Result<Json<Deployment>, ApiError> {
    let id = parse_id(&deployment_id)?;
    let deployment = state.service.stop(id).map_err(map_service_err)?;
    Ok(Json(deployment))
}

pub fn router(service: Arc<dyn PlatformService>) -> Router {
    let state = ApiState { service };
    Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/projects", post(create_project))
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
