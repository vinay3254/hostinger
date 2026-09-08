use crate::model::{Deployment, PlatformState, Project};
use anyhow::{Context, Result};
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct StateStore {
    root: PathBuf,
}

impl StateStore {
    pub fn at(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn state_path(&self) -> PathBuf {
        self.root.join("state.json")
    }

    pub fn build_dir(&self, deployment_id: Uuid) -> PathBuf {
        self.root.join("builds").join(deployment_id.to_string())
    }

    pub fn load(&self) -> Result<PlatformState> {
        let path = self.state_path();
        if !path.exists() {
            return Ok(PlatformState::empty());
        }
        let file = File::open(&path)
            .with_context(|| format!("failed to open state file at {}", path.display()))?;
        let state: PlatformState = serde_json::from_reader(file)
            .with_context(|| format!("failed to deserialize state from {}", path.display()))?;
        Ok(state)
    }

    pub fn save(&self, state: &PlatformState) -> Result<()> {
        fs::create_dir_all(&self.root).with_context(|| {
            format!(
                "failed to create state root directory at {}",
                self.root.display()
            )
        })?;

        let target = self.state_path();
        let temp = target.with_extension("json.tmp");

        let write_res = (|| -> Result<()> {
            let mut file = File::create(&temp).with_context(|| {
                format!(
                    "failed to create temporary state file at {}",
                    temp.display()
                )
            })?;
            serde_json::to_writer_pretty(&mut file, state)
                .context("failed to serialize platform state to JSON")?;
            file.sync_all().with_context(|| {
                format!("failed to sync temporary state file at {}", temp.display())
            })?;
            drop(file);

            fs::rename(&temp, &target).with_context(|| {
                format!(
                    "failed to rename {} to {}",
                    temp.display(),
                    target.display()
                )
            })?;
            Ok(())
        })();

        if write_res.is_err() && (temp.exists() || temp.is_symlink()) {
            let _ = fs::remove_file(&temp);
        }

        write_res
    }

    pub fn project(&self, id: Uuid) -> Result<Project> {
        let state = self.load()?;
        state
            .projects
            .into_iter()
            .find(|p| p.id == id)
            .ok_or_else(|| anyhow::anyhow!("project not found: {id}"))
    }

    pub fn deployment(&self, id: Uuid) -> Result<Deployment> {
        let state = self.load()?;
        state
            .deployments
            .into_iter()
            .find(|d| d.id == id)
            .ok_or_else(|| anyhow::anyhow!("deployment not found: {id}"))
    }

    pub fn insert_project(&self, project: Project) -> Result<()> {
        let mut state = self.load()?;
        if state.projects.iter().any(|p| p.id == project.id) {
            anyhow::bail!("duplicate project ID: {}", project.id);
        }
        state.projects.push(project);
        self.save(&state)
    }

    pub fn insert_deployment(&self, deployment: Deployment) -> Result<()> {
        let mut state = self.load()?;
        if state.deployments.iter().any(|d| d.id == deployment.id) {
            anyhow::bail!("duplicate deployment ID: {}", deployment.id);
        }
        state.deployments.push(deployment);
        self.save(&state)
    }

    pub fn update_project(&self, project: Project) -> Result<()> {
        let mut state = self.load()?;
        let pos = state
            .projects
            .iter()
            .position(|p| p.id == project.id)
            .ok_or_else(|| anyhow::anyhow!("project not found: {}", project.id))?;
        state.projects[pos] = project;
        self.save(&state)
    }

    pub fn update_deployment(&self, deployment: Deployment) -> Result<()> {
        let mut state = self.load()?;
        let pos = state
            .deployments
            .iter()
            .position(|d| d.id == deployment.id)
            .ok_or_else(|| anyhow::anyhow!("deployment not found: {}", deployment.id))?;
        state.deployments[pos] = deployment;
        self.save(&state)
    }
}
