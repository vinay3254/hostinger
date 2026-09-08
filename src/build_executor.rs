use crate::framework::BuildPlan;
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct BuildLogLine {
    pub sequence: u64,
    pub timestamp: OffsetDateTime,
    pub stream: LogStream,
    pub message: String,
}

pub trait LogSink: Send {
    fn write_line(&mut self, stream: LogStream, message: &str) -> Result<()>;
}

#[derive(Debug, Default, Clone)]
pub struct MemoryLogSink {
    pub lines: Vec<BuildLogLine>,
    pub sequence: u64,
}

impl MemoryLogSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn lines(&self) -> &[BuildLogLine] {
        &self.lines
    }
}

impl LogSink for MemoryLogSink {
    fn write_line(&mut self, stream: LogStream, message: &str) -> Result<()> {
        self.sequence += 1;
        self.lines.push(BuildLogLine {
            sequence: self.sequence,
            timestamp: OffsetDateTime::now_utc(),
            stream,
            message: message.to_string(),
        });
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildResult {
    pub artifact_path: PathBuf,
    pub cache_key: String,
    pub duration: Duration,
}

pub trait BuildExecutor: Send + Sync {
    fn execute(
        &self,
        plan: &BuildPlan,
        workspace: &Path,
        log: &mut dyn LogSink,
    ) -> Result<BuildResult>;
}

pub struct MinidockBuildExecutor {
    pub artifacts_dir: PathBuf,
    pub timeout: Duration,
    pub secrets: Vec<String>,
    pub memory_bytes: Option<usize>,
    pub cpu_percent: Option<u32>,
    pub base_rootfs: Option<PathBuf>,
}

impl MinidockBuildExecutor {
    pub fn new(artifacts_dir: impl Into<PathBuf>) -> Self {
        Self {
            artifacts_dir: artifacts_dir.into(),
            timeout: Duration::from_secs(900),
            secrets: Vec::new(),
            memory_bytes: None,
            cpu_percent: None,
            base_rootfs: None,
        }
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn with_secret(mut self, secret: impl Into<String>) -> Self {
        self.secrets.push(secret.into());
        self
    }

    pub fn with_base_rootfs(mut self, base_rootfs: PathBuf) -> Self {
        self.base_rootfs = Some(base_rootfs);
        self
    }

    pub fn with_resource_limits(
        mut self,
        memory_bytes: Option<usize>,
        cpu_percent: Option<u32>,
    ) -> Self {
        self.memory_bytes = memory_bytes;
        self.cpu_percent = cpu_percent;
        self
    }

    fn run_command_streamed(
        &self,
        cmd: &[String],
        cwd: &Path,
        log: &mut dyn LogSink,
    ) -> Result<()> {
        if cmd.is_empty() {
            return Ok(());
        }

        let mut child = Command::new(&cmd[0])
            .args(&cmd[1..])
            .current_dir(cwd)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to spawn command: {:?}", cmd))?;

        let stdout = child.stdout.take().context("missing child stdout")?;
        let stderr = child.stderr.take().context("missing child stderr")?;

        enum LogItem {
            Line(LogStream, String),
            Finished,
        }

        let (tx, rx) = mpsc::channel();
        let tx_err = tx.clone();

        thread::spawn(move || {
            let reader = BufReader::new(stdout);
            for line in reader.lines().map_while(Result::ok) {
                let _ = tx.send(LogItem::Line(LogStream::Stdout, line));
            }
            let _ = tx.send(LogItem::Finished);
        });

        thread::spawn(move || {
            let reader = BufReader::new(stderr);
            for line in reader.lines().map_while(Result::ok) {
                let _ = tx_err.send(LogItem::Line(LogStream::Stderr, line));
            }
            let _ = tx_err.send(LogItem::Finished);
        });

        let deadline = Instant::now() + self.timeout;
        let mut finished_count = 0;

        while finished_count < 2 {
            let now = Instant::now();
            if now >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                bail!("command {:?} timed out after {:?}", cmd, self.timeout);
            }
            let timeout_remaining = deadline - now;
            match rx.recv_timeout(timeout_remaining) {
                Ok(LogItem::Line(stream, line)) => {
                    let redacted = redact_text(&line, &self.secrets);
                    log.write_line(stream, &redacted)?;
                }
                Ok(LogItem::Finished) => {
                    finished_count += 1;
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    bail!("command {:?} timed out after {:?}", cmd, self.timeout);
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    break;
                }
            }
        }

        let status = child
            .wait()
            .with_context(|| format!("failed waiting for command {:?}", cmd))?;

        if !status.success() {
            bail!("command {:?} failed with status: {:?}", cmd, status.code());
        }

        Ok(())
    }
}

fn redact_text(line: &str, secrets: &[String]) -> String {
    let mut result = line.to_string();
    for secret in secrets {
        if !secret.is_empty() {
            result = result.replace(secret, "[REDACTED]");
        }
    }
    result
}

impl BuildExecutor for MinidockBuildExecutor {
    fn execute(
        &self,
        plan: &BuildPlan,
        workspace: &Path,
        log: &mut dyn LogSink,
    ) -> Result<BuildResult> {
        for comp in plan.output.components() {
            match comp {
                Component::ParentDir => {
                    bail!(
                        "path traversal detected in output path: {}",
                        plan.output.display()
                    );
                }
                Component::RootDir | Component::Prefix(_) => {
                    bail!(
                        "absolute output path escapes workspace: {}",
                        plan.output.display()
                    );
                }
                _ => {}
            }
        }

        let start = Instant::now();

        if !plan.install.is_empty() {
            self.run_command_streamed(&plan.install, workspace, log)?;
        }

        if !plan.build.is_empty() {
            self.run_command_streamed(&plan.build, workspace, log)?;
        }

        let output_path = workspace.join(&plan.output);
        if !output_path.exists() {
            bail!(
                "build output path does not exist: {}",
                output_path.display()
            );
        }

        fs::create_dir_all(&self.artifacts_dir).with_context(|| {
            format!(
                "failed to create artifacts directory: {}",
                self.artifacts_dir.display()
            )
        })?;

        let artifact_filename = format!("{}.tar.gz", Uuid::new_v4());
        let artifact_path = self.artifacts_dir.join(artifact_filename);

        if output_path.is_dir() {
            minidock::build_image(&output_path, &artifact_path).with_context(|| {
                format!(
                    "failed to build artifact image from {}",
                    output_path.display()
                )
            })?;
        } else {
            let parent = output_path.parent().unwrap_or(workspace);
            minidock::build_image(parent, &artifact_path).with_context(|| {
                format!("failed to build artifact image from {}", parent.display())
            })?;
        }

        let mut hasher = Sha256::new();
        hasher.update(format!("{:?}", plan.framework).as_bytes());
        for cmd in &plan.install {
            hasher.update(cmd.as_bytes());
        }
        for cmd in &plan.build {
            hasher.update(cmd.as_bytes());
        }
        if let Ok(entries) = fs::read_dir(workspace) {
            for entry in entries.flatten() {
                if let Ok(name) = entry.file_name().into_string() {
                    hasher.update(name.as_bytes());
                }
            }
        }
        let cache_key = hex::encode(hasher.finalize());

        let duration = start.elapsed();

        Ok(BuildResult {
            artifact_path,
            cache_key,
            duration,
        })
    }
}
