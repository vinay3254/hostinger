use crate::{
    providers::{
        OAuthTokenResponse, Provider, ProviderClient, PullRequestAction, PullRequestRef,
        RemoteCommit, RemoteRepository, RepositoryRef, SourceEvent, SourceEventKind,
    },
    Result,
};
use anyhow::bail;
use serde_json::Value;
use url::Url;

#[derive(Clone)]
pub struct GitHubClient {
    client_id: String,
    client_secret: String,
    api_base_url: String,
    auth_base_url: String,
    http: reqwest::Client,
}

impl GitHubClient {
    pub fn new(client_id: String, client_secret: String) -> Self {
        Self::with_base_urls(
            client_id,
            client_secret,
            "https://api.github.com".into(),
            "https://github.com".into(),
        )
    }

    pub fn with_base_urls(
        client_id: String,
        client_secret: String,
        api_base_url: String,
        auth_base_url: String,
    ) -> Self {
        let http = reqwest::Client::builder()
            .user_agent("deploy-platform")
            .build()
            .unwrap_or_default();
        Self {
            client_id,
            client_secret,
            api_base_url,
            auth_base_url,
            http,
        }
    }
}

fn url_encode(s: &str) -> String {
    url::form_urlencoded::byte_serialize(s.as_bytes()).collect()
}

#[async_trait::async_trait]
impl ProviderClient for GitHubClient {
    fn provider(&self) -> Provider {
        Provider::GitHub
    }

    fn begin_authorization(&self, state: &str, redirect_uri: &str) -> String {
        format!(
            "{}/login/oauth/authorize?client_id={}&redirect_uri={}&state={}&scope=repo,read:user",
            self.auth_base_url,
            url_encode(&self.client_id),
            url_encode(redirect_uri),
            url_encode(state),
        )
    }

    async fn complete_authorization(
        &self,
        code: &str,
        redirect_uri: &str,
    ) -> Result<OAuthTokenResponse> {
        let token_url = format!("{}/login/oauth/access_token", self.auth_base_url);
        let resp = self
            .http
            .post(&token_url)
            .header("Accept", "application/json")
            .json(&serde_json::json!({
                "client_id": self.client_id,
                "client_secret": self.client_secret,
                "code": code,
                "redirect_uri": redirect_uri,
            }))
            .send()
            .await?;

        let json: Value = resp.json().await?;
        let access_token = json
            .get("access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing access_token in response: {json}"))?
            .to_string();
        let refresh_token = json
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let expires_in = json.get("expires_in").and_then(|v| v.as_i64());

        let user_resp = self
            .http
            .get(format!("{}/user", self.api_base_url))
            .bearer_auth(&access_token)
            .send()
            .await?;
        let user_json: Value = user_resp.json().await?;
        let external_user_id = user_json
            .get("id")
            .map(|v| v.to_string())
            .ok_or_else(|| anyhow::anyhow!("missing user id in github /user response"))?;

        Ok(OAuthTokenResponse {
            access_token,
            refresh_token,
            expires_in,
            external_user_id,
        })
    }

    async fn list_repositories(&self, access_token: &str) -> Result<Vec<RemoteRepository>> {
        let url = format!("{}/user/repos?per_page=100&sort=updated", self.api_base_url);
        let resp = self.http.get(&url).bearer_auth(access_token).send().await?;
        let items: Vec<Value> = resp.json().await?;

        let mut repos = Vec::new();
        for item in items {
            if let (Some(id), Some(full_name), Some(clone_url_str)) = (
                item.get("id"),
                item.get("full_name").and_then(|v| v.as_str()),
                item.get("clone_url").and_then(|v| v.as_str()),
            ) {
                let default_branch = item
                    .get("default_branch")
                    .and_then(|v| v.as_str())
                    .unwrap_or("main")
                    .to_string();
                if let Ok(clone_url) = Url::parse(clone_url_str) {
                    repos.push(RemoteRepository {
                        external_id: id.to_string(),
                        full_name: full_name.to_string(),
                        clone_url,
                        default_branch,
                    });
                }
            }
        }
        Ok(repos)
    }

    async fn get_commit(
        &self,
        access_token: &str,
        repo_id: &str,
        commit_sha: &str,
    ) -> Result<RemoteCommit> {
        let url = format!(
            "{}/repositories/{}/commits/{}",
            self.api_base_url, repo_id, commit_sha
        );
        let resp = self.http.get(&url).bearer_auth(access_token).send().await?;
        let json: Value = resp.json().await?;
        let sha = json
            .get("sha")
            .and_then(|v| v.as_str())
            .unwrap_or(commit_sha)
            .to_string();
        let message = json
            .get("commit")
            .and_then(|c| c.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string();
        let author = json
            .get("commit")
            .and_then(|c| c.get("author"))
            .and_then(|a| a.get("name"))
            .and_then(|n| n.as_str())
            .unwrap_or("Unknown")
            .to_string();
        Ok(RemoteCommit {
            sha,
            message,
            author,
        })
    }

    async fn create_webhook(
        &self,
        access_token: &str,
        repo_id: &str,
        webhook_url: &str,
        secret: &str,
    ) -> Result<String> {
        let url = format!("{}/repositories/{}/hooks", self.api_base_url, repo_id);
        let resp = self
            .http
            .post(&url)
            .bearer_auth(access_token)
            .json(&serde_json::json!({
                "name": "web",
                "active": true,
                "events": ["push", "pull_request"],
                "config": {
                    "url": webhook_url,
                    "content_type": "json",
                    "secret": secret
                }
            }))
            .send()
            .await?;
        let json: Value = resp.json().await?;
        let id = json
            .get("id")
            .map(|v| v.to_string())
            .ok_or_else(|| anyhow::anyhow!("failed to create webhook: {json}"))?;
        Ok(id)
    }
}

pub fn parse_github_webhook(
    event_type: &str,
    delivery_id: &str,
    payload: &[u8],
) -> Result<SourceEvent> {
    let json: Value = serde_json::from_slice(payload)?;

    let repo_val = json
        .get("repository")
        .ok_or_else(|| anyhow::anyhow!("missing repository object"))?;
    let repo_id = repo_val
        .get("id")
        .ok_or_else(|| anyhow::anyhow!("missing repository id"))?;
    let external_id = if repo_id.is_number() {
        repo_id.as_i64().unwrap().to_string()
    } else if repo_id.is_string() {
        repo_id.as_str().unwrap().to_string()
    } else {
        bail!("invalid repository id");
    };

    let clone_url_str = repo_val
        .get("clone_url")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing clone_url"))?;
    let clone_url = Url::parse(clone_url_str)?;
    let default_branch = repo_val
        .get("default_branch")
        .and_then(|v| v.as_str())
        .unwrap_or("main")
        .to_string();

    let repository = RepositoryRef {
        provider: Provider::GitHub,
        external_id,
        clone_url,
        default_branch,
    };

    match event_type {
        "push" => {
            let after = json
                .get("after")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("missing after/commit_sha in push event"))?;
            if after.chars().all(|c| c == '0') {
                bail!("branch deletion push ignored");
            }
            let ref_str = json
                .get("ref")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("missing ref in push event"))?;
            let branch = ref_str
                .strip_prefix("refs/heads/")
                .unwrap_or(ref_str)
                .to_string();

            Ok(SourceEvent {
                delivery_id: delivery_id.to_string(),
                repository,
                kind: SourceEventKind::Push,
                commit_sha: after.to_string(),
                branch: Some(branch),
                pull_request: None,
            })
        }
        "pull_request" => {
            let action_str = json
                .get("action")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("missing action in pull_request event"))?;
            let (kind, action) = match action_str {
                "opened" => (
                    SourceEventKind::PullRequestOpened,
                    PullRequestAction::Opened,
                ),
                "synchronize" => (
                    SourceEventKind::PullRequestUpdated,
                    PullRequestAction::Synchronize,
                ),
                "closed" => (
                    SourceEventKind::PullRequestClosed,
                    PullRequestAction::Closed,
                ),
                "reopened" => (
                    SourceEventKind::PullRequestOpened,
                    PullRequestAction::Reopened,
                ),
                other => bail!("unsupported pull_request action: {other}"),
            };
            let number = json
                .get("number")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| anyhow::anyhow!("missing pr number"))?;
            let pr_val = json
                .get("pull_request")
                .ok_or_else(|| anyhow::anyhow!("missing pull_request object"))?;
            let head_sha = pr_val
                .get("head")
                .and_then(|h| h.get("sha"))
                .and_then(|s| s.as_str())
                .ok_or_else(|| anyhow::anyhow!("missing head.sha in pull_request"))?;
            let base_branch = pr_val
                .get("base")
                .and_then(|b| b.get("ref"))
                .and_then(|r| r.as_str())
                .unwrap_or("main")
                .to_string();
            let branch = pr_val
                .get("head")
                .and_then(|h| h.get("ref"))
                .and_then(|r| r.as_str())
                .map(|s| s.to_string());

            Ok(SourceEvent {
                delivery_id: delivery_id.to_string(),
                repository,
                kind,
                commit_sha: head_sha.to_string(),
                branch,
                pull_request: Some(PullRequestRef {
                    number,
                    head_sha: head_sha.to_string(),
                    base_branch,
                    action,
                }),
            })
        }
        other => bail!("unsupported github event type: {other}"),
    }
}
