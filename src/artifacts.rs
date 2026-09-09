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
            let tmp_path = target_dir.join(format!("artifact.tmp.{}", Uuid::new_v4().simple()));
            fs::copy(source_archive, &tmp_path).with_context(|| {
                format!(
                    "failed to copy artifact from {} to {}",
                    source_archive.display(),
                    tmp_path.display()
                )
            })?;
            fs::rename(&tmp_path, &target_path).with_context(|| {
                format!(
                    "failed to rename atomic artifact from {} to {}",
                    tmp_path.display(),
                    target_path.display()
                )
            })?;
        }

        let metadata = fs::metadata(&target_path)
            .with_context(|| format!("failed to read metadata of {}", target_path.display()))?;
        let size_bytes = metadata.len();

        let sha256 = self.compute_sha256(&target_path)?;

        Ok(ArtifactMeta {
            id: Uuid::new_v4(),
            deployment_id,
            file_path: target_path,
            size_bytes,
            sha256,
            created_at: OffsetDateTime::now_utc(),
        })
    }

    pub fn compute_sha256(&self, path: &Path) -> Result<String> {
        let mut file = File::open(path)
            .with_context(|| format!("failed to open file for checksum: {}", path.display()))?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 8192];
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            hasher.update(&buffer[..n]);
        }
        Ok(hex::encode(hasher.finalize()))
    }

    pub fn verify_checksum(&self, path: &Path, expected_sha256: &str) -> Result<()> {
        let actual = self.compute_sha256(path)?;
        if actual.eq_ignore_ascii_case(expected_sha256) {
            Ok(())
        } else {
            anyhow::bail!(
                "checksum mismatch for {}: expected {}, got {}",
                path.display(),
                expected_sha256,
                actual
            );
        }
    }
}
