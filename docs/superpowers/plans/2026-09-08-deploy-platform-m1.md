# Deploy Platform M1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a separate Rust `deploy-platform` repository that validates a local static site, packages it with a minidock base rootfs, runs it as a single-node HTTP deployment, and exposes the lifecycle through a service layer, CLI, and Axum API.

**Architecture:** Keep `model`, `store`, `detector`, `builder`, `runtime`, `service`, `api`, and `main` as focused modules. The service depends on `ImageBuilder` and `Runtime` traits, so unprivileged tests use fakes while production uses minidock through a single adapter. M1 uses synchronous deployment operations guarded by one mutex and persists all platform state atomically as JSON.

**Tech Stack:** Rust 1.70+, Cargo, `anyhow`, `axum` 0.7, `tokio`, `clap`, `serde`, `serde_json`, `time` 0.3.47, `uuid`, `tempfile`, `assert_cmd`, `predicates`, and the local `minidock` Cargo path dependency.

**Spec:** `docs/superpowers/specs/2026-09-08-deploy-platform-m1-design.md`

## Global Constraints

- Keep `/home/vinay/deploy-platform` independent from `/home/vinay/minidock`; use `minidock = { path = "../minidock" }` only at the runtime/image boundary.
- Support Rust 1.70 or newer and Linux for the real runtime path.
- Accept only local static sources with a regular root `index.html`; reject unsupported frameworks explicitly.
- Do not run user input through a shell; commands are argv vectors and `{PORT}` is replaced only when it is a complete token.
- Treat base images as trusted local gzip-compressed minidock rootfs archives.
- Reject source symlinks, special files, absolute paths, and traversal outside the source or staging root.
- Persist state atomically under the configured state root and parse UUIDs before using them in paths.
- Keep the default test suite unprivileged; real minidock integration is ignored unless explicitly enabled.
- Do not add authentication, webhooks, queues, TLS, custom domains, dashboard code, previews, SSR, or multi-node scheduling.

## Planned File Structure

| Path | Responsibility |
| --- | --- |
| `Cargo.toml` | Package metadata and runtime/test dependencies. |
| `.gitignore` | Ignore Cargo output, local state, and deployment build artifacts. |
| `src/lib.rs` | Public module declarations and shared exports. |
| `src/model.rs` | Serializable platform records, statuses, and request inputs. |
| `src/config.rs` | Environment/default configuration and argv template parsing. |
| `src/store.rs` | Atomic JSON state and per-deployment build paths. |
| `src/detector.rs` | Safe static-source traversal and framework detection. |
| `src/builder.rs` | Base-rootfs extraction, static-file copy, and image packaging. |
| `src/runtime.rs` | Runtime trait, minidock adapter, port/command helpers, health check. |
| `src/service.rs` | Project/deployment orchestration and lifecycle transitions. |
| `src/api.rs` | Axum routes, DTO conversion, and HTTP error mapping. |
| `src/main.rs` | Clap CLI, local service construction, and API server startup. |
| `tests/model_test.rs` | Domain serialization and validation. |
| `tests/store_test.rs` | Atomic store round trips and ID/status behavior. |
| `tests/detector_test.rs` | Static detection and filesystem safety. |
| `tests/builder_test.rs` | Image assembly and `/srv/app` contents. |
| `tests/runtime_test.rs` | Command substitution, port allocation, and state discovery. |
| `tests/service_test.rs` | Deployment success/failure/stop behavior using fakes. |
| `tests/api_test.rs` | In-process Axum endpoint behavior. |
| `tests/cli_test.rs` | CLI parsing/help/error behavior. |
| `tests/privileged_runtime_test.rs` | Opt-in real minidock smoke test. |
| `README.md` | Prerequisites, local workflow, API examples, and verification commands. |

### Task 1: Bootstrap the separate crate and domain/config types

**Files:**

- Create: `Cargo.toml`
- Create: `.gitignore`
- Create: `src/lib.rs`
- Create: `src/model.rs`
- Create: `src/config.rs`
- Create: `tests/model_test.rs`

**Interfaces:**

- Produces `Framework`, `DeploymentStatus`, `PlatformState`, `Project`, `Deployment`, `CreateProjectInput`, `RunningContainer`, and `PlatformConfig` types used by all later tasks.
- Produces `parse_server_command(&str) -> anyhow::Result<Vec<String>>`.
- Consumes no project code; depends on the existing minidock manifest only through `Cargo.toml`.

- [x] **Step 1: Write the failing domain/config tests**

```rust
// tests/model_test.rs
use deploy_platform::{
    config::parse_server_command,
    model::{DeploymentStatus, Framework, PlatformState},
};

#[test]
fn empty_platform_state_serializes_with_version_one() {
    let json = serde_json::to_string(&PlatformState::empty()).unwrap();
    assert!(json.contains("\"version\":1"));
    assert!(json.contains("\"projects\":[]"));
    assert!(json.contains("\"deployments\":[]"));
}

#[test]
fn deployment_status_uses_stable_lowercase_json_names() {
    let json = serde_json::to_string(&DeploymentStatus::Running).unwrap();
    assert_eq!(json, "\"running\"");
}

#[test]
fn server_command_splits_ascii_whitespace_without_shell_expansion() {
    let command = parse_server_command("/bin/busybox httpd -p {PORT} -h /srv/app").unwrap();
    assert_eq!(command, ["/bin/busybox", "httpd", "-p", "{PORT}", "-h", "/srv/app"]);
}

#[test]
fn empty_server_command_is_rejected() {
    let error = parse_server_command("   ").unwrap_err();
    assert!(error.to_string().contains("server command cannot be empty"));
}

#[test]
fn framework_has_only_the_m1_static_variant() {
    assert_eq!(serde_json::to_string(&Framework::Static).unwrap(), "\"static\"");
}
```

- [x] **Step 2: Run the focused tests and verify the expected missing-crate failure**

Run: `cargo test --test model_test`

Expected: FAIL because the manifest and library modules do not exist yet.

- [x] **Step 3: Add the manifest, ignore rules, and public module root**

```toml
# Cargo.toml
[package]
name = "deploy-platform"
version = "0.1.0"
edition = "2021"
rust-version = "1.70"

[dependencies]
anyhow = "1.0"
axum = "0.7"
clap = { version = "4.5", features = ["derive"] }
minidock = { path = "../minidock" }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
time = { version = "=0.3.47", features = ["formatting", "macros", "parsing", "serde"] }
tokio = { version = "1.0", features = ["macros", "net", "rt-multi-thread"] }
uuid = { version = "1.8", features = ["v4", "serde"] }

[dev-dependencies]
assert_cmd = "2.0"
http-body-util = "0.1"
predicates = "3.1"
tempfile = "3.10"
tower = "0.5"
```

```gitignore
/target/
/.deploy-platform/
/builds/
```

```rust
// src/lib.rs
pub mod config;
pub mod model;

pub type Result<T> = anyhow::Result<T>;
```

- [x] **Step 4: Implement the model and config contracts**

```rust
// src/model.rs
use std::path::PathBuf;
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Framework { Static }

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DeploymentStatus { Pending, Building, Running, Failed, Stopped }

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PlatformState {
    pub version: u8,
    pub projects: Vec<Project>,
    pub deployments: Vec<Deployment>,
}

impl PlatformState {
    pub fn empty() -> Self {
        Self { version: 1, projects: Vec::new(), deployments: Vec::new() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub source_dir: PathBuf,
    pub base_image: PathBuf,
    pub server_command: Vec<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    pub active_deployment: Option<Uuid>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Deployment {
    pub id: Uuid,
    pub project_id: Uuid,
    pub framework: Framework,
    pub status: DeploymentStatus,
    pub image_path: Option<PathBuf>,
    pub container_id: Option<Uuid>,
    pub port: Option<u16>,
    pub url: Option<String>,
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,
    #[serde(with = "option_time_rfc3339")]
    pub finished_at: Option<OffsetDateTime>,
    pub error: Option<String>,
}

mod option_time_rfc3339 {
    pub use time::serde::rfc3339::option::{deserialize, serialize};
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateProjectInput {
    pub name: String,
    pub source_dir: PathBuf,
    pub base_image: PathBuf,
    pub server_command: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningContainer {
    pub id: Uuid,
    pub port: u16,
    pub url: String,
}
```

Implement `config::PlatformConfig` with `state_dir`, `listen_addr`, and
`server_command`. `from_env()` must use `$HOME/.deploy-platform`,
`127.0.0.1:8787`, and `/bin/busybox httpd -f -p {PORT} -h /srv/app` as defaults;
`PLATFORM_STATE_DIR` and `PLATFORM_LISTEN_ADDR` override those values. Parse
`PLATFORM_SERVER_COMMAND` using `split_ascii_whitespace`, reject an empty result,
and require exactly one token equal to `{PORT}`. Do not interpret quotes or shell
operators.

- [x] **Step 5: Run formatting and the focused tests**

Run: `cargo fmt --check && cargo test --test model_test`

Expected: PASS with all model/config tests green.

- [x] **Step 6: Commit the bootstrap**

```bash
git add Cargo.toml .gitignore src tests/model_test.rs
git commit -m "feat: bootstrap deploy platform domain types"
```

### Task 2: Implement atomic platform state storage

**Files:**

- Create: `src/store.rs`
- Create: `tests/store_test.rs`
- Modify: `src/lib.rs`

**Interfaces:**

- Consumes `PlatformState`, `Project`, and `Deployment` from Task 1.
- Produces `StateStore::at`, `load`, `save`, `build_dir`, `project`, `deployment`, `insert_project`, `insert_deployment`, `update_project`, and `update_deployment`.

- [x] **Step 1: Write failing store tests**

```rust
// tests/store_test.rs
use deploy_platform::{
    model::{Deployment, DeploymentStatus, Framework, PlatformState, Project},
    store::StateStore,
};
use std::path::PathBuf;
use tempfile::tempdir;
use time::OffsetDateTime;
use uuid::Uuid;

fn project() -> Project {
    Project {
        id: Uuid::new_v4(),
        name: "site".into(),
        source_dir: PathBuf::from("/tmp/site"),
        base_image: PathBuf::from("/tmp/base.tar.gz"),
        server_command: vec!["/bin/busybox".into(), "httpd".into(), "-p".into(), "{PORT}".into()],
        created_at: OffsetDateTime::now_utc(),
        active_deployment: None,
    }
}

#[test]
fn save_and_load_round_trip_atomically() {
    let dir = tempdir().unwrap();
    let store = StateStore::at(dir.path().to_path_buf());
    let mut state = PlatformState::empty();
    state.projects.push(project());
    store.save(&state).unwrap();
    assert_eq!(store.load().unwrap(), state);
    assert!(!store.state_path().with_extension("json.tmp").exists());
}

#[test]
fn lookup_missing_uuid_returns_a_clear_error() {
    let store = StateStore::at(tempdir().unwrap().path().to_path_buf());
    let error = store.project(Uuid::new_v4()).unwrap_err();
    assert!(error.to_string().contains("project not found"));
}

#[test]
fn build_directory_is_scoped_to_uuid() {
    let root = tempdir().unwrap();
    let store = StateStore::at(root.path().to_path_buf());
    let id = Uuid::new_v4();
    assert_eq!(store.build_dir(id), root.path().join("builds").join(id.to_string()));
}

#[test]
fn updating_deployment_persists_status() {
    let root = tempdir().unwrap();
    let store = StateStore::at(root.path().to_path_buf());
    let project = project();
    let deployment = Deployment {
        id: Uuid::new_v4(), project_id: project.id, framework: Framework::Static,
        status: DeploymentStatus::Pending, image_path: None, container_id: None,
        port: None, url: None, created_at: OffsetDateTime::now_utc(),
        finished_at: None, error: None,
    };
    store.insert_project(project.clone()).unwrap();
    store.insert_deployment(deployment.clone()).unwrap();
    let mut changed = deployment;
    let changed_id = changed.id;
    changed.status = DeploymentStatus::Failed;
    changed.error = Some("build failed".into());
    store.update_deployment(changed).unwrap();
    assert_eq!(store.deployment(changed_id).unwrap().status, DeploymentStatus::Failed);
}
```

- [x] **Step 2: Run the tests and verify they fail for the missing store module**

Run: `cargo test --test store_test`

Expected: FAIL because `store.rs` and `StateStore` do not exist.

- [x] **Step 3: Implement the store with UUID-safe paths and atomic replacement**

```rust
// src/store.rs, public shape
#[derive(Debug, Clone)]
pub struct StateStore { root: PathBuf }

impl StateStore {
    pub fn at(root: PathBuf) -> Self;
    pub fn root(&self) -> &Path;
    pub fn state_path(&self) -> PathBuf;
    pub fn build_dir(&self, deployment_id: Uuid) -> PathBuf;
    pub fn load(&self) -> Result<PlatformState>;
    pub fn save(&self, state: &PlatformState) -> Result<()>;
    pub fn project(&self, id: Uuid) -> Result<Project>;
    pub fn deployment(&self, id: Uuid) -> Result<Deployment>;
    pub fn insert_project(&self, project: Project) -> Result<()>;
    pub fn insert_deployment(&self, deployment: Deployment) -> Result<()>;
    pub fn update_project(&self, project: Project) -> Result<()>;
    pub fn update_deployment(&self, deployment: Deployment) -> Result<()>;
}
```

`load()` returns `PlatformState::empty()` when the state file does not exist.
`save()` creates the root, writes pretty JSON to `state.json.tmp`, calls
`sync_all`, closes the file, and renames it to `state.json`; remove the temporary
file on any failed write. Each mutation loads the full state, rejects duplicate
IDs, replaces the matching record, and calls `save`. Never accept a caller-
provided path component in place of a parsed `Uuid`.

- [x] **Step 4: Run formatting and store tests**

Run: `cargo fmt --check && cargo test --test store_test`

Expected: PASS with four store tests green.

- [x] **Step 5: Commit the storage layer**

```bash
git add src/lib.rs src/store.rs tests/store_test.rs
git commit -m "feat: add atomic platform state store"
```

### Task 3: Add safe static-source detection

**Files:**

- Create: `src/detector.rs`
- Create: `tests/detector_test.rs`
- Modify: `src/lib.rs`

**Interfaces:**

- Consumes filesystem paths from project records.
- Produces `StaticSource`, `SourceFile`, and `detect_static_source(&Path) -> Result<StaticSource>`.

- [x] **Step 1: Write failing detector tests**

```rust
// tests/detector_test.rs
use deploy_platform::detector::detect_static_source;
use std::fs;
use tempfile::tempdir;

#[test]
fn detects_root_index_and_lists_regular_files() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("index.html"), "hello").unwrap();
    fs::create_dir(dir.path().join("assets")).unwrap();
    fs::write(dir.path().join("assets/app.css"), "body{}").unwrap();
    let source = detect_static_source(dir.path()).unwrap();
    assert_eq!(source.files.iter().map(|file| file.relative.clone()).collect::<Vec<_>>(), vec![
        std::path::PathBuf::from("assets/app.css"),
        std::path::PathBuf::from("index.html"),
    ]);
}

#[test]
fn rejects_a_source_without_root_index_html() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("README.md"), "not a site").unwrap();
    let error = detect_static_source(dir.path()).unwrap_err();
    assert!(error.to_string().contains("root index.html"));
}

#[cfg(unix)]
#[test]
fn rejects_source_symlinks_instead_of_following_them() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("index.html"), "hello").unwrap();
    std::os::unix::fs::symlink("/etc/passwd", dir.path().join("leak")).unwrap();
    let error = detect_static_source(dir.path()).unwrap_err();
    assert!(error.to_string().contains("symlink"));
}

#[test]
fn skips_the_source_git_directory() {
    let dir = tempdir().unwrap();
    fs::write(dir.path().join("index.html"), "hello").unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    fs::write(dir.path().join(".git/config"), "secret metadata").unwrap();
    let source = detect_static_source(dir.path()).unwrap();
    assert!(source.files.iter().all(|file| !file.relative.starts_with(".git")));
}
```

- [x] **Step 2: Run detector tests and verify the missing-module failure**

Run: `cargo test --test detector_test`

Expected: FAIL because `detector.rs` is not implemented.

- [x] **Step 3: Implement canonical traversal and regular-file validation**

```rust
// src/detector.rs
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceFile { pub relative: PathBuf }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticSource { pub root: PathBuf, pub files: Vec<SourceFile> }

pub fn detect_static_source(source_dir: &Path) -> Result<StaticSource>;
```

Canonicalize the input and require a directory. Check `root/index.html` with
`symlink_metadata` and require a regular file. Recursively enumerate directories
with `read_dir`, sort entries by relative path for deterministic builds, skip only
the root `.git` directory and its descendants, reject symlink entries and any
non-regular/non-directory entry, and return relative paths. Do not follow a
symlink while deciding whether an entry is safe.

- [x] **Step 4: Run formatting and detector tests**

Run: `cargo fmt --check && cargo test --test detector_test`

Expected: PASS with valid, missing-index, symlink, and `.git` cases green.

- [x] **Step 5: Commit source detection**

```bash
git add src/lib.rs src/detector.rs tests/detector_test.rs
git commit -m "feat: detect safe static deployment sources"
```

### Task 4: Build deployable minidock images

**Files:**

- Create: `src/builder.rs`
- Create: `tests/builder_test.rs`
- Modify: `src/lib.rs`

**Interfaces:**

- Consumes `StaticSource` and minidock image helpers.
- Produces `BuildOutput`, `ImageBuilder`, `MinidockImageBuilder`, and
  `build_deployment_image(&Path, Uuid, &StaticSource, &Path) -> Result<BuildOutput>`.

- [x] **Step 1: Write a failing end-to-end image assembly test**

```rust
// tests/builder_test.rs
use deploy_platform::{builder::build_deployment_image, detector::detect_static_source};
use minidock::{build_image, extract_rootfs};
use std::fs;
use tempfile::tempdir;
use uuid::Uuid;

#[test]
fn assembled_image_contains_base_files_and_static_files_under_srv_app() {
    let root = tempdir().unwrap();
    let base_context = root.path().join("base-context");
    fs::create_dir_all(base_context.join("bin")).unwrap();
    fs::write(base_context.join("bin/busybox"), "placeholder").unwrap();
    let base_image = root.path().join("base.tar.gz");
    build_image(&base_context, &base_image).unwrap();

    let source_dir = root.path().join("site");
    fs::create_dir_all(source_dir.join("assets")).unwrap();
    fs::write(source_dir.join("index.html"), "<h1>hello</h1>").unwrap();
    fs::write(source_dir.join("assets/app.css"), "body{}").unwrap();
    let source = detect_static_source(&source_dir).unwrap();
    let output = build_deployment_image(root.path(), Uuid::new_v4(), &source, &base_image).unwrap();

    let extracted = root.path().join("verify");
    extract_rootfs(&output.image_path, &extracted).unwrap();
    assert_eq!(fs::read_to_string(extracted.join("bin/busybox")).unwrap(), "placeholder");
    assert_eq!(fs::read_to_string(extracted.join("srv/app/index.html")).unwrap(), "<h1>hello</h1>");
    assert_eq!(fs::read_to_string(extracted.join("srv/app/assets/app.css")).unwrap(), "body{}");
}
```

- [x] **Step 2: Run the builder test and verify the missing-module failure**

Run: `cargo test --test builder_test`

Expected: FAIL because `builder.rs` and `build_deployment_image` do not exist.

- [x] **Step 3: Implement `MinidockImageBuilder` and safe file copying**

```rust
// src/builder.rs
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildOutput {
    pub rootfs_path: PathBuf,
    pub image_path: PathBuf,
}

pub trait ImageBuilder {
    fn build(&self, root: &Path, deployment_id: Uuid, source: &StaticSource, base_image: &Path)
        -> Result<BuildOutput>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct MinidockImageBuilder;

impl ImageBuilder for MinidockImageBuilder { /* delegates to build_deployment_image */ }

pub fn build_deployment_image(
    state_root: &Path,
    deployment_id: Uuid,
    source: &StaticSource,
    base_image: &Path,
) -> Result<BuildOutput>;
```

Create `state_root/builds/<deployment_id>/rootfs`, extract `base_image` into it,
create `/srv/app`, and copy exactly the `StaticSource.files` entries from the
canonical source root. For each relative path, assert it is relative and that
`destination.join(relative)` remains under `/srv/app`; create parent directories,
copy file bytes, and preserve only the regular-file contents needed by M1. Call
`minidock::build_image(&rootfs_path, &image_path)` with the image outside the
rootfs context. On error, remove only `builds/<deployment_id>` and return the
original error with context.

- [x] **Step 4: Run formatting and builder tests**

Run: `cargo fmt --check && cargo test --test builder_test`

Expected: PASS and the extracted image contains both base and `/srv/app` files.

- [x] **Step 5: Commit image assembly**

```bash
git add src/lib.rs src/builder.rs tests/builder_test.rs
git commit -m "feat: assemble static minidock deployment images"
```

### Task 5: Add the runtime boundary and minidock adapter

**Files:**

- Create: `src/runtime.rs`
- Create: `tests/runtime_test.rs`
- Modify: `src/lib.rs`

**Interfaces:**

- Consumes `RunningContainer` and minidock public APIs.
- Produces `Runtime`, `MinidockRuntime`, `render_server_command`,
  `discover_new_container`, and `wait_for_port`.

- [x] **Step 1: Write failing pure runtime tests**

```rust
// tests/runtime_test.rs
use deploy_platform::runtime::{discover_new_container, render_server_command};
use minidock::state::{ContainerState, ContainerStatus};
use std::path::PathBuf;
use time::OffsetDateTime;
use uuid::Uuid;

fn state(hostname: &str, command: &[&str]) -> ContainerState {
    ContainerState {
        version: 1, id: Uuid::new_v4(), pid: 123, cgroup_path: PathBuf::new(),
        rootfs: PathBuf::new(), command: command.iter().map(|item| (*item).into()).collect(),
        hostname: hostname.into(), detached: true, started_at: OffsetDateTime::now_utc(),
        status: ContainerStatus::Running,
    }
}

#[test]
fn replaces_only_a_complete_port_token() {
    let template = ["/bin/busybox", "-p", "{PORT}", "-h", "/srv/app"]
        .into_iter().map(String::from).collect::<Vec<_>>();
    let result = render_server_command(&template, 43123).unwrap();
    assert_eq!(result, vec!["/bin/busybox", "-p", "43123", "-h", "/srv/app"]);
}

#[test]
fn rejects_missing_or_ambiguous_port_tokens() {
    let missing = vec!["server".to_string()];
    let ambiguous = vec!["server".to_string(), "{PORT}".to_string(), "{PORT}".to_string()];
    let embedded = vec!["server=:{PORT}".to_string()];
    assert!(render_server_command(&missing, 8080).unwrap_err().to_string().contains("{PORT}"));
    assert!(render_server_command(&ambiguous, 8080).unwrap_err().to_string().contains("exactly one"));
    assert!(render_server_command(&embedded, 8080).unwrap_err().to_string().contains("complete token"));
}

#[test]
fn discovers_only_the_new_matching_container_state() {
    let old = state("platform-old", &["server", "1"]);
    let new = state("platform-new", &["server", "43123"]);
    let command = vec!["server".to_string(), "43123".to_string()];
    let found = discover_new_container(&[old.clone()], &[old, new.clone()], "platform-new", &command).unwrap();
    assert_eq!(found.id, new.id);
}

#[test]
fn ambiguous_container_discovery_fails() {
    let first = state("same-hostname", &["server", "43123"]);
    let second = state("same-hostname", &["server", "43123"]);
    let command = vec!["server".to_string(), "43123".to_string()];
    let error = discover_new_container(&[], &[first, second], "same-hostname", &command).unwrap_err();
    assert!(error.to_string().contains("ambiguous"));
}
```

- [x] **Step 2: Run runtime tests and verify the missing-module failure**

Run: `cargo test --test runtime_test`

Expected: FAIL because `runtime.rs` does not exist.

- [x] **Step 3: Implement command rendering, discovery, and the trait**

```rust
// src/runtime.rs
use crate::{model::RunningContainer, Result};

pub trait Runtime: Send {
    fn start_static(&mut self, image: &Path, hostname: &str, command: &[String]) -> Result<RunningContainer>;
    fn stop(&mut self, container_id: Uuid) -> Result<()>;
    fn logs(&self, container_id: Uuid) -> Result<String>;
}

pub struct MinidockRuntime {
    pub minidock_store: minidock::StateStore,
}

pub fn render_server_command(template: &[String], port: u16) -> Result<Vec<String>>;
pub fn discover_new_container(
    before: &[minidock::ContainerState],
    after: &[minidock::ContainerState],
    hostname: &str,
    command: &[String],
) -> Result<minidock::ContainerState>;
pub fn wait_for_port(port: u16, timeout: Duration) -> Result<()>;
```

`render_server_command` replaces one exact `{PORT}` token with the decimal port,
rejects no token, multiple tokens, and embedded placeholders, and preserves every
other argv token byte-for-byte. `discover_new_container` compares UUIDs, filters
for the generated hostname, exact command, detached mode, and running/exited
states, then requires one match. `wait_for_port` retries `TcpStream::connect` to
`127.0.0.1:port` every 100ms until five seconds have elapsed.

- [x] **Step 4: Implement `MinidockRuntime::start_static`, stop, and logs**

Use this sequence in `start_static`:

1. bind `TcpListener` to `127.0.0.1:0`, record its port, then drop it;
2. render the configured server command with that port;
3. record `before = minidock_store.list()?`;
4. call `minidock::run(minidock::RunRequest { image: image.to_path_buf(), memory_bytes: None, cpu_percent: None, hostname: hostname.to_string(), detached: true, command: rendered.clone() }, minidock_store.clone())`;
5. record `after = minidock_store.list()?` and discover the new state;
6. wait for the allocated port; if health fails, call `minidock::stop` on the discovered UUID before returning the health error; and
7. return `RunningContainer { id, port, url: format!("http://127.0.0.1:{port}") }`.

`stop` loads the UUID through `minidock::StateStore` and delegates to
`minidock::stop`. `logs` opens the detached container log and reads it to a
String. Include the generated hostname in operation errors. Do not execute a
shell or parse command strings after `render_server_command`.

- [x] **Step 5: Run formatting, runtime tests, and the full current suite**

Run: `cargo fmt --check && cargo test --test runtime_test && cargo test`

Expected: PASS; no test may require root or a real minidock image.

- [x] **Step 6: Commit the runtime boundary**

```bash
git add src/lib.rs src/runtime.rs tests/runtime_test.rs
git commit -m "feat: add minidock runtime adapter"
```

### Task 6: Orchestrate projects and deployments through injectable traits

**Files:**

- Create: `src/service.rs`
- Create: `tests/service_test.rs`
- Modify: `src/lib.rs`

**Interfaces:**

- Consumes model, store, detector, builder, and runtime interfaces from Tasks 1–5.
- Produces `PlatformService`, `DeploymentService<R, B>`, and service methods for project creation, deploy, list, stop, and logs.

- [x] **Step 1: Write failing service tests with fake builder/runtime implementations**

```rust
// tests/service_test.rs, test doubles and success case
struct FakeBuilder { should_fail: bool }
#[derive(Default)]
struct FakeRuntime { started: usize, stopped: Vec<Uuid>, logs: String }

impl ImageBuilder for FakeBuilder {
    fn build(&self, _: &Path, _: Uuid, _: &StaticSource, _: &Path) -> Result<BuildOutput> {
        if self.should_fail {
            Err(anyhow::anyhow!("build failed"))
        } else {
            Ok(fake_build())
        }
    }
}

impl Runtime for FakeRuntime {
    fn start_static(&mut self, _: &Path, _: &str, _: &[String]) -> Result<RunningContainer> {
        self.started += 1;
        Ok(RunningContainer { id: Uuid::new_v4(), port: 43123, url: "http://127.0.0.1:43123".into() })
    }
    fn stop(&mut self, id: Uuid) -> Result<()> { self.stopped.push(id); Ok(()) }
    fn logs(&self, _: Uuid) -> Result<String> { Ok(self.logs.clone()) }
}

#[test]
fn deploy_transitions_to_running_and_sets_active_deployment() {
    let root = tempdir().unwrap();
    let source = root.path().join("site");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("index.html"), "hello").unwrap();
    let project = project_input(&source);
    let service = test_service(root.path(), FakeBuilder { should_fail: false }, FakeRuntime::default());
    let saved = service.create_project(project).unwrap();
    let deployment = service.deploy(saved.id).unwrap();
    assert_eq!(deployment.status, DeploymentStatus::Running);
    assert_eq!(deployment.url.as_deref(), Some("http://127.0.0.1:43123"));
    assert_eq!(service.project(saved.id).unwrap().active_deployment, Some(deployment.id));
}
```

Add separate tests for: build failure leaves the prior active deployment
unchanged; runtime failure records `Failed` and retains the error; stopping a
running deployment records `Stopped`; and logs delegate to the runtime. The fake
builder must not touch minidock privileges; use a temporary file as its
`BuildOutput.image_path`.

Define `project_input(source)` with a base image path under the test temporary
directory and the command `[/bin/busybox, httpd, -f, -p, {PORT}, -h, /srv/app]`;
define `fake_build()` with a temporary-file image path; and define
`test_service(root, builder, runtime)` to construct
`DeploymentService::new(StateStore::at(root.to_path_buf()), runtime, builder)`.

- [x] **Step 2: Run the service tests and verify the missing-service failure**

Run: `cargo test --test service_test`

Expected: FAIL because `service.rs` and its service traits do not exist.

- [x] **Step 3: Define the injectable service contracts**

```rust
// src/service.rs
pub trait PlatformService: Send + Sync {
    fn create_project(&self, input: CreateProjectInput) -> Result<Project>;
    fn project(&self, id: Uuid) -> Result<Project>;
    fn deploy(&self, project_id: Uuid) -> Result<Deployment>;
    fn deployments(&self, project_id: Uuid) -> Result<Vec<Deployment>>;
    fn deployment(&self, id: Uuid) -> Result<Deployment>;
    fn stop(&self, id: Uuid) -> Result<Deployment>;
    fn logs(&self, id: Uuid) -> Result<String>;
}

pub struct DeploymentService<R, B> {
    store: StateStore,
    runtime: std::sync::Mutex<R>,
    builder: B,
    operation_lock: std::sync::Mutex<()>,
}

impl<R: Runtime, B: ImageBuilder> DeploymentService<R, B> {
    pub fn new(store: StateStore, runtime: R, builder: B) -> Self;
}
```

Implement `PlatformService` for `DeploymentService<R, B>` with `R: Runtime + Send`
and `B: ImageBuilder + Send + Sync`. The service owns the only deployment mutex;
all project/deployment mutations occur while its guard is held.

- [x] **Step 4: Implement project creation and read/list methods**

`create_project` must reject empty or greater-than-63-byte names, canonicalize an
existing source directory and base image, reject duplicate names, require one
complete `{PORT}` token in `server_command`, create a UUID, and persist a `Project`
with `active_deployment: None`. `project`, `deployment`, and `deployments` load
records from the store; deployment lists filter by project ID and sort by
`created_at` ascending.

- [x] **Step 5: Implement deployment lifecycle and cleanup**

Use this exact flow in `deploy`:

1. lock `operation_lock`;
2. load the project and insert a `Pending` deployment with `Framework::Static`;
3. set and persist `Building`;
4. call `detect_static_source`, then `builder.build` using the store root, deployment UUID, source, and project base image; persist the resulting image path;
5. generate hostname `platform-<deployment UUID without hyphens>` and call `runtime.start_static`;
6. set `container_id`, `port`, `url`, `status: Running`, and `finished_at`, then persist;
7. if an older active deployment exists, call `runtime.stop` and mark it `Stopped`; and
8. update the project’s active deployment pointer and return the new record.

On detection/build/runtime failure, set `Failed`, `finished_at`, and a contextual
error. If runtime returned a container before a persistence failure, stop that
container before returning. Do not clear the old active deployment on a failed
new deployment. Cleanup failures are appended to the primary error.

- [x] **Step 6: Implement stop/log methods and run the service suite**

`stop` loads the deployment, returns it unchanged when already stopped/failed,
otherwise requires a container ID, calls `runtime.stop`, marks it `Stopped`, and
clears the project pointer only when it points to that deployment. `logs` requires
a container ID and delegates to `runtime.logs`.

Run: `cargo fmt --check && cargo test --test service_test && cargo test`

Expected: PASS with success, failure-preservation, stop, and logs cases green.

- [x] **Step 7: Commit the service layer**

```bash
git add src/lib.rs src/service.rs tests/service_test.rs
git commit -m "feat: orchestrate static deployments"
```

### Task 7: Expose the Axum HTTP API

**Files:**

- Create: `src/api.rs`
- Create: `tests/api_test.rs`
- Modify: `src/lib.rs`

**Interfaces:**

- Consumes the `PlatformService` trait from Task 6.
- Produces `router(Arc<dyn PlatformService>) -> axum::Router`, JSON DTOs, and one consistent API error response.

- [x] **Step 1: Write failing in-process API tests**

```rust
// tests/api_test.rs
use axum::{body::Body, http::{Request, StatusCode}};
use deploy_platform::api::router;
use http_body_util::BodyExt;
use std::sync::Arc;
use tower::ServiceExt;

#[tokio::test]
async fn health_endpoint_returns_ok_json() {
    let response = router(test_service()).oneshot(
        Request::builder().uri("/healthz").body(Body::empty()).unwrap(),
    ).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&body[..], br#"{"status":"ok"}"#);
}

#[tokio::test]
async fn unknown_project_id_returns_json_not_found() {
    let response = router(test_service()).oneshot(
        Request::builder().uri(format!("/v1/projects/{}/deployments", uuid::Uuid::new_v4()))
            .body(Body::empty()).unwrap(),
    ).await.unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    assert!(String::from_utf8_lossy(&body).contains("error"));
}
```

Add tests for `POST /v1/projects` returning `201`, `GET /v1/projects/:id`,
`POST /v1/projects/:id/deployments` returning a deployment, and malformed UUIDs
returning `400`. Define `test_service()` by reusing the Task 6 fake builder and
runtime factory, wrapping the resulting `DeploymentService` in
`Arc<dyn PlatformService>`; its empty temporary store naturally supplies the
not-found case.

- [x] **Step 2: Run API tests and verify the missing-router failure**

Run: `cargo test --test api_test`

Expected: FAIL because `api.rs` and `router` do not exist.

- [x] **Step 3: Implement DTOs, routes, and error mapping**

```rust
// src/api.rs
#[derive(Clone)]
pub struct ApiState { pub service: Arc<dyn PlatformService> }

pub fn router(service: Arc<dyn PlatformService>) -> axum::Router;

#[derive(serde::Deserialize)]
struct CreateProjectRequest { name: String, source_dir: PathBuf, base_image: PathBuf }

#[derive(serde::Serialize)]
struct ErrorResponse { error: String }
```

Register exactly these routes:

```text
GET  /healthz
POST /v1/projects
GET  /v1/projects/:project_id
GET  /v1/projects/:project_id/deployments
POST /v1/projects/:project_id/deployments
GET  /v1/deployments/:deployment_id
GET  /v1/deployments/:deployment_id/logs
POST /v1/deployments/:deployment_id/stop
```

Parse path UUIDs with `Uuid::parse_str`, call the corresponding service method,
and serialize records directly. Map malformed UUIDs to `400`, missing records to
`404`, validation/build/runtime errors from project/deploy operations to `422`,
and unexpected service errors to `500`. Every error body is
`{"error":"<message>"}`. Set `Content-Type: application/json` through Axum's
`Json` response type. The deployment endpoint has no request body in M1.

- [x] **Step 4: Run formatting, API tests, and the full suite**

Run: `cargo fmt --check && cargo test --test api_test && cargo test`

Expected: PASS with all API routes covered and no privileged setup.

- [x] **Step 5: Commit the HTTP API**

```bash
git add src/lib.rs src/api.rs tests/api_test.rs
git commit -m "feat: expose deployment lifecycle over http"
```

### Task 8: Add the CLI and server startup

**Files:**

- Modify: `Cargo.toml`
- Create: `src/main.rs`
- Create: `tests/cli_test.rs`
- Modify: `src/config.rs`, `src/service.rs`, `src/builder.rs`, `src/runtime.rs`

**Interfaces:**

- Consumes `PlatformConfig`, `DeploymentService`, `MinidockImageBuilder`, `MinidockRuntime`, and `api::router`.
- Produces the `platform` binary with `serve`, `project create`, `project show`, `deploy`, `deployments`, `logs`, and `stop` subcommands.

- [x] **Step 1: Write failing CLI validation tests**

```rust
// tests/cli_test.rs
use assert_cmd::Command;
use predicates::prelude::*;

#[test]
fn help_lists_the_m1_commands() {
    Command::cargo_bin("platform").unwrap()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("serve"))
        .stdout(predicate::str::contains("deploy"))
        .stdout(predicate::str::contains("project"));
}

#[test]
fn project_create_requires_name_source_and_base_image() {
    Command::cargo_bin("platform").unwrap()
        .args(["project", "create"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}
```

- [x] **Step 2: Run CLI tests and verify the missing-binary failure**

Run: `cargo test --test cli_test`

Expected: FAIL because `src/main.rs` does not define the binary.

- [x] **Step 3: Implement clap commands and local service construction**

Add this binary declaration to `Cargo.toml` so the package name and executable
name remain distinct:

```toml
[[bin]]
name = "platform"
path = "src/main.rs"
```

```rust
#[derive(clap::Parser)]
#[command(name = "platform", about = "Local single-node static deploy platform")]
struct Cli { #[command(subcommand)] command: Command }

#[derive(clap::Subcommand)]
enum Command {
    Serve { #[arg(long)] listen: Option<String> },
    Project(ProjectCommand),
    Deploy { project_id: String },
    Deployments { project_id: String },
    Logs { deployment_id: String },
    Stop { deployment_id: String },
}

#[derive(clap::Subcommand)]
enum ProjectCommand {
    Create { #[arg(long)] name: String, #[arg(long)] source: PathBuf, #[arg(long)] base_image: PathBuf },
    Show { project_id: String },
}
```

Build one `Arc<DeploymentService<MinidockRuntime, MinidockImageBuilder>>` from
`PlatformConfig`, `StateStore::at(config.state_dir)`,
`MinidockRuntime { minidock_store: minidock::StateStore::from_current_user()? }`,
and `MinidockImageBuilder`. Parse UUIDs before service calls. Use the same service
for CLI commands and HTTP server; CLI output should be concise JSON records or
plain logs, with errors on stderr and nonzero exit status.

For `serve`, bind `tokio::net::TcpListener` to the configured address and call
`axum::serve(listener, api::router(service)).await`. A `--listen` value overrides
`PLATFORM_LISTEN_ADDR` for that process. `#[tokio::main]` is the only async entry
point.

- [x] **Step 4: Run formatting, CLI tests, build, and clippy**

Run: `cargo fmt --check && cargo test --test cli_test && cargo build && cargo clippy --all-targets -- -D warnings`

Expected: PASS with a built `platform` binary and no clippy warnings.

- [x] **Step 5: Commit the CLI and server**

```bash
git add Cargo.toml src/main.rs src/config.rs src/service.rs src/builder.rs src/runtime.rs tests/cli_test.rs
git commit -m "feat: add platform cli and api server"
```

### Task 9: Add documentation and the opt-in privileged smoke test

**Files:**

- Create: `README.md`
- Create: `tests/privileged_runtime_test.rs`
- Modify: `.gitignore`

**Interfaces:**

- Consumes the completed CLI/API/runtime behavior from Tasks 1–8.
- Produces documented local setup and a reproducible opt-in runtime check.

- [x] **Step 1: Write the ignored smoke-test contract**

```rust
// tests/privileged_runtime_test.rs
use deploy_platform::{
    builder::MinidockImageBuilder,
    model::CreateProjectInput,
    runtime::MinidockRuntime,
    service::{DeploymentService, PlatformService},
    store::StateStore,
};
use std::{fs, io::{Read, Write}, net::TcpStream, path::PathBuf};
use tempfile::tempdir;

#[test]
#[ignore = "requires root, cgroups v1, a working minidock base image, and PLATFORM_BASE_IMAGE"]
fn real_static_deployment_serves_index_html() {
    let image = std::env::var("PLATFORM_BASE_IMAGE")
        .expect("PLATFORM_BASE_IMAGE must point to a minidock rootfs tar.gz");
    assert!(std::path::Path::new(&image).is_file());
    let root = tempdir().unwrap();
    let source = root.path().join("site");
    fs::create_dir_all(&source).unwrap();
    fs::write(source.join("index.html"), "m1 smoke test").unwrap();
    let service = DeploymentService::new(
        StateStore::at(root.path().join("platform-state")),
        MinidockRuntime { minidock_store: minidock::StateStore::from_current_user().unwrap() },
        MinidockImageBuilder,
    );
    let project = service.create_project(CreateProjectInput {
        name: "smoke".into(), source_dir: source, base_image: PathBuf::from(image),
        server_command: vec!["/bin/busybox".into(), "httpd".into(), "-f".into(),
            "-p".into(), "{PORT}".into(), "-h".into(), "/srv/app".into()],
    }).unwrap();
    let deployment = service.deploy(project.id).unwrap();
    let mut stream = TcpStream::connect(("127.0.0.1", deployment.port.unwrap())).unwrap();
    stream.write_all(b"GET /index.html HTTP/1.0\r\nConnection: close\r\n\r\n").unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.contains("m1 smoke test"));
    assert!(service.logs(deployment.id).is_ok());
    service.stop(deployment.id).unwrap();
}
```

- [x] **Step 2: Implement README usage and test prerequisites**

Document:

1. Linux/root and cgroups-v1 requirements inherited from minidock;
2. `cargo build`, `cargo test`, and clippy commands;
3. a source directory containing `index.html`;
4. how to create a base image with minidock's `build` command or BusyBox export;
5. `PLATFORM_STATE_DIR`, `PLATFORM_LISTEN_ADDR`, and `PLATFORM_SERVER_COMMAND`;
6. `platform project create --name landing --source ./site --base-image ./base.tar.gz`;
7. `platform deploy <project-id>` and the returned local URL;
8. `curl http://127.0.0.1:<port>/index.html`, `platform logs`, and `platform stop`;
9. API route examples for project creation and deployment; and
10. the exact ignored smoke-test command and why it is not part of default CI.

- [x] **Step 3: Run the complete verification suite**

Run:

```bash
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --release
git diff --check
git status --short
```

Expected: all unprivileged tests pass, clippy has zero warnings, the release
binary builds, `git diff --check` is clean, and only intended project files are
modified. Do not claim M1 is complete unless these commands have fresh successful
output.

- [x] **Step 4: Commit documentation and smoke-test coverage**

```bash
git add README.md .gitignore tests/privileged_runtime_test.rs
git commit -m "docs: document deploy platform m1 workflow"
```

## Plan Self-Review

- Spec coverage: the plan covers the separate repository boundary, model, atomic
  JSON state, safe static detection, base-rootfs/image assembly, runtime adapter,
  health check, deployment lifecycle, HTTP API, CLI, error mapping, tests, and
  README acceptance criteria. Deferred features remain absent from every task.
- Placeholder scan: no task depends on a `TBD`, `TODO`, or unspecified future
  implementation; every named interface has a concrete signature or construction
  step.
- Type consistency: `RunningContainer`, `Runtime`, `ImageBuilder`,
  `PlatformService`, `StateStore`, and all model names are introduced before their
  consumers. The plan consistently uses `DeploymentStatus`, `Framework::Static`,
  `BuildOutput`, and `PlatformConfig`.
- Test order: every production layer begins with a failing test, verifies the
  failure, implements the smallest behavior, and reruns the focused and full
  suites before committing.
