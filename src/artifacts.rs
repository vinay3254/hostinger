use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ArtifactMeta {
    pub id: Uuid,
    pub deployment_id: Uuid,
    pub file_path: PathBuf,
    pub size_bytes: u64,
    pub sha256: String,
    pub created_at: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct ArtifactStore {
    pub base_dir: PathBuf,
}

impl ArtifactStore {
    pub fn new(base_dir: impl Into<PathBuf>) -> Self {
        Self {
            base_dir: base_dir.into(),
        }
    }

    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }

    pub fn artifact_dir(&self, deployment_id: Uuid) -> PathBuf {
        self.base_dir.join(deployment_id.to_string())
    }

    pub fn artifact_path(&self, deployment_id: Uuid) -> PathBuf {
        self.artifact_dir(deployment_id).join("artifact.tar.gz")
    }

    pub fn store_artifact(
        &self,
        deployment_id: Uuid,
        source_archive: &Path,
    ) -> Result<ArtifactMeta> {
        let target_dir = self.artifact_dir(deployment_id);
        fs::create_dir_all(&target_dir).with_context(|| {
            format!(
                "failed to create artifact directory: {}",
                target_dir.display()
            )
        })?;

        let target_path = self.artifact_path(deployment_id);
        if source_archive != target_path {
            fs::copy(source_archive, &target_path).with_context(|| {
                format!(
                    "failed to copy artifact from {} to {}",
                    source_archive.display(),
                    target_path.display()
                )
            })?;
        }

        let metadata = fs::metadata(&target_path)
            .with_context(|| format!("failed to read metadata of {}", target_path.display()))?;
        let size_bytes = metadata.len();

        let mut file = File::open(&target_path)
            .with_context(|| format!("failed to open {}", target_path.display()))?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 8192];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
        let sha256 = hex::encode(hasher.finalize());

        Ok(ArtifactMeta {
            id: Uuid::new_v4(),
            deployment_id,
            file_path: target_path,
            size_bytes,
            sha256,
            created_at: OffsetDateTime::now_utc(),
        })
    }
}
