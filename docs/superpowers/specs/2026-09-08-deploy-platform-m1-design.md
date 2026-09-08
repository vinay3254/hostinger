# Deploy Platform M1 Design

## Purpose

Deploy Platform is a separate Rust project that provides the first usable slice
of the deploy-platform architecture described in
`/home/vinay/Downloads/deploy-platform-architecture.md`. M1 is a local, single-node
static-site deployment path built on top of the existing `minidock` runtime:

```text
local source directory
        |
        v
static validation -> rootfs/image build -> minidock container -> HTTP health check
        |
        v
deployment record and local URL
```

The project lives at `/home/vinay/deploy-platform` and is independent from the
`/home/vinay/minidock` repository. During local development it uses minidock as a
Cargo path dependency. The runtime adapter is deliberately isolated so a future
minidock agent or HTTP control API can replace the path dependency without
changing the API, store, or builder layers.

## M1 Success Case

Given a directory containing an `index.html`, a configured base rootfs image, and
a base image containing a static HTTP server such as BusyBox `httpd`, the user can:

1. create a project pointing at the source directory;
2. request a deployment through the CLI or HTTP API;
3. observe a successful deployment record with a local URL;
4. fetch the deployed `index.html` over HTTP; and
5. inspect logs or stop the deployment.

The deployment service is synchronous from the caller's perspective in M1. It
serializes deployment operations so discovery of the container created by the
current minidock API cannot race another local deployment.

## Scope and Constraints

- Rust 1.70 or newer, matching minidock's toolchain floor.
- Linux only for the production runtime path because minidock requires Linux
  namespaces and cgroups.
- A local source directory is the only source integration in M1. Git clone,
  GitHub/GitLab webhooks, and commit/branch metadata are deferred.
- Static sites are the only supported framework. A source must contain a regular
  `index.html` at its root; unsupported source shapes fail with an explicit error.
- No shell is used to execute build or server commands. Commands are argv arrays,
  and `{PORT}` is replaced as a single argument token before execution.
- The base rootfs image is a trusted local gzip-compressed tar archive understood
  by minidock. It must contain the configured HTTP server command.
- No authentication, secrets, TLS, custom domains, preview deployments, queue,
  dashboard, multi-node scheduling, autoscaling, or SSR is included in M1.
- The default test suite must not require root, cgroups, or a base image fixture.
  Runtime integration tests that need a real minidock container are opt-in and
  ignored by default.

## Workspace and Dependency Boundary

The new repository is a standalone Cargo package named `deploy-platform`. Its
development manifest uses:

```toml
minidock = { path = "../minidock" }
```

This dependency is used only by `runtime.rs` and the image-building adapter. No
other module imports minidock types. The project therefore has a single runtime
boundary:

```text
API / CLI -> deployment service -> Runtime trait -> MinidockRuntime -> minidock
```

The path dependency is a development arrangement, not a requirement that the
two repositories be merged. A later runtime-agent implementation can satisfy the
same trait over HTTP or gRPC.

## Components and Module Boundaries

```text
deploy-platform/
├── Cargo.toml
├── src/
│   ├── lib.rs          module exports and shared result types
│   ├── main.rs         clap CLI and process exit handling
│   ├── api.rs          Axum routes, request/response JSON, HTTP errors
│   ├── config.rs       environment and project runtime configuration
│   ├── model.rs        Project, Deployment, status, and API DTO types
│   ├── store.rs        atomic JSON persistence under ~/.deploy-platform
│   ├── detector.rs     static-source validation and framework detection
│   ├── builder.rs      safe source copy, base-rootfs assembly, image creation
│   ├── runtime.rs      Runtime trait and MinidockRuntime implementation
│   └── service.rs      deployment orchestration and lifecycle transitions
└── tests/
    ├── detector_test.rs
    ├── store_test.rs
    ├── builder_test.rs
    ├── runtime_test.rs
    ├── service_test.rs
    └── api_test.rs
```

Each module has one responsibility:

- `model` contains serializable records and status transitions but performs no I/O.
- `store` owns the filesystem layout and atomic reads/writes.
- `detector` inspects a source directory without running user code.
- `builder` creates a deployable minidock image from a trusted base image and
  validated static files.
- `runtime` owns all minidock calls, container-ID discovery, port allocation, and
  log/stop operations.
- `service` coordinates model, store, builder, and runtime operations.
- `api` and `main` translate external requests into service calls and render output.

## Configuration

The service reads these environment variables at startup:

| Variable | Required | Meaning |
| --- | --- | --- |
| `PLATFORM_STATE_DIR` | No | State root; defaults to `$HOME/.deploy-platform`. |
| `PLATFORM_LISTEN_ADDR` | No | API bind address; defaults to `127.0.0.1:8787`. |
| `PLATFORM_SERVER_COMMAND` | No | ASCII-whitespace-separated argv template; quotes are not interpreted; defaults to `/bin/busybox httpd -f -p {PORT} -h /srv/app`. |

Each project supplies its base image path when it is created. The server command
is stored as an argv vector in the project record after parsing configuration once;
there is no shell interpretation during a deployment.

The CLI also accepts explicit values for state directory, base image, and server
command where needed for local workflows. Environment configuration is used by
the API server and is not silently mixed with a project-specific value.

## Persistent Data Model

State is stored below the configured state root:

```text
~/.deploy-platform/
├── state.json
├── builds/<deployment-id>/rootfs/
└── builds/<deployment-id>/image.tar.gz
```

`state.json` is written atomically through a same-directory temporary file and
rename. UUIDs are parsed before being used in any path. The schema is versioned
from its first write.

```rust
struct PlatformState {
    version: u8,
    projects: Vec<Project>,
    deployments: Vec<Deployment>,
}

struct Project {
    id: Uuid,
    name: String,
    source_dir: PathBuf,
    base_image: PathBuf,
    server_command: Vec<String>,
    created_at: OffsetDateTime,
    active_deployment: Option<Uuid>,
}

struct Deployment {
    id: Uuid,
    project_id: Uuid,
    framework: Framework,
    status: DeploymentStatus,
    image_path: Option<PathBuf>,
    container_id: Option<Uuid>,
    port: Option<u16>,
    url: Option<String>,
    created_at: OffsetDateTime,
    finished_at: Option<OffsetDateTime>,
    error: Option<String>,
}
```

`Framework` contains `Static`. `DeploymentStatus` contains `Pending`, `Building`,
`Running`, `Failed`, and `Stopped`. A failed deployment retains its error and
build artifact path, if one was created, for inspection. An active project points
to at most one running deployment in M1.

## Source Detection and Image Build

`detector::detect_static_source(source_dir)` performs these checks:

1. the source path exists and is a directory;
2. `source_dir/index.html` exists and is a regular file; and
3. every copied entry is inside the canonical source directory, is a regular file
   or directory, and is not a symlink or special file.

The detector returns a `StaticSource` containing the canonical directory and a
list of relative files. It does not run package managers or inspect executable
scripts. A source with no root `index.html` returns an unsupported-framework error.

`builder::build_image` creates a per-deployment staging directory, extracts the
configured base image using `minidock::extract_rootfs`, copies the validated source
files to `/srv/app`, and packages the resulting rootfs with
`minidock::build_image`. The source tree is copied without following symlinks,
skips the source `.git` directory, and cannot write outside `/srv/app` in the
staging rootfs. The output image is stored
under `builds/<deployment-id>/image.tar.gz`.

The builder returns the image path and never starts a process. If any build step
fails, the service marks the deployment failed and removes only the current
deployment's staging directory.

## Runtime Adapter

The runtime boundary is:

```rust
trait Runtime {
    fn start_static(
        &mut self,
        image: &Path,
        hostname: &str,
        server_command: &[String],
    ) -> Result<RunningContainer>;
    fn stop(&mut self, container_id: Uuid) -> Result<()>;
    fn logs(&self, container_id: Uuid) -> Result<String>;
}
```

`MinidockRuntime` implements this boundary by constructing a minidock
`RunRequest` with detached mode enabled. It allocates a local free TCP port, replaces
the exact `{PORT}` token in the configured command with that port, and starts the
container with the generated deployment hostname.

The current minidock library returns the exit code rather than the generated
container UUID. For M1, `MinidockRuntime` uses a unique hostname and the minidock
`StateStore` to identify the new detached state record after `minidock::run`
returns. Deployment operations are serialized, and ambiguous discovery is a hard
error. This adapter limitation is isolated so a future minidock control API can
return a handle directly.

After start, the runtime retries a TCP connection to `127.0.0.1:<port>` for up to
five seconds. Failure marks the deployment failed and stops the discovered
container. A successful start returns:

```rust
struct RunningContainer {
    id: Uuid,
    port: u16,
    url: String,
}
```

## Deployment Service Flow

`service::deploy(project_id)` performs this sequence under a deployment mutex:

1. load the project and create a `Pending` deployment record;
2. transition to `Building` and run source detection/image assembly;
3. transition to runtime startup and start the image through `Runtime`;
4. perform the runtime health check;
5. persist `Running`, URL, port, and container ID;
6. stop the previous active deployment after the new deployment is healthy; and
7. set the project's active deployment to the new deployment.

If build or runtime startup fails, the deployment becomes `Failed` with a
user-readable error and the previous active deployment remains unchanged. If the
new container starts but health checking fails, the service attempts to stop it
before recording failure. Cleanup errors are appended to the failure message and
never replace the primary cause.

`service::stop(deployment_id)` stops a running container, marks the deployment
`Stopped`, and clears the project's active-deployment pointer when it refers to
that deployment. `service::logs(deployment_id)` reads logs through the runtime
adapter and rejects deployments that have no container ID.

## HTTP API

The API binds to `PLATFORM_LISTEN_ADDR` and exposes JSON routes below. All UUID
parameters are parsed before store access.

### Health

```text
GET /healthz
200 {"status":"ok"}
```

### Projects

```text
POST /v1/projects
{
  "name": "landing",
  "source_dir": "/absolute/path/to/site",
  "base_image": "/absolute/path/to/base-rootfs.tar.gz"
}
```

The server command comes from configuration for this endpoint. The response is
`201 Created` with the complete project record.

```text
GET /v1/projects/:project_id
GET /v1/projects/:project_id/deployments
```

### Deployments

```text
POST /v1/projects/:project_id/deployments
```

The request has no body in M1. The response is `201 Created` with the completed
deployment record, or `422 Unprocessable Entity` when detection, build, or runtime
startup fails.

```text
GET  /v1/deployments/:deployment_id
GET  /v1/deployments/:deployment_id/logs
POST /v1/deployments/:deployment_id/stop
```

Errors use one shape:

```json
{"error":"deployment failed: source does not contain a root index.html"}
```

The API does not expose arbitrary command execution, raw environment values, or
filesystem contents beyond deployment logs and records.

## CLI

The `platform` binary provides:

```text
platform serve [--listen ADDR]
platform project create --name NAME --source DIR --base-image IMAGE
platform project show PROJECT_ID
platform deploy PROJECT_ID
platform deployments PROJECT_ID
platform logs DEPLOYMENT_ID
platform stop DEPLOYMENT_ID
```

The CLI uses the same service layer and local state store rather than making HTTP
requests to a separately running server. This keeps the first local workflow
usable without a daemon while preserving the API boundary for the dashboard and
future CLI-over-HTTP implementation.

## Error Handling and Safety

- Paths are canonicalized and validated before source traversal or image extraction.
- Archive and source operations reject absolute paths, traversal, symlinks, device
  nodes, and FIFOs in the M1 source copy path.
- Project names are non-empty, limited to 63 bytes, and stored as data only.
- `{PORT}` must appear as an entire argv token; embedded substitutions are rejected.
- UUIDs are parsed before constructing state or build paths.
- The state store uses atomic replacement and preserves a valid previous state if a
  write fails.
- User-visible errors include the failed operation but do not expose secrets; M1
  has no secret-management surface.
- Runtime cleanup is best effort and scoped to the deployment being created.

## Test Strategy

The default suite is unprivileged and uses temporary directories plus a fake
`Runtime` implementation.

1. Detector tests cover valid static sources, missing `index.html`, symlink
   rejection, special-file rejection, and source-root containment.
2. Store tests cover atomic persistence, round trips, missing IDs, status updates,
   and schema/version serialization.
3. Builder tests create a small base image with minidock's image builder and verify
   static files land at `/srv/app` without executing a server.
4. Runtime tests cover exact argv port substitution, placeholder validation, free
   port handling, and minidock-state discovery logic without starting a container.
5. Service tests cover successful deployment, build failure preserving the previous
   deployment, runtime failure cleanup, stop, and log retrieval.
6. API tests exercise health, project creation, deployment listing, validation
   errors, and UUID-not-found responses through an in-process Axum router.
7. An ignored privileged smoke test is allowed to run a real BusyBox/minidock
   deployment when `PLATFORM_BASE_IMAGE` is provided on a supported host.

## Acceptance Criteria

- `/home/vinay/deploy-platform` is a separate Git repository with a documented
  local minidock dependency boundary.
- `cargo fmt --check`, `cargo test`, and `cargo clippy --all-targets -- -D warnings`
  pass without root privileges.
- A valid static source can be represented as a project and produces a deployment
  record through both the service layer and HTTP API.
- A configured privileged host can build the image, start it through minidock,
  return a local URL, serve `index.html`, read logs, and stop the deployment.
- Unsupported frameworks, invalid paths, build errors, runtime errors, and missing
  deployments return explicit failures without corrupting existing state.
- The implementation does not add queueing, authentication, webhooks, TLS,
  dashboard code, or multi-node scheduling before the M1 acceptance criteria are
  met.

## Alternatives Considered

1. **Modify minidock first to expose a direct container handle.** Rejected for M1
   because it couples the new platform's first milestone to a second repository's
   API change. The adapter's serialized hostname/state discovery is sufficient for
   the local, single-operation scope and can be replaced later.
2. **Use a database and background queue immediately.** Rejected because M1 has no
   concurrent workers or webhook traffic. Atomic JSON state keeps the first slice
   inspectable and minimizes operational dependencies.
3. **Implement the API in Node/Fastify.** Rejected because the existing runtime and
   stated preferred stack are Rust, and a Rust service can share types and avoid a
   second language in the first deployment path.
4. **Run static files directly on the host.** Rejected because M1 exists to prove
   the build-to-minidock runtime boundary; host serving would not validate the
   core architecture.
