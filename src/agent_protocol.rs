use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use subtle::ConstantTimeEq;
use time::OffsetDateTime;
use uuid::Uuid;

type HmacSha256 = Hmac<Sha256>;

pub const AGENT_PROTOCOL_VERSION: &str = "v1";
pub const MAX_PAYLOAD_SIZE_BYTES: usize = 1024 * 1024; // 1 MB

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentProtocolError {
    VersionMismatch { expected: String, actual: String },
    ReplayDetected(Uuid),
    InvalidSignature,
    TimestampExpired { skew_secs: i64 },
    ForbiddenCommand(String),
    PayloadTooLarge { size: usize, max: usize },
    NodeNotFound(Uuid),
    TokenRevoked(Uuid),
    SerializationError(String),
}

impl std::fmt::Display for AgentProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::VersionMismatch { expected, actual } => {
                write!(
                    f,
                    "Protocol version mismatch: expected {}, got {}",
                    expected, actual
                )
            }
            Self::ReplayDetected(id) => write!(f, "Duplicate operation ID replayed: {}", id),
            Self::InvalidSignature => write!(f, "Invalid HMAC signature"),
            Self::TimestampExpired { skew_secs } => {
                write!(f, "Timestamp drift exceeded allowed window: {}s", skew_secs)
            }
            Self::ForbiddenCommand(reason) => write!(f, "Forbidden command: {}", reason),
            Self::PayloadTooLarge { size, max } => {
                write!(f, "Payload size {} exceeds maximum allowed {}", size, max)
            }
            Self::NodeNotFound(id) => write!(f, "Node not found: {}", id),
            Self::TokenRevoked(id) => write!(f, "Node token has been revoked: {}", id),
            Self::SerializationError(msg) => write!(f, "Serialization error: {}", msg),
        }
    }
}

impl std::error::Error for AgentProtocolError {}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NodeStatus {
    Online,
    Degraded,
    Draining,
    Offline,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeCapacity {
    pub cpu_millicores: u64,
    pub memory_bytes: u64,
    pub max_releases: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterNodeCommand {
    pub node_id: Uuid,
    pub hostname: String,
    pub endpoint: String,
    pub capacity: NodeCapacity,
    pub token_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeartbeatCommand {
    pub node_id: Uuid,
    pub epoch: u64,
    pub cpu_used_millicores: u64,
    pub memory_used_bytes: u64,
    pub running_releases: Vec<Uuid>,
    pub status: NodeStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateReleaseCommand {
    pub release_id: Uuid,
    pub deployment_id: Uuid,
    pub image_path: String,
    pub env: HashMap<String, String>,
    pub cpu_limit_millicores: u64,
    pub memory_limit_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", content = "params")]
pub enum AgentCommand {
    RegisterNode(RegisterNodeCommand),
    Heartbeat(HeartbeatCommand),
    CreateRelease(CreateReleaseCommand),
    StartRelease {
        release_id: Uuid,
        container_id: String,
    },
    StopRelease {
        release_id: Uuid,
        container_id: String,
        grace_period_secs: u32,
    },
    DrainRelease {
        release_id: Uuid,
        drain_timeout_secs: u32,
    },
    ReleaseLogs {
        release_id: Uuid,
        since_seq: Option<u64>,
        limit: usize,
    },
    ReleaseStats {
        release_id: Uuid,
    },
    Health,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegisterNodeResponse {
    pub registered: bool,
    pub assigned_epoch: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeartbeatResponse {
    pub acknowledged: bool,
    pub drain_requested: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateReleaseResponse {
    pub release_id: Uuid,
    pub container_id: String,
    pub assigned_port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartReleaseResponse {
    pub started: bool,
    pub port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StopReleaseResponse {
    pub stopped: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DrainReleaseResponse {
    pub draining: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseLogsResponse {
    pub logs: Vec<String>,
    pub next_seq: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseStatsResponse {
    pub cpu_millicores: u64,
    pub memory_bytes: u64,
    pub uptime_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthResponse {
    pub healthy: bool,
    pub version: String,
    pub uptime_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEnvelope {
    pub version: String,
    pub operation_id: Uuid,
    pub node_id: Uuid,
    #[serde(with = "time::serde::rfc3339")]
    pub timestamp: OffsetDateTime,
    pub payload: String,
    pub signature: String,
}

impl AgentEnvelope {
    pub fn new_signed(
        version: String,
        operation_id: Uuid,
        node_id: Uuid,
        timestamp: OffsetDateTime,
        command: AgentCommand,
        secret_key: &str,
    ) -> Result<Self, AgentProtocolError> {
        let payload = serde_json::to_string(&command)
            .map_err(|e| AgentProtocolError::SerializationError(e.to_string()))?;

        if payload.len() > MAX_PAYLOAD_SIZE_BYTES {
            return Err(AgentProtocolError::PayloadTooLarge {
                size: payload.len(),
                max: MAX_PAYLOAD_SIZE_BYTES,
            });
        }

        let signature = compute_signature(
            &version,
            &operation_id,
            &node_id,
            &timestamp,
            &payload,
            secret_key,
        )?;

        Ok(Self {
            version,
            operation_id,
            node_id,
            timestamp,
            payload,
            signature,
        })
    }
}

pub fn compute_signature(
    version: &str,
    operation_id: &Uuid,
    node_id: &Uuid,
    timestamp: &OffsetDateTime,
    payload: &str,
    secret_key: &str,
) -> Result<String, AgentProtocolError> {
    let mut mac = HmacSha256::new_from_slice(secret_key.as_bytes())
        .map_err(|_| AgentProtocolError::InvalidSignature)?;

    let timestamp_str = timestamp
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();

    mac.update(version.as_bytes());
    mac.update(operation_id.as_bytes());
    mac.update(node_id.as_bytes());
    mac.update(timestamp_str.as_bytes());
    mac.update(payload.as_bytes());

    Ok(hex::encode(mac.finalize().into_bytes()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeCredentials {
    pub node_id: Uuid,
    pub secret_key: String,
    pub epoch: u64,
}

impl NodeCredentials {
    pub fn generate(node_id: Uuid) -> Self {
        let mut key_bytes = [0u8; 32];
        for b in &mut key_bytes {
            *b = rand::random::<u8>();
        }
        Self {
            node_id,
            secret_key: hex::encode(key_bytes),
            epoch: 1,
        }
    }
}

#[derive(Debug, Default)]
pub struct NodeTokenManager {
    nodes: HashMap<Uuid, (NodeCredentials, bool)>, // (creds, is_active)
}

impl NodeTokenManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn issue_token(&mut self, node_id: Uuid) -> NodeCredentials {
        let creds = NodeCredentials::generate(node_id);
        self.nodes.insert(node_id, (creds.clone(), true));
        creds
    }

    pub fn rotate_token(&mut self, node_id: Uuid) -> Option<NodeCredentials> {
        if let Some((old_creds, is_active)) = self.nodes.get_mut(&node_id) {
            if !*is_active {
                return None;
            }
            let mut new_creds = NodeCredentials::generate(node_id);
            new_creds.epoch = old_creds.epoch + 1;
            *old_creds = new_creds.clone();
            Some(new_creds)
        } else {
            None
        }
    }

    pub fn revoke_node(&mut self, node_id: Uuid) {
        if let Some((_, is_active)) = self.nodes.get_mut(&node_id) {
            *is_active = false;
        }
    }

    pub fn is_valid(&self, creds: &NodeCredentials) -> bool {
        if let Some((current, active)) = self.nodes.get(&creds.node_id) {
            *active && current.epoch == creds.epoch && current.secret_key == creds.secret_key
        } else {
            false
        }
    }

    pub fn get_credentials(&self, node_id: &Uuid) -> Result<NodeCredentials, AgentProtocolError> {
        match self.nodes.get(node_id) {
            Some((_, false)) => Err(AgentProtocolError::TokenRevoked(*node_id)),
            Some((creds, true)) => Ok(creds.clone()),
            None => Err(AgentProtocolError::NodeNotFound(*node_id)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentProtocolValidator {
    max_skew: Duration,
    processed_operations: Arc<Mutex<HashMap<Uuid, Instant>>>,
    credentials_store: Arc<Mutex<HashMap<Uuid, NodeCredentials>>>,
    revoked_nodes: Arc<Mutex<HashSet<Uuid>>>,
}

impl AgentProtocolValidator {
    pub fn new(max_skew: Duration) -> Self {
        Self {
            max_skew,
            processed_operations: Arc::new(Mutex::new(HashMap::new())),
            credentials_store: Arc::new(Mutex::new(HashMap::new())),
            revoked_nodes: Arc::new(Mutex::new(HashSet::new())),
        }
    }

    pub fn register_credentials(&mut self, creds: NodeCredentials) {
        let mut store = self.credentials_store.lock().unwrap();
        store.insert(creds.node_id, creds);
    }

    pub fn revoke_node(&mut self, node_id: Uuid) {
        let mut revoked = self.revoked_nodes.lock().unwrap();
        revoked.insert(node_id);
    }

    pub fn validate_and_unpack(
        &self,
        envelope: &AgentEnvelope,
    ) -> Result<AgentCommand, AgentProtocolError> {
        // 1. Version check
        if envelope.version != AGENT_PROTOCOL_VERSION {
            return Err(AgentProtocolError::VersionMismatch {
                expected: AGENT_PROTOCOL_VERSION.to_string(),
                actual: envelope.version.clone(),
            });
        }

        // 2. Payload size check
        if envelope.payload.len() > MAX_PAYLOAD_SIZE_BYTES {
            return Err(AgentProtocolError::PayloadTooLarge {
                size: envelope.payload.len(),
                max: MAX_PAYLOAD_SIZE_BYTES,
            });
        }

        // 3. Timestamp drift check
        let now = OffsetDateTime::now_utc();
        let skew = (now - envelope.timestamp).whole_seconds();
        if skew.abs() > self.max_skew.as_secs() as i64 {
            return Err(AgentProtocolError::TimestampExpired { skew_secs: skew });
        }

        // 4. Revocation check
        {
            let revoked = self.revoked_nodes.lock().unwrap();
            if revoked.contains(&envelope.node_id) {
                return Err(AgentProtocolError::TokenRevoked(envelope.node_id));
            }
        }

        // 5. Credential lookup
        let creds = {
            let store = self.credentials_store.lock().unwrap();
            store
                .get(&envelope.node_id)
                .cloned()
                .ok_or(AgentProtocolError::NodeNotFound(envelope.node_id))?
        };

        // 6. Signature check
        let expected_signature = compute_signature(
            &envelope.version,
            &envelope.operation_id,
            &envelope.node_id,
            &envelope.timestamp,
            &envelope.payload,
            &creds.secret_key,
        )?;

        if envelope.signature.len() != expected_signature.len()
            || envelope
                .signature
                .as_bytes()
                .ct_eq(expected_signature.as_bytes())
                .unwrap_u8()
                != 1
        {
            return Err(AgentProtocolError::InvalidSignature);
        }

        // 7. Replay / deduplication check
        {
            let mut ops = self.processed_operations.lock().unwrap();
            // Prune expired ops older than 2x max_skew
            let now_inst = Instant::now();
            let prune_limit = self.max_skew * 2;
            ops.retain(|_, time| now_inst.duration_since(*time) < prune_limit);

            if ops.contains_key(&envelope.operation_id) {
                return Err(AgentProtocolError::ReplayDetected(envelope.operation_id));
            }
            ops.insert(envelope.operation_id, now_inst);
        }

        // 8. Deserialize command
        let command: AgentCommand = serde_json::from_str(&envelope.payload)
            .map_err(|e| AgentProtocolError::SerializationError(e.to_string()))?;

        // 9. Safety inspection of command
        self.inspect_safety(&command)?;

        Ok(command)
    }

    fn inspect_safety(&self, command: &AgentCommand) -> Result<(), AgentProtocolError> {
        match command {
            AgentCommand::CreateRelease(cmd) => {
                let path = Path::new(&cmd.image_path);
                for component in path.components() {
                    if let std::path::Component::ParentDir = component {
                        return Err(AgentProtocolError::ForbiddenCommand(
                            "Directory traversal '..' is prohibited in artifact image paths"
                                .to_string(),
                        ));
                    }
                }
                let forbidden_prefixes = ["/etc", "/sys", "/proc", "/dev", "/root"];
                for prefix in forbidden_prefixes {
                    if cmd.image_path.starts_with(prefix) {
                        return Err(AgentProtocolError::ForbiddenCommand(format!(
                            "Access to system path {} is prohibited",
                            prefix
                        )));
                    }
                }
            }
            AgentCommand::RegisterNode(cmd)
                if cmd.hostname.is_empty() || cmd.endpoint.is_empty() =>
            {
                return Err(AgentProtocolError::ForbiddenCommand(
                    "Hostname and endpoint cannot be empty".to_string(),
                ));
            }
            _ => {}
        }
        Ok(())
    }
}
