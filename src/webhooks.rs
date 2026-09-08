use crate::{
    api::{ApiError, ApiState},
    providers::{parse_github_webhook, parse_gitlab_webhook, Provider},
    source_events::SourceEventRecord,
};
use axum::{
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use hmac::{Hmac, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;
use time::OffsetDateTime;
use uuid::Uuid;

pub fn verify_github_signature(secret: &str, signature_header: Option<&str>, body: &[u8]) -> bool {
    let Some(sig_str) = signature_header else {
        return false;
    };
    let Some(hex_sig) = sig_str.strip_prefix("sha256=") else {
        return false;
    };
    let Ok(expected_bytes) = hex::decode(hex_sig) else {
        return false;
    };
    let Ok(mut mac) = Hmac::<Sha256>::new_from_slice(secret.as_bytes()) else {
        return false;
    };
    mac.update(body);
    mac.verify_slice(&expected_bytes).is_ok()
}

pub fn verify_gitlab_token(secret: &str, token_header: Option<&str>) -> bool {
    let Some(token) = token_header else {
        return false;
    };
    secret.as_bytes().ct_eq(token.as_bytes()).into()
}

pub async fn handle_webhook(
    State(state): State<ApiState>,
    Path((provider_str, project_id_str)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    let provider: Provider = provider_str
        .parse()
        .map_err(|e: anyhow::Error| ApiError::BadRequest(e.to_string()))?;
    let project_id = Uuid::parse_str(&project_id_str)
        .map_err(|_| ApiError::BadRequest("invalid project UUID".into()))?;

    let db = state
        .db
        .as_ref()
        .ok_or_else(|| ApiError::Internal("database not configured".into()))?;

    // Load project webhook_secret from database
    let query = "SELECT webhook_secret FROM projects WHERE id = $1";
    let row_opt = sqlx::query(query)
        .bind(project_id)
        .fetch_optional(db.pool())
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    let Some(row) = row_opt else {
        return Err(ApiError::NotFound("project not found".into()));
    };

    let secret: Option<String> = sqlx::Row::get(&row, "webhook_secret");
    let Some(secret) = secret else {
        return Err(ApiError::Unauthorized(
            "webhook secret not configured".into(),
        ));
    };

    // Verify signature / token
    let is_verified = match provider {
        Provider::GitHub => {
            let sig_header = headers
                .get("x-hub-signature-256")
                .and_then(|v| v.to_str().ok());
            verify_github_signature(&secret, sig_header, &body)
        }
        Provider::GitLab => {
            let token_header = headers.get("x-gitlab-token").and_then(|v| v.to_str().ok());
            verify_gitlab_token(&secret, token_header)
        }
    };

    if !is_verified {
        return Err(ApiError::Unauthorized(
            "invalid webhook signature or token".into(),
        ));
    }

    // Extract delivery ID
    let delivery_id = headers
        .get("x-github-delivery")
        .or_else(|| headers.get("x-gitlab-delivery"))
        .or_else(|| headers.get("x-gitlab-event-uuid"))
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| ApiError::BadRequest("missing webhook delivery ID header".into()))?;

    // Extract event type
    let event_type = headers
        .get("x-github-event")
        .or_else(|| headers.get("x-gitlab-event"))
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| ApiError::BadRequest("missing webhook event type header".into()))?;

    let now = OffsetDateTime::now_utc();

    // Deduplicate delivery ID
    let inserted = db
        .source_events()
        .record_delivery(&provider.to_string(), delivery_id, Some(project_id), now)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    if !inserted {
        return Ok(StatusCode::ACCEPTED);
    }

    // Parse payload into normalized SourceEvent
    let source_event = match provider {
        Provider::GitHub => parse_github_webhook(event_type, delivery_id, &body)
            .map_err(|e| ApiError::BadRequest(e.to_string()))?,
        Provider::GitLab => parse_gitlab_webhook(event_type, delivery_id, &body)
            .map_err(|e| ApiError::BadRequest(e.to_string()))?,
    };

    // Persist durable SourceEventRecord
    let record = SourceEventRecord::from_event(Uuid::new_v4(), project_id, &source_event, now);
    db.source_events()
        .record_event(&record)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;

    Ok(StatusCode::OK)
}
