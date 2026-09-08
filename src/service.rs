use crate::builder::ImageBuilder;
use crate::detector::detect_static_source;
use crate::model::{CreateProjectInput, Deployment, DeploymentStatus, Framework, Project};
use crate::runtime::Runtime;
use crate::store::StateStore;
use anyhow::{Context, Result};
use std::sync::Mutex;
use time::OffsetDateTime;
use uuid::Uuid;

pub trait PlatformService: Send + Sync {
    fn create_project(&self, input: CreateProjectInput) -> Result<Project>;
    fn project(&self, id: Uuid) -> Result<Project>;
    fn projects(&self) -> Result<Vec<Project>>;
    fn deploy(&self, project_id: Uuid) -> Result<Deployment>;
    fn deployments(&self, project_id: Uuid) -> Result<Vec<Deployment>>;
    fn deployment(&self, id: Uuid) -> Result<Deployment>;
    fn stop(&self, id: Uuid) -> Result<Deployment>;
    fn logs(&self, id: Uuid) -> Result<String>;
}

pub struct DeploymentService<R, B> {
    store: StateStore,
    runtime: Mutex<R>,
    builder: B,
    operation_lock: Mutex<()>,
}

impl<R: Runtime, B: ImageBuilder> DeploymentService<R, B> {
    pub fn new(store: StateStore, runtime: R, builder: B) -> Self {
        Self {
            store,
            runtime: Mutex::new(runtime),
            builder,
            operation_lock: Mutex::new(()),
        }
    }
}

impl<R: Runtime + Send, B: ImageBuilder + Send + Sync> PlatformService for DeploymentService<R, B> {
    fn create_project(&self, input: CreateProjectInput) -> Result<Project> {
        let name = input.name.trim();
        if name.is_empty() {
            anyhow::bail!("project name cannot be empty");
        }
        if name.len() > 63 {
            anyhow::bail!("project name cannot exceed 63 bytes");
        }

        let source_dir = input.source_dir.canonicalize().with_context(|| {
            format!(
                "source directory does not exist: {}",
                input.source_dir.display()
            )
        })?;
        if !source_dir.is_dir() {
            anyhow::bail!("source path is not a directory: {}", source_dir.display());
        }

        let base_image = input.base_image.canonicalize().with_context(|| {
            format!("base image does not exist: {}", input.base_image.display())
        })?;
        if !base_image.is_file() {
            anyhow::bail!("base image is not a file: {}", base_image.display());
        }

        let port_exact = input
            .server_command
            .iter()
            .filter(|t| *t == "{PORT}")
            .count();
        let port_contains = input
            .server_command
            .iter()
            .filter(|t| t.contains("{PORT}"))
            .count();
        if port_contains != 1 || port_exact != 1 {
            anyhow::bail!("server command must contain exactly one {{PORT}} token");
        }

        let state = self.store.load()?;
        if state.projects.iter().any(|p| p.name == name) {
            anyhow::bail!("project name already exists: {name}");
        }

        let project = Project {
            id: Uuid::new_v4(),
            name: name.to_string(),
            user_id: input.user_id,
            source_dir,
            base_image,
            server_command: input.server_command,
            created_at: OffsetDateTime::now_utc(),
            active_deployment: None,
        };

        self.store.insert_project(project.clone())?;
        Ok(project)
    }

    fn project(&self, id: Uuid) -> Result<Project> {
        self.store.project(id)
    }

    fn projects(&self) -> Result<Vec<Project>> {
        self.store.load().map(|s| s.projects)
    }

    fn deploy(&self, project_id: Uuid) -> Result<Deployment> {
        let _guard = self.operation_lock.lock().unwrap();

        let mut project = self.store.project(project_id)?;
        let deployment_id = Uuid::new_v4();
        let now = OffsetDateTime::now_utc();

        let mut deployment = Deployment {
            id: deployment_id,
            project_id,
            framework: Framework::Static,
            status: DeploymentStatus::Pending,
            image_path: None,
            container_id: None,
            port: None,
            url: None,
            created_at: now,
            finished_at: None,
            error: None,
        };
        self.store.insert_deployment(deployment.clone())?;

        deployment.status = DeploymentStatus::Building;
        self.store.update_deployment(deployment.clone())?;

        let static_source = match detect_static_source(&project.source_dir) {
            Ok(s) => s,
            Err(e) => {
                let msg = format!("detection failed: {e:#}");
                deployment.status = DeploymentStatus::Failed;
                deployment.finished_at = Some(OffsetDateTime::now_utc());
                deployment.error = Some(msg.clone());
                let _ = self.store.update_deployment(deployment);
                return Err(anyhow::anyhow!(msg));
            }
        };

        let build_output = match self.builder.build(
            self.store.root(),
            deployment_id,
            &static_source,
            &project.base_image,
        ) {
            Ok(o) => o,
            Err(e) => {
                let msg = format!("build failed: {e:#}");
                deployment.status = DeploymentStatus::Failed;
                deployment.finished_at = Some(OffsetDateTime::now_utc());
                deployment.error = Some(msg.clone());
                let _ = self.store.update_deployment(deployment);
                return Err(anyhow::anyhow!(msg));
            }
        };

        deployment.image_path = Some(build_output.image_path.clone());
        self.store.update_deployment(deployment.clone())?;

        let hostname = format!("platform-{}", deployment_id.simple());
        let running_res = {
            let mut rt = self.runtime.lock().unwrap();
            rt.start_static(&build_output.image_path, &hostname, &project.server_command)
        };

        let running_container = match running_res {
            Ok(c) => c,
            Err(e) => {
                let msg = format!("runtime start failed for {hostname}: {e:#}");
                deployment.status = DeploymentStatus::Failed;
                deployment.finished_at = Some(OffsetDateTime::now_utc());
                deployment.error = Some(msg.clone());
                let _ = self.store.update_deployment(deployment);
                return Err(anyhow::anyhow!(msg));
            }
        };

        deployment.container_id = Some(running_container.id);
        deployment.port = Some(running_container.port);
        deployment.url = Some(running_container.url);
        deployment.status = DeploymentStatus::Running;
        deployment.finished_at = Some(OffsetDateTime::now_utc());

        if let Err(save_err) = self.store.update_deployment(deployment.clone()) {
            let mut rt = self.runtime.lock().unwrap();
            let _ = rt.stop(running_container.id);
            return Err(save_err.context("failed to persist running deployment state"));
        }

        if let Some(old_id) = project.active_deployment {
            if let Ok(mut old_deployment) = self.store.deployment(old_id) {
                if old_deployment.status == DeploymentStatus::Running {
                    let mut stop_cleanup_err = None;
                    if let Some(cid) = old_deployment.container_id {
                        let mut rt = self.runtime.lock().unwrap();
                        if let Err(e) = rt.stop(cid) {
                            stop_cleanup_err = Some(e);
                        }
                    }
                    old_deployment.status = DeploymentStatus::Stopped;
                    if let Some(cleanup_err) = stop_cleanup_err {
                        let prior_err = old_deployment.error.unwrap_or_default();
                        old_deployment.error =
                            Some(format!("{prior_err}; cleanup failed: {cleanup_err}"));
                    }
                    let _ = self.store.update_deployment(old_deployment);
                }
            }
        }

        project.active_deployment = Some(deployment_id);
        self.store.update_project(project)?;

        Ok(deployment)
    }

    fn deployments(&self, project_id: Uuid) -> Result<Vec<Deployment>> {
        self.store.project(project_id)?;
        let state = self.store.load()?;
        let mut list: Vec<Deployment> = state
            .deployments
            .into_iter()
            .filter(|d| d.project_id == project_id)
            .collect();
        list.sort_by_key(|d| d.created_at);
        Ok(list)
    }

    fn deployment(&self, id: Uuid) -> Result<Deployment> {
        self.store.deployment(id)
    }

    fn stop(&self, id: Uuid) -> Result<Deployment> {
        let _guard = self.operation_lock.lock().unwrap();

        let mut deployment = self.store.deployment(id)?;
        if deployment.status == DeploymentStatus::Stopped
            || deployment.status == DeploymentStatus::Failed
        {
            return Ok(deployment);
        }

        let container_id = deployment
            .container_id
            .ok_or_else(|| anyhow::anyhow!("deployment {id} has no container ID"))?;

        {
            let mut rt = self.runtime.lock().unwrap();
            rt.stop(container_id)?;
        }

        deployment.status = DeploymentStatus::Stopped;
        self.store.update_deployment(deployment.clone())?;

        if let Ok(mut project) = self.store.project(deployment.project_id) {
            if project.active_deployment == Some(id) {
                project.active_deployment = None;
                self.store.update_project(project)?;
            }
        }

        Ok(deployment)
    }

    fn logs(&self, id: Uuid) -> Result<String> {
        let deployment = self.store.deployment(id)?;
        let container_id = deployment
            .container_id
            .ok_or_else(|| anyhow::anyhow!("deployment {id} has no container ID"))?;
        let rt = self.runtime.lock().unwrap();
        rt.logs(container_id)
    }
}
