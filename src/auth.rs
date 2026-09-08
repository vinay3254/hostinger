use crate::db::Database;
use anyhow::{Context, Result};
use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use rand::RngCore;
use sha2::{Digest, Sha256};
use sqlx::Row;
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

pub fn hash_password(password: &str) -> Result<String> {
    let salt = SaltString::generate(&mut OsRng);
    let argon2 = Argon2::default();
    let hash = argon2
        .hash_password(password.as_bytes(), &salt)
        .map_err(|e| anyhow::anyhow!("failed to hash password: {e}"))?
        .to_string();
    Ok(hash)
}

pub fn verify_password(password: &str, hash: &str) -> Result<bool> {
    let parsed_hash = PasswordHash::new(hash)
        .map_err(|e| anyhow::anyhow!("invalid password hash format: {e}"))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed_hash)
        .is_ok())
}

fn hash_session_token(token: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(token.as_bytes());
    hex::encode(hasher.finalize())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthContext {
    pub user_id: Uuid,
    pub session_id: Option<Uuid>,
    pub scopes: Vec<String>,
}

impl AuthContext {
    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == "*" || s == scope)
    }
}

#[derive(Debug, Clone)]
pub struct ProjectAccess<'a> {
    pub auth: &'a AuthContext,
    pub project_user_id: Option<Uuid>,
}

impl<'a> ProjectAccess<'a> {
    pub fn new(auth: &'a AuthContext, project_user_id: Option<Uuid>) -> Self {
        Self {
            auth,
            project_user_id,
        }
    }

    pub fn is_owner(&self) -> bool {
        match self.project_user_id {
            Some(owner_id) => self.auth.user_id == owner_id || self.auth.user_id.is_nil(),
            None => true,
        }
    }

    pub fn can_read(&self) -> bool {
        self.is_owner() && (self.auth.has_scope("*") || self.auth.has_scope("project:read"))
    }

    pub fn can_mutate(&self) -> bool {
        self.is_owner() && (self.auth.has_scope("*") || self.auth.has_scope("project:write"))
    }

    pub fn can_operate(&self) -> bool {
        self.is_owner()
            && (self.auth.has_scope("*")
                || self.auth.has_scope("deploy:write")
                || self.auth.has_scope("deploy:operate"))
    }
}

#[derive(Clone)]
pub struct AuditService {
    db: Option<Database>,
}

impl AuditService {
    pub fn new(db: Option<Database>) -> Self {
        Self { db }
    }

    pub async fn record(
        &self,
        actor_id: Uuid,
        action: &str,
        target_type: &str,
        target_id: Uuid,
        metadata: serde_json::Value,
    ) -> Result<()> {
        if let Some(db) = &self.db {
            let record = crate::repository::CreateAuditEventRecord {
                id: Uuid::new_v4(),
                user_id: Some(actor_id),
                action: action.to_string(),
                target_type: target_type.to_string(),
                target_id,
                metadata,
                occurred_at: OffsetDateTime::now_utc(),
            };
            let mut audits = db.audits();
            audits.record(record).await?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ApiTokenSummary {
    pub id: Uuid,
    pub name: String,
    pub prefix: String,
    pub scopes: Vec<String>,
    pub revoked: bool,
    #[serde(with = "time::serde::rfc3339::option")]
    pub last_used_at: Option<OffsetDateTime>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct IssuedToken {
    pub id: Uuid,
    pub name: String,
    pub raw_token: String,
    pub prefix: String,
    pub scopes: Vec<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub id: Uuid,
    pub user_id: Uuid,
    pub token: String,
    pub expires_at: OffsetDateTime,
}

#[derive(Clone)]
pub struct AuthService {
    db: Database,
}

impl AuthService {
    pub fn new(db: Database) -> Self {
        Self { db }
    }

    pub fn db(&self) -> &Database {
        &self.db
    }

    pub async fn register(
        &self,
        email: &str,
        password: &str,
        name: &str,
    ) -> Result<crate::repository::UserRecord> {
        let user_id = Uuid::new_v4();
        let hash = hash_password(password)?;
        let now = OffsetDateTime::now_utc();
        let mut users = self.db.users();
        users.create_user(user_id, email, &hash, name, now).await
    }

    pub async fn login(
        &self,
        email: &str,
        password: &str,
        duration: Duration,
    ) -> Result<(crate::repository::UserRecord, SessionInfo)> {
        let mut users = self.db.users();
        let user = users.get_by_email(email).await?;
        if !verify_password(password, &user.password_hash)? {
            anyhow::bail!("invalid email or password");
        }
        let session = self.create_session(user.id, duration).await?;
        Ok((user, session))
    }

    pub async fn load_session(&self, session_id: Uuid) -> Result<Option<SessionInfo>> {
        let query = "SELECT id, user_id, expires_at FROM sessions WHERE id = $1";
        let row = sqlx::query(query)
            .bind(session_id)
            .fetch_optional(self.db.pool())
            .await
            .context("failed to load session")?;

        if let Some(r) = row {
            let expires_at: OffsetDateTime = r.get("expires_at");
            if expires_at < OffsetDateTime::now_utc() {
                let _ = self.revoke_session(session_id).await;
                return Ok(None);
            }
            Ok(Some(SessionInfo {
                id: r.get("id"),
                user_id: r.get("user_id"),
                token: String::new(),
                expires_at,
            }))
        } else {
            Ok(None)
        }
    }

    pub async fn list_api_tokens(&self, user_id: Uuid) -> Result<Vec<ApiTokenSummary>> {
        let query = "SELECT id, name, token_prefix, scopes, revoked, last_used_at, created_at FROM api_tokens WHERE user_id = $1 ORDER BY created_at DESC";
        let rows = sqlx::query(query)
            .bind(user_id)
            .fetch_all(self.db.pool())
            .await
            .context("failed to list api tokens")?;

        let mut tokens = Vec::new();
        for row in rows {
            tokens.push(ApiTokenSummary {
                id: row.get("id"),
                name: row.get("name"),
                prefix: row.get("token_prefix"),
                scopes: row.get("scopes"),
                revoked: row.get("revoked"),
                last_used_at: row.get("last_used_at"),
                created_at: row.get("created_at"),
            });
        }
        Ok(tokens)
    }

    pub async fn create_session(&self, user_id: Uuid, duration: Duration) -> Result<SessionInfo> {
        let session_id = Uuid::new_v4();
        let mut bytes = [0u8; 32];
        OsRng.fill_bytes(&mut bytes);
        let raw_token = hex::encode(bytes);
        let token_hash = hash_session_token(&raw_token);
        let now = OffsetDateTime::now_utc();
        let expires_at = now + duration;

        let query = r#"
            INSERT INTO sessions (id, user_id, token_hash, expires_at, created_at)
            VALUES ($1, $2, $3, $4, $5)
        "#;
        sqlx::query(query)
            .bind(session_id)
            .bind(user_id)
            .bind(token_hash)
            .bind(expires_at)
            .bind(now)
            .execute(self.db.pool())
            .await
            .context("failed to insert session")?;

        Ok(SessionInfo {
            id: session_id,
            user_id,
            token: raw_token,
            expires_at,
        })
    }

    pub async fn authenticate_session(&self, token: &str) -> Result<AuthContext> {
        let token_hash = hash_session_token(token);
        let query = "SELECT id, user_id, expires_at FROM sessions WHERE token_hash = $1";
        let row = sqlx::query(query)
            .bind(token_hash)
            .fetch_optional(self.db.pool())
            .await
            .context("failed to query session")?
            .ok_or_else(|| anyhow::anyhow!("session not found or invalid"))?;

        let expires_at: OffsetDateTime = row.get("expires_at");
        if expires_at < OffsetDateTime::now_utc() {
            let session_id: Uuid = row.get("id");
            let _ = self.revoke_session(session_id).await;
            anyhow::bail!("session has expired");
        }

        let user_id: Uuid = row.get("user_id");
        let session_id: Uuid = row.get("id");

        Ok(AuthContext {
            user_id,
            session_id: Some(session_id),
            scopes: vec!["*".into()],
        })
    }

    pub async fn revoke_session(&self, session_id: Uuid) -> Result<()> {
        let query = "DELETE FROM sessions WHERE id = $1";
        sqlx::query(query)
            .bind(session_id)
            .execute(self.db.pool())
            .await
            .context("failed to revoke session")?;
        Ok(())
    }

    pub async fn issue_api_token(
        &self,
        user_id: Uuid,
        name: &str,
        scopes: Vec<String>,
        expires_at: Option<OffsetDateTime>,
    ) -> Result<IssuedToken> {
        let token_id = Uuid::new_v4();
        let mut rand_bytes = [0u8; 20];
        OsRng.fill_bytes(&mut rand_bytes);
        let raw_token = format!("dp_{}", hex::encode(rand_bytes));
        let prefix = raw_token[..10].to_string();
        let token_hash = hash_password(&raw_token)?;
        let now = OffsetDateTime::now_utc();

        let query = r#"
            INSERT INTO api_tokens (id, user_id, name, token_prefix, token_hash, scopes, revoked, expires_at, created_at)
            VALUES ($1, $2, $3, $4, $5, $6, FALSE, $7, $8)
        "#;
        sqlx::query(query)
            .bind(token_id)
            .bind(user_id)
            .bind(name)
            .bind(&prefix)
            .bind(&token_hash)
            .bind(&scopes)
            .bind(expires_at)
            .bind(now)
            .execute(self.db.pool())
            .await
            .context("failed to insert api token")?;

        Ok(IssuedToken {
            id: token_id,
            name: name.to_string(),
            raw_token,
            prefix,
            scopes,
            created_at: now,
        })
    }

    pub async fn authenticate_api_token(&self, raw_token: &str) -> Result<AuthContext> {
        if raw_token.len() < 10 {
            anyhow::bail!("invalid token format");
        }
        let prefix = &raw_token[..10];

        let query = "SELECT id, user_id, token_hash, scopes, revoked, expires_at FROM api_tokens WHERE token_prefix = $1 AND revoked = FALSE";
        let rows = sqlx::query(query)
            .bind(prefix)
            .fetch_all(self.db.pool())
            .await
            .context("failed to query api tokens")?;

        for row in rows {
            let token_hash: String = row.get("token_hash");
            if verify_password(raw_token, &token_hash).unwrap_or(false) {
                let expires_at: Option<OffsetDateTime> = row.get("expires_at");
                if let Some(exp) = expires_at {
                    if exp < OffsetDateTime::now_utc() {
                        anyhow::bail!("api token has expired");
                    }
                }

                let id: Uuid = row.get("id");
                let now = OffsetDateTime::now_utc();
                let _ = sqlx::query("UPDATE api_tokens SET last_used_at = $2 WHERE id = $1")
                    .bind(id)
                    .bind(now)
                    .execute(self.db.pool())
                    .await;

                let user_id: Uuid = row.get("user_id");
                let scopes: Vec<String> = row.get("scopes");

                return Ok(AuthContext {
                    user_id,
                    session_id: None,
                    scopes,
                });
            }
        }

        anyhow::bail!("invalid api token");
    }

    pub async fn revoke_api_token(&self, user_id: Uuid, token_id: Uuid) -> Result<()> {
        let query = "UPDATE api_tokens SET revoked = TRUE WHERE id = $1 AND user_id = $2";
        let res = sqlx::query(query)
            .bind(token_id)
            .bind(user_id)
            .execute(self.db.pool())
            .await
            .context("failed to revoke api token")?;
        if res.rows_affected() == 0 {
            anyhow::bail!("token not found or unauthorized");
        }
        Ok(())
    }
}
