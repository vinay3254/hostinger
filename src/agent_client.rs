use crate::agent_protocol::{
    AgentCommand, AgentEnvelope, AgentProtocolError, AgentResponse, AGENT_PROTOCOL_VERSION,
};
use reqwest::Client;
use std::time::Duration;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug)]
pub enum AgentClientError {
    Http(reqwest::Error),
    Protocol(AgentProtocolError),
    RemoteRejected { status: u16, message: String },
    Deserialization(String),
}

impl std::fmt::Display for AgentClientError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Http(e) => write!(f, "HTTP transport error: {}", e),
            Self::Protocol(e) => write!(f, "Agent protocol error: {}", e),
            Self::RemoteRejected { status, message } => {
                write!(
                    f,
                    "Remote agent rejected request with status {}: {}",
                    status, message
                )
            }
            Self::Deserialization(msg) => {
                write!(f, "Failed to parse agent response: {}", msg)
            }
        }
    }
}

impl std::error::Error for AgentClientError {}

impl From<reqwest::Error> for AgentClientError {
    fn from(e: reqwest::Error) -> Self {
        Self::Http(e)
    }
}

impl From<AgentProtocolError> for AgentClientError {
    fn from(e: AgentProtocolError) -> Self {
        Self::Protocol(e)
    }
}

#[derive(Debug, Clone)]
pub struct AgentClient {
    client: Client,
    pub node_id: Uuid,
    pub endpoint: String,
    pub secret_key: String,
}

impl AgentClient {
    pub fn new(node_id: Uuid, endpoint: String, secret_key: String) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            client,
            node_id,
            endpoint: endpoint.trim_end_matches('/').to_string(),
            secret_key,
        }
    }

    pub async fn execute(&self, command: AgentCommand) -> Result<AgentResponse, AgentClientError> {
        let operation_id = Uuid::new_v4();
        let now = OffsetDateTime::now_utc();

        let envelope = AgentEnvelope::new_signed(
            AGENT_PROTOCOL_VERSION.to_string(),
            operation_id,
            self.node_id,
            now,
            command,
            &self.secret_key,
        )?;

        let rpc_url = format!("{}/agent/rpc", self.endpoint);
        let resp = self.client.post(&rpc_url).json(&envelope).send().await?;

        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            return Err(AgentClientError::RemoteRejected {
                status: status.as_u16(),
                message: body_text,
            });
        }

        let agent_resp = resp
            .json::<AgentResponse>()
            .await
            .map_err(|e| AgentClientError::Deserialization(e.to_string()))?;

        Ok(agent_resp)
    }
}
