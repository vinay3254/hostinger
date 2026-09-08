use std::path::PathBuf;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Framework {
    Static,
    #[serde(rename = "node")]
    Node,
    #[serde(rename = "nextjs")]
    NextJs,
    #[serde(rename = "python")]
    Python,
    #[serde(rename = "docker")]
    Docker,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeploymentStatus {
    Pending,
    Queued,
    Building,
    Running,
    Failed,
    Stopped,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PlatformState {
    pub version: u8,
    pub projects: Vec<Project>,
    pub deployments: Vec<Deployment>,
}

impl PlatformState {
    pub fn empty() -> Self {
        Self {
            version: 1,
            projects: Vec::new(),
            deployments: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    #[serde(default)]
    pub user_id: Option<Uuid>,
    pub source_dir: PathBuf,
    pub base_image: PathBuf,
    pub server_command: Vec<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    pub active_deployment: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Deployment {
    pub id: Uuid,
    pub project_id: Uuid,
    pub framework: Framework,
    pub status: DeploymentStatus,
    pub image_path: Option<PathBuf>,
    pub container_id: Option<Uuid>,
    pub port: Option<u16>,
    pub url: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "option_time_rfc3339")]
    pub finished_at: Option<OffsetDateTime>,
    pub error: Option<String>,
}

mod option_time_rfc3339 {
    pub use time::serde::rfc3339::option::{deserialize, serialize};
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateProjectInput {
    pub name: String,
    pub source_dir: PathBuf,
    pub base_image: PathBuf,
    pub server_command: Vec<String>,
    pub user_id: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningContainer {
    pub id: Uuid,
    pub port: u16,
    pub url: String,
}
