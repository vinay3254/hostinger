use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;
use time::OffsetDateTime;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HealthPolicy {
    pub path: String,
    pub port: u16,
    pub interval: Duration,
    pub timeout: Duration,
    pub attempts: u32,
    pub expected_status: u16,
    pub grace_period: Duration,
    pub expected_body_contains: Option<String>,
    pub expected_headers: HashMap<String, String>,
}

impl Default for HealthPolicy {
    fn default() -> Self {
        Self {
            path: "/health".to_string(),
            port: 8080,
            interval: Duration::from_millis(100),
            timeout: Duration::from_secs(2),
            attempts: 5,
            expected_status: 200,
            grace_period: Duration::ZERO,
            expected_body_contains: None,
            expected_headers: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HealthAttemptRecord {
    pub attempt: u32,
    pub status_code: Option<u16>,
    pub is_healthy: bool,
    pub error_message: Option<String>,
    pub latency_ms: u64,
    #[serde(with = "time::serde::rfc3339")]
    pub timestamp: OffsetDateTime,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HealthCheckResult {
    pub is_healthy: bool,
    pub attempts: Vec<HealthAttemptRecord>,
    pub final_error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ProbeResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: String,
}

#[async_trait::async_trait]
pub trait HealthProbeClient: Send + Sync {
    async fn probe(&self, url: &str, timeout: Duration) -> Result<ProbeResponse>;
}

pub struct ReqwestHealthProbeClient {
    client: reqwest::Client,
}

impl Default for ReqwestHealthProbeClient {
    fn default() -> Self {
        Self::new()
    }
}

impl ReqwestHealthProbeClient {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .unwrap_or_default(),
        }
    }
}

#[async_trait::async_trait]
impl HealthProbeClient for ReqwestHealthProbeClient {
    async fn probe(&self, url: &str, timeout: Duration) -> Result<ProbeResponse> {
        let resp = self.client.get(url).timeout(timeout).send().await?;
        let status = resp.status().as_u16();
        let mut headers = HashMap::new();
        for (k, v) in resp.headers() {
            if let Ok(val) = v.to_str() {
                headers.insert(k.as_str().to_lowercase(), val.to_string());
            }
        }
        let body = resp.text().await.unwrap_or_default();
        Ok(ProbeResponse {
            status,
            headers,
            body,
        })
    }
}

#[derive(Default, Clone)]
pub struct MockHealthProbeClient {
    pub queued_responses: std::sync::Arc<std::sync::Mutex<Vec<Result<ProbeResponse, String>>>>,
    pub probed_urls: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}

impl MockHealthProbeClient {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_responses(responses: Vec<Result<ProbeResponse, String>>) -> Self {
        Self {
            queued_responses: std::sync::Arc::new(std::sync::Mutex::new(responses)),
            probed_urls: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    pub fn with_success() -> Self {
        Self::with_responses(vec![Ok(ProbeResponse {
            status: 200,
            headers: HashMap::new(),
            body: "ok".into(),
        })])
    }

    pub fn with_failures_then_success(failures: usize) -> Self {
        let mut responses = Vec::new();
        for _ in 0..failures {
            responses.push(Err("connection refused".into()));
        }
        responses.push(Ok(ProbeResponse {
            status: 200,
            headers: HashMap::new(),
            body: "ok".into(),
        }));
        Self::with_responses(responses)
    }

    pub fn with_status_sequence(statuses: Vec<u16>) -> Self {
        let responses = statuses
            .into_iter()
            .map(|s| {
                Ok(ProbeResponse {
                    status: s,
                    headers: HashMap::new(),
                    body: format!("status-{s}"),
                })
            })
            .collect();
        Self::with_responses(responses)
    }
}

#[async_trait::async_trait]
impl HealthProbeClient for MockHealthProbeClient {
    async fn probe(&self, url: &str, _timeout: Duration) -> Result<ProbeResponse> {
        self.probed_urls.lock().unwrap().push(url.to_string());
        let mut queue = self.queued_responses.lock().unwrap();
        if queue.is_empty() {
            return Err(anyhow!("no mock responses remaining"));
        }
        let next = queue.remove(0);
        match next {
            Ok(resp) => Ok(resp),
            Err(e) => Err(anyhow!(e)),
        }
    }
}

pub async fn wait_until_healthy<C: HealthProbeClient + ?Sized>(
    client: &C,
    base_url: &str,
    policy: &HealthPolicy,
) -> HealthCheckResult {
    if policy.grace_period > Duration::ZERO {
        tokio::time::sleep(policy.grace_period).await;
    }

    let base = base_url.trim_end_matches('/');
    let path = if policy.path.starts_with('/') {
        policy.path.clone()
    } else {
        format!("/{}", policy.path)
    };
    let probe_url = format!("{base}{path}");

    let mut attempts = Vec::new();
    let max_attempts = policy.attempts.max(1);

    for attempt_num in 1..=max_attempts {
        let start = tokio::time::Instant::now();
        let timestamp = OffsetDateTime::now_utc();
        let probe_res = client.probe(&probe_url, policy.timeout).await;
        let latency_ms = start.elapsed().as_millis() as u64;

        match probe_res {
            Ok(resp) => {
                let mut is_healthy = true;
                let mut error_msg = None;

                if resp.status != policy.expected_status {
                    is_healthy = false;
                    error_msg = Some(format!(
                        "expected HTTP status {}, got {}",
                        policy.expected_status, resp.status
                    ));
                } else if let Some(ref required_text) = policy.expected_body_contains {
                    if !resp.body.contains(required_text) {
                        is_healthy = false;
                        error_msg = Some(format!(
                            "response body does not contain expected substring '{}'",
                            required_text
                        ));
                    }
                }

                if is_healthy {
                    for (header_key, expected_val) in &policy.expected_headers {
                        let header_key_lower = header_key.to_lowercase();
                        match resp.headers.get(&header_key_lower) {
                            Some(actual_val) if actual_val == expected_val => {}
                            Some(actual_val) => {
                                is_healthy = false;
                                error_msg = Some(format!(
                                    "header '{}' expected '{}', got '{}'",
                                    header_key, expected_val, actual_val
                                ));
                                break;
                            }
                            None => {
                                is_healthy = false;
                                error_msg =
                                    Some(format!("missing expected header '{}'", header_key));
                                break;
                            }
                        }
                    }
                }

                attempts.push(HealthAttemptRecord {
                    attempt: attempt_num,
                    status_code: Some(resp.status),
                    is_healthy,
                    error_message: error_msg.clone(),
                    latency_ms,
                    timestamp,
                });

                if is_healthy {
                    return HealthCheckResult {
                        is_healthy: true,
                        attempts,
                        final_error: None,
                    };
                }
            }
            Err(err) => {
                let err_str = err.to_string();
                attempts.push(HealthAttemptRecord {
                    attempt: attempt_num,
                    status_code: None,
                    is_healthy: false,
                    error_message: Some(err_str),
                    latency_ms,
                    timestamp,
                });
            }
        }

        if attempt_num < max_attempts && policy.interval > Duration::ZERO {
            tokio::time::sleep(policy.interval).await;
        }
    }

    let final_error = attempts
        .last()
        .and_then(|a| a.error_message.clone())
        .unwrap_or_else(|| "health check failed all attempts".to_string());

    HealthCheckResult {
        is_healthy: false,
        attempts,
        final_error: Some(final_error),
    }
}
