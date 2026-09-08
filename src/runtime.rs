use crate::model::RunningContainer;
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::time::Duration;
use uuid::Uuid;

pub trait Runtime: Send {
    fn start_static(
        &mut self,
        image: &Path,
        hostname: &str,
        command: &[String],
    ) -> Result<RunningContainer>;
    fn stop(&mut self, container_id: Uuid) -> Result<()>;
    fn logs(&self, container_id: Uuid) -> Result<String>;
}

pub struct MinidockRuntime {
    pub minidock_store: minidock::StateStore,
}

impl Runtime for MinidockRuntime {
    fn start_static(
        &mut self,
        image: &Path,
        hostname: &str,
        command: &[String],
    ) -> Result<RunningContainer> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .with_context(|| format!("failed to allocate local port for {hostname}"))?;
        let port = listener
            .local_addr()
            .with_context(|| format!("failed to get local addr for {hostname}"))?
            .port();
        drop(listener);

        let rendered = render_server_command(command, port)
            .with_context(|| format!("failed to render server command for {hostname}"))?;

        let before = self.minidock_store.list().with_context(|| {
            format!("failed to list minidock containers before run for {hostname}")
        })?;

        let req = minidock::RunRequest {
            image: image.to_path_buf(),
            memory_bytes: None,
            cpu_percent: None,
            hostname: hostname.to_string(),
            detached: true,
            command: rendered.clone(),
        };

        minidock::run(req, self.minidock_store.clone())
            .with_context(|| format!("failed to run minidock container for {hostname}"))?;

        let after = self.minidock_store.list().with_context(|| {
            format!("failed to list minidock containers after run for {hostname}")
        })?;

        let container = discover_new_container(&before, &after, hostname, &rendered)
            .with_context(|| format!("failed to discover new minidock container for {hostname}"))?;

        if let Err(health_err) = wait_for_port(port, Duration::from_secs(5)) {
            let _ = minidock::stop(container.id, &self.minidock_store);
            return Err(health_err.context(format!("container health check failed for {hostname}")));
        }

        Ok(RunningContainer {
            id: container.id,
            port,
            url: format!("http://127.0.0.1:{port}"),
        })
    }

    fn stop(&mut self, container_id: Uuid) -> Result<()> {
        minidock::stop(container_id, &self.minidock_store)
            .with_context(|| format!("failed to stop minidock container {container_id}"))
    }

    fn logs(&self, container_id: Uuid) -> Result<String> {
        let log_path = self.minidock_store.log_path(container_id);
        if !log_path.exists() {
            return Ok(String::new());
        }
        std::fs::read_to_string(&log_path)
            .with_context(|| format!("failed to read container log at {}", log_path.display()))
    }
}

pub fn render_server_command(template: &[String], port: u16) -> Result<Vec<String>> {
    let exact_matches = template.iter().filter(|t| *t == "{PORT}").count();
    let embedded_matches = template
        .iter()
        .filter(|t| t.contains("{PORT}") && *t != "{PORT}")
        .count();

    if embedded_matches > 0 {
        anyhow::bail!(
            "{{PORT}} placeholder must be a complete token, not embedded within an argument"
        );
    }
    if exact_matches == 0 {
        anyhow::bail!("server command template must contain {{PORT}} placeholder");
    }
    if exact_matches > 1 {
        anyhow::bail!("server command template must contain exactly one {{PORT}} token");
    }

    let rendered = template
        .iter()
        .map(|t| {
            if t == "{PORT}" {
                port.to_string()
            } else {
                t.clone()
            }
        })
        .collect();

    Ok(rendered)
}

pub fn discover_new_container(
    before: &[minidock::ContainerState],
    after: &[minidock::ContainerState],
    hostname: &str,
    command: &[String],
) -> Result<minidock::ContainerState> {
    let before_ids: HashSet<Uuid> = before.iter().map(|c| c.id).collect();

    let mut matches: Vec<minidock::ContainerState> = after
        .iter()
        .filter(|c| {
            !before_ids.contains(&c.id)
                && c.hostname == hostname
                && c.command == command
                && c.detached
                && (c.status == minidock::ContainerStatus::Running
                    || c.status == minidock::ContainerStatus::Exited)
        })
        .cloned()
        .collect();

    if matches.is_empty() {
        anyhow::bail!("no new container discovered for hostname {hostname}");
    }
    if matches.len() > 1 {
        anyhow::bail!(
            "ambiguous container discovery for hostname {hostname}: matched {} containers",
            matches.len()
        );
    }

    Ok(matches.remove(0))
}

pub fn wait_for_port(port: u16, timeout: Duration) -> Result<()> {
    let start = std::time::Instant::now();
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    loop {
        if TcpStream::connect_timeout(&addr, Duration::from_millis(100)).is_ok() {
            return Ok(());
        }
        if start.elapsed() >= timeout {
            anyhow::bail!(
                "health check timed out waiting for port {port} on 127.0.0.1 after {timeout:?}"
            );
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
