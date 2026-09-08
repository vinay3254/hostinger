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
pub struct GitLabClient {
    client_id: String,
    client_secret: String,
    api_base_url: String,
    auth_base_url: String,
    http: reqwest::Client,
}

impl GitLabClient {
    pub fn new(client_id: String, client_secret: String) -> Self {
        Self::with_base_urls(
            client_id,
            client_secret,
            "https://gitlab.com".into(),
            "https://gitlab.com".into(),
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
impl ProviderClient for GitLabClient {
    fn provider(&self) -> Provider {
        Provider::GitLab
    }

    fn begin_authorization(&self, state: &str, redirect_uri: &str) -> String {
        format!(
            "{}/oauth/authorize?client_id={}&redirect_uri={}&response_type=code&state={}&scope=api,read_user",
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
        let token_url = format!("{}/oauth/token", self.auth_base_url);
        let resp = self
            .http
            .post(&token_url)
            .json(&serde_json::json!({
                "client_id": self.client_id,
                "client_secret": self.client_secret,
                "code": code,
                "grant_type": "authorization_code",
                "redirect_uri": redirect_uri,
            }))
            .send()
            .await?;

        let json: Value = resp.json().await?;
        let access_token = json
            .get("access_token")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("missing access_token in gitlab response: {json}"))?
            .to_string();
        let refresh_token = json
            .get("refresh_token")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let expires_in = json.get("expires_in").and_then(|v| v.as_i64());

        let user_resp = self
            .http
            .get(format!("{}/api/v4/user", self.api_base_url))
            .bearer_auth(&access_token)
            .send()
            .await?;
        let user_json: Value = user_resp.json().await?;
        let external_user_id = user_json
            .get("id")
            .map(|v| v.to_string())
            .ok_or_else(|| anyhow::anyhow!("missing user id in gitlab /user response"))?;

        Ok(OAuthTokenResponse {
            access_token,
            refresh_token,
            expires_in,
            external_user_id,
        })
    }

    async fn list_repositories(&self, access_token: &str) -> Result<Vec<RemoteRepository>> {
        let url = format!(
            "{}/api/v4/projects?membership=true&per_page=100&order_by=updated_at",
            self.api_base_url
        );
        let resp = self.http.get(&url).bearer_auth(access_token).send().await?;
        let items: Vec<Value> = resp.json().await?;

        let mut repos = Vec::new();
        for item in items {
            if let (Some(id), Some(full_name), Some(clone_url_str)) = (
                item.get("id"),
                item.get("path_with_namespace")
                    .or_else(|| item.get("name"))
                    .and_then(|v| v.as_str()),
                item.get("http_url_to_repo")
                    .or_else(|| item.get("git_http_url"))
                    .and_then(|v| v.as_str()),
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
            "{}/api/v4/projects/{}/repository/commits/{}",
            self.api_base_url,
            url_encode(repo_id),
            commit_sha
        );
        let resp = self.http.get(&url).bearer_auth(access_token).send().await?;
        let json: Value = resp.json().await?;
        let sha = json
            .get("id")
            .and_then(|v| v.as_str())
            .unwrap_or(commit_sha)
            .to_string();
        let message = json
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("")
            .to_string();
        let author = json
            .get("author_name")
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
        let url = format!(
            "{}/api/v4/projects/{}/hooks",
            self.api_base_url,
            url_encode(repo_id)
        );
        let resp = self
            .http
            .post(&url)
            .bearer_auth(access_token)
            .json(&serde_json::json!({
                "url": webhook_url,
                "token": secret,
                "push_events": true,
                "merge_requests_events": true
            }))
            .send()
            .await?;
        let json: Value = resp.json().await?;
        let id = json
            .get("id")
            .map(|v| v.to_string())
            .ok_or_else(|| anyhow::anyhow!("failed to create gitlab webhook: {json}"))?;
        Ok(id)
    }
}

pub fn parse_gitlab_webhook(
    event_type: &str,
    delivery_id: &str,
    payload: &[u8],
) -> Result<SourceEvent> {
    let json: Value = serde_json::from_slice(payload)?;

    let project_val = json
        .get("project")
        .ok_or_else(|| anyhow::anyhow!("missing project object"))?;
    let proj_id = project_val
        .get("id")
        .ok_or_else(|| anyhow::anyhow!("missing project id"))?;
    let external_id = if proj_id.is_number() {
        proj_id.as_i64().unwrap().to_string()
    } else if proj_id.is_string() {
        proj_id.as_str().unwrap().to_string()
    } else {
        bail!("invalid project id");
    };

    let clone_url_str = project_val
        .get("git_http_url")
        .or_else(|| project_val.get("http_url"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("missing git_http_url/http_url"))?;
    let clone_url = Url::parse(clone_url_str)?;
    let default_branch = project_val
        .get("default_branch")
        .and_then(|v| v.as_str())
        .unwrap_or("main")
        .to_string();

    let repository = RepositoryRef {
        provider: Provider::GitLab,
        external_id,
        clone_url,
        default_branch,
    };

    match event_type {
        "Push Hook" | "push" => {
            let commit_sha = json
                .get("checkout_sha")
                .or_else(|| json.get("after"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("missing checkout_sha/after in push event"))?;
            if commit_sha.chars().all(|c| c == '0') {
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
                commit_sha: commit_sha.to_string(),
                branch: Some(branch),
                pull_request: None,
            })
        }
        "Merge Request Hook" | "merge_request" => {
            let attrs = json
                .get("object_attributes")
                .ok_or_else(|| anyhow::anyhow!("missing object_attributes"))?;
            let action_str = attrs
                .get("action")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("missing action in merge_request"))?;
            let (kind, action) = match action_str {
                "open" | "reopen" => (
                    SourceEventKind::PullRequestOpened,
                    PullRequestAction::Opened,
                ),
                "update" => (
                    SourceEventKind::PullRequestUpdated,
                    PullRequestAction::Synchronize,
                ),
                "close" => (
                    SourceEventKind::PullRequestClosed,
                    PullRequestAction::Closed,
                ),
                other => bail!("unsupported merge request action: {other}"),
            };
            let iid = attrs
                .get("iid")
                .or_else(|| attrs.get("id"))
                .and_then(|v| v.as_u64())
                .ok_or_else(|| anyhow::anyhow!("missing iid/id in merge request"))?;
            let commit_sha = attrs
                .get("last_commit")
                .and_then(|c| c.get("id"))
                .and_then(|s| s.as_str())
                .ok_or_else(|| anyhow::anyhow!("missing last_commit.id in merge request"))?;
            let base_branch = attrs
                .get("target_branch")
                .and_then(|b| b.as_str())
                .unwrap_or("main")
                .to_string();
            let branch = attrs
                .get("source_branch")
                .and_then(|b| b.as_str())
                .map(|s| s.to_string());

            Ok(SourceEvent {
                delivery_id: delivery_id.to_string(),
                repository,
                kind,
                commit_sha: commit_sha.to_string(),
                branch,
                pull_request: Some(PullRequestRef {
                    number: iid,
                    head_sha: commit_sha.to_string(),
                    base_branch,
                    action,
                }),
            })
        }
        other => bail!("unsupported gitlab event type: {other}"),
    }
}
