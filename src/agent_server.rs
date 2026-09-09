use crate::agent_protocol::{
    AgentCommand, AgentEnvelope, AgentProtocolError, AgentProtocolValidator, AgentResponse,
    CreateReleaseCommand, CreateReleaseResponse, DrainReleaseResponse, HealthResponse,
    NodeCredentials, ReleaseLogsResponse, ReleaseStatsResponse, StartReleaseResponse,
    StopReleaseResponse, AGENT_PROTOCOL_VERSION,
};
use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use uuid::Uuid;

#[async_trait::async_trait]
pub trait AgentRuntimeAdapter: Send + Sync {
    async fn create_release(
        &self,
        cmd: &CreateReleaseCommand,
    ) -> Result<CreateReleaseResponse, AgentProtocolError>;

    async fn start_release(
        &self,
        release_id: Uuid,
        container_id: &str,
    ) -> Result<StartReleaseResponse, AgentProtocolError>;

    async fn stop_release(
        &self,
        release_id: Uuid,
        container_id: &str,
        grace_period_secs: u32,
    ) -> Result<StopReleaseResponse, AgentProtocolError>;

    async fn drain_release(
        &self,
        release_id: Uuid,
        drain_timeout_secs: u32,
    ) -> Result<DrainReleaseResponse, AgentProtocolError>;

    async fn get_logs(
        &self,
        release_id: Uuid,
        since_seq: Option<u64>,
        limit: usize,
    ) -> Result<ReleaseLogsResponse, AgentProtocolError>;

    async fn get_stats(&self, release_id: Uuid)
        -> Result<ReleaseStatsResponse, AgentProtocolError>;
}

#[derive(Debug, Default)]
pub struct MockAgentRuntimeAdapter {
    pub created_releases: Mutex<HashMap<Uuid, CreateReleaseCommand>>,
    pub running_containers: Mutex<HashMap<Uuid, String>>, // release_id -> container_id
    pub logs_store: Mutex<HashMap<Uuid, Vec<String>>>,
}

impl MockAgentRuntimeAdapter {
    pub fn new() -> Self {
        Self::default()
    }
}

#[async_trait::async_trait]
impl AgentRuntimeAdapter for MockAgentRuntimeAdapter {
    async fn create_release(
        &self,
        cmd: &CreateReleaseCommand,
    ) -> Result<CreateReleaseResponse, AgentProtocolError> {
        let container_id = format!("mock-container-{}", &cmd.release_id.to_string()[0..8]);
        let mut created = self.created_releases.lock().unwrap();
        created.insert(cmd.release_id, cmd.clone());

        Ok(CreateReleaseResponse {
            release_id: cmd.release_id,
            container_id,
            assigned_port: 41000,
        })
    }

    async fn start_release(
        &self,
        release_id: Uuid,
        container_id: &str,
    ) -> Result<StartReleaseResponse, AgentProtocolError> {
        let mut running = self.running_containers.lock().unwrap();
        running.insert(release_id, container_id.to_string());

        let mut logs = self.logs_store.lock().unwrap();
        logs.entry(release_id).or_default().push(format!(
            "Container {} started successfully on port 41000",
            container_id
        ));

        Ok(StartReleaseResponse {
            started: true,
            port: 41000,
        })
    }

    async fn stop_release(
        &self,
        release_id: Uuid,
        _container_id: &str,
        _grace_period_secs: u32,
    ) -> Result<StopReleaseResponse, AgentProtocolError> {
        let mut running = self.running_containers.lock().unwrap();
        running.remove(&release_id);

        Ok(StopReleaseResponse { stopped: true })
    }

    async fn drain_release(
        &self,
        _release_id: Uuid,
        _drain_timeout_secs: u32,
    ) -> Result<DrainReleaseResponse, AgentProtocolError> {
        Ok(DrainReleaseResponse { draining: true })
    }

    async fn get_logs(
        &self,
        release_id: Uuid,
        _since_seq: Option<u64>,
        limit: usize,
    ) -> Result<ReleaseLogsResponse, AgentProtocolError> {
        let logs = self.logs_store.lock().unwrap();
        let list = logs
            .get(&release_id)
            .cloned()
            .unwrap_or_else(|| vec!["Release initialized".to_string()]);
        let count = list.len();
        let trimmed = list.into_iter().take(limit).collect();
        Ok(ReleaseLogsResponse {
            logs: trimmed,
            next_seq: count as u64,
        })
    }

    async fn get_stats(
        &self,
        _release_id: Uuid,
    ) -> Result<ReleaseStatsResponse, AgentProtocolError> {
        Ok(ReleaseStatsResponse {
            cpu_millicores: 120,
            memory_bytes: 64 * 1024 * 1024,
            uptime_secs: 42,
        })
    }
}

pub struct AgentServer<R: AgentRuntimeAdapter> {
    pub node_id: Uuid,
    pub credentials: NodeCredentials,
    pub validator: AgentProtocolValidator,
    pub runtime: Arc<R>,
    pub is_draining: Arc<AtomicBool>,
    pub start_time: Instant,
}

impl<R: AgentRuntimeAdapter> AgentServer<R> {
    pub fn new(node_id: Uuid, credentials: NodeCredentials, runtime: Arc<R>) -> Self {
        let mut validator = AgentProtocolValidator::new(Duration::from_secs(300));
        validator.register_credentials(credentials.clone());

        Self {
            node_id,
            credentials,
            validator,
            runtime,
            is_draining: Arc::new(AtomicBool::new(false)),
            start_time: Instant::now(),
        }
    }

    pub fn set_draining(&self, draining: bool) {
        self.is_draining.store(draining, Ordering::SeqCst);
    }

    pub fn is_draining(&self) -> bool {
        self.is_draining.load(Ordering::SeqCst)
    }

    pub async fn execute_command_direct(
        &self,
        command: AgentCommand,
    ) -> Result<AgentResponse, AgentProtocolError> {
        if self.is_draining() {
            if let AgentCommand::CreateRelease(_) = command {
                return Err(AgentProtocolError::ForbiddenCommand(
                    "Node is currently in draining mode; cannot accept new releases".to_string(),
                ));
            }
        }

        match command {
            AgentCommand::CreateRelease(cmd) => {
                let res = self.runtime.create_release(&cmd).await?;
                Ok(AgentResponse::CreateRelease(res))
            }
            AgentCommand::StartRelease {
                release_id,
                container_id,
            } => {
                let res = self
                    .runtime
                    .start_release(release_id, &container_id)
                    .await?;
                Ok(AgentResponse::StartRelease(res))
            }
            AgentCommand::StopRelease {
                release_id,
                container_id,
                grace_period_secs,
            } => {
                let res = self
                    .runtime
                    .stop_release(release_id, &container_id, grace_period_secs)
                    .await?;
                Ok(AgentResponse::StopRelease(res))
            }
            AgentCommand::DrainRelease {
                release_id,
                drain_timeout_secs,
            } => {
                let res = self
                    .runtime
                    .drain_release(release_id, drain_timeout_secs)
                    .await?;
                Ok(AgentResponse::DrainRelease(res))
            }
            AgentCommand::ReleaseLogs {
                release_id,
                since_seq,
                limit,
            } => {
                let res = self.runtime.get_logs(release_id, since_seq, limit).await?;
                Ok(AgentResponse::ReleaseLogs(res))
            }
            AgentCommand::ReleaseStats { release_id } => {
                let res = self.runtime.get_stats(release_id).await?;
                Ok(AgentResponse::ReleaseStats(res))
            }
            AgentCommand::Health => Ok(AgentResponse::Health(HealthResponse {
                healthy: !self.is_draining(),
                version: AGENT_PROTOCOL_VERSION.to_string(),
                uptime_secs: self.start_time.elapsed().as_secs(),
            })),
            AgentCommand::RegisterNode(_) => Err(AgentProtocolError::ForbiddenCommand(
                "RegisterNode must be handled by scheduler".to_string(),
            )),
            AgentCommand::Heartbeat(_) => Err(AgentProtocolError::ForbiddenCommand(
                "Heartbeat must be handled by scheduler".to_string(),
            )),
        }
    }

    pub async fn handle_envelope(
        &self,
        envelope: &AgentEnvelope,
    ) -> Result<AgentResponse, AgentProtocolError> {
        let command = self.validator.validate_and_unpack(envelope)?;
        self.execute_command_direct(command).await
    }

    pub fn into_router(server: Arc<Self>) -> Router
    where
        R: 'static,
    {
        Router::new()
            .route("/agent/rpc", post(handle_agent_rpc::<R>))
            .route("/health", get(handle_health::<R>))
            .with_state(server)
    }
}

async fn handle_agent_rpc<R: AgentRuntimeAdapter + 'static>(
    State(server): State<Arc<AgentServer<R>>>,
    Json(envelope): Json<AgentEnvelope>,
) -> Response {
    match server.handle_envelope(&envelope).await {
        Ok(res) => (StatusCode::OK, Json(res)).into_response(),
        Err(AgentProtocolError::InvalidSignature) => (
            StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "InvalidSignature" })),
        )
            .into_response(),
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn handle_health<R: AgentRuntimeAdapter + 'static>(
    State(server): State<Arc<AgentServer<R>>>,
) -> Response {
    let health = HealthResponse {
        healthy: !server.is_draining(),
        version: AGENT_PROTOCOL_VERSION.to_string(),
        uptime_secs: server.start_time.elapsed().as_secs(),
    };
    (StatusCode::OK, Json(health)).into_response()
}
