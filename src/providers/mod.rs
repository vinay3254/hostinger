pub mod crypto;
pub mod github;
pub mod gitlab;

use crate::Result;
pub use github::{parse_github_webhook, GitHubClient};
pub use gitlab::{parse_gitlab_webhook, GitLabClient};
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    GitHub,
    GitLab,
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::GitHub => write!(f, "github"),
            Self::GitLab => write!(f, "gitlab"),
        }
    }
}

impl std::str::FromStr for Provider {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "github" => Ok(Self::GitHub),
            "gitlab" => Ok(Self::GitLab),
            other => Err(anyhow::anyhow!("unknown provider: {other}")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RepositoryRef {
    pub provider: Provider,
    pub external_id: String,
    pub clone_url: Url,
    pub default_branch: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceEventKind {
    Push,
    PullRequestOpened,
    PullRequestUpdated,
    PullRequestClosed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PullRequestAction {
    Opened,
    Synchronize,
    Closed,
    Reopened,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PullRequestRef {
    pub number: u64,
    pub head_sha: String,
    pub base_branch: String,
    pub action: PullRequestAction,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SourceEvent {
    pub delivery_id: String,
    pub repository: RepositoryRef,
    pub kind: SourceEventKind,
    pub commit_sha: String,
    pub branch: Option<String>,
    pub pull_request: Option<PullRequestRef>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OAuthTokenResponse {
    pub access_token: String,
    pub refresh_token: Option<String>,
    pub expires_in: Option<i64>,
    pub external_user_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct RemoteRepository {
    pub external_id: String,
    pub full_name: String,
    pub clone_url: Url,
    pub default_branch: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RemoteCommit {
    pub sha: String,
    pub message: String,
    pub author: String,
}

#[async_trait::async_trait]
pub trait ProviderClient: Send + Sync {
    fn provider(&self) -> Provider;
    fn begin_authorization(&self, state: &str, redirect_uri: &str) -> String;
    async fn complete_authorization(
        &self,
        code: &str,
        redirect_uri: &str,
    ) -> Result<OAuthTokenResponse>;
    async fn list_repositories(&self, access_token: &str) -> Result<Vec<RemoteRepository>>;
    async fn get_commit(
        &self,
        access_token: &str,
        repo_id: &str,
        commit_sha: &str,
    ) -> Result<RemoteCommit>;
    async fn create_webhook(
        &self,
        access_token: &str,
        repo_id: &str,
        webhook_url: &str,
        secret: &str,
    ) -> Result<String>;
}
