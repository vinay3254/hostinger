# Deploy Platform (M1)

`deploy-platform` is a standalone single-node static-site deployment platform built in Rust. It validates local static sources, packages them with a base rootfs image, and deploys them using the `minidock` container runtime.

## Prerequisites & System Requirements

- **Rust toolchain:** Rust 1.70 or newer (`cargo`, `rustc`).
- **Operating System:** Linux is required for executing containers via `minidock`.
- **Privileges & Cgroups:** Minidock container execution requires `root` privileges and Linux **cgroups v1** (`/sys/fs/cgroup`).
- **Unprivileged Development:** Building the codebase and running the test suite requires no privileges and runs on any standard development environment.

## Building and Testing

```bash
# Build the project (debug)
cargo build

# Run unprivileged test suite
cargo test

# Run clippy checks with warnings denied
cargo clippy --all-targets -- -D warnings

# Build optimized release binary
cargo build --release
```

## Base Image Preparation

The platform packages static sites on top of a trusted gzip-compressed base rootfs tarball containing a static web server (such as BusyBox `httpd`).

You can construct a base image with `minidock`:
```bash
# Prepare a minimal context directory with busybox
mkdir -p /tmp/base-context/bin
cp /bin/busybox /tmp/base-context/bin/busybox

# Create the compressed rootfs archive using minidock
cargo run --manifest-path ../minidock/Cargo.toml -- build --context /tmp/base-context --output /tmp/base.tar.gz
```

## Configuration

Environment variables can configure the platform defaults:

| Variable | Default | Description |
|---|---|---|
| `PLATFORM_STATE_DIR` | `$HOME/.deploy-platform` | Directory where platform state (`state.json`) and builds are stored. |
| `PLATFORM_LISTEN_ADDR` | `127.0.0.1:8787` | Default HTTP address and port for the Axum API server. |
| `PLATFORM_SERVER_COMMAND` | `/bin/busybox httpd -f -p {PORT} -h /srv/app` | Server command template. Must contain exactly one `{PORT}` token. |

## Local CLI Workflow

### 1. Prepare Static Source Directory
A static source must contain a regular `index.html` at its root:
```bash
mkdir -p ./site
echo "<h1>Hello from Deploy Platform</h1>" > ./site/index.html
```

### 2. Create a Project
```bash
platform project create \
  --name landing \
  --source ./site \
  --base-image /tmp/base.tar.gz
```
Output:
```json
{
  "id": "123e4567-e89b-12d3-a456-426614174000",
  "name": "landing",
  "source_dir": "/absolute/path/to/site",
  "base_image": "/tmp/base.tar.gz",
  "server_command": [
    "/bin/busybox",
    "httpd",
    "-f",
    "-p",
    "{PORT}",
    "-h",
    "/srv/app"
  ],
  "created_at": "2026-09-08T20:00:00Z",
  "active_deployment": null
}
```

### 3. Deploy the Project
Deploy using the returned project ID:
```bash
platform deploy 123e4567-e89b-12d3-a456-426614174000
```
Output:
```json
{
  "id": "987fcdeb-51a2-43f1-b9cd-567890abcdef",
  "project_id": "123e4567-e89b-12d3-a456-426614174000",
  "framework": "static",
  "status": "running",
  "image_path": "/home/user/.deploy-platform/builds/987fcdeb-51a2-43f1-b9cd-567890abcdef/image.tar.gz",
  "container_id": "a1b2c3d4-e5f6-7a8b-9c0d-1e2f3a4b5c6d",
  "port": 43123,
  "url": "http://127.0.0.1:43123",
  "created_at": "2026-09-08T20:01:00Z",
  "finished_at": "2026-09-08T20:01:02Z",
  "error": null
}
```

### 4. Verify, Inspect Logs, and Stop
```bash
# Fetch the deployed page
curl http://127.0.0.1:43123/index.html

# View deployment logs
platform logs 987fcdeb-51a2-43f1-b9cd-567890abcdef

# Stop deployment
platform stop 987fcdeb-51a2-43f1-b9cd-567890abcdef
```

## HTTP API Server

Start the API daemon:
```bash
platform serve --listen 127.0.0.1:8787
```

### API Endpoints

- `GET /healthz` - Health check.
- `POST /v1/projects` - Create a project.
  ```bash
  curl -X POST http://127.0.0.1:8787/v1/projects \
    -H "Content-Type: application/json" \
    -d '{
      "name": "landing",
      "source_dir": "/absolute/path/to/site",
      "base_image": "/tmp/base.tar.gz"
    }'
  ```
- `GET /v1/projects/:project_id` - Get project details.
- `GET /v1/projects/:project_id/deployments` - List project deployments.
- `POST /v1/projects/:project_id/deployments` - Trigger a new deployment.
  ```bash
  curl -X POST http://127.0.0.1:8787/v1/projects/123e4567-e89b-12d3-a456-426614174000/deployments
  ```
- `GET /v1/deployments/:deployment_id` - Get deployment status.
- `GET /v1/deployments/:deployment_id/logs` - Fetch deployment container logs.
- `POST /v1/deployments/:deployment_id/stop` - Stop running deployment.

## Privileged Smoke Test

An opt-in integration test runs against a real minidock runtime. It is marked `#[ignore]` by default so standard CI and unprivileged test runs remain fast and safe.

To run the privileged test:
```bash
sudo PLATFORM_BASE_IMAGE=/tmp/base.tar.gz cargo test --test privileged_runtime_test -- --ignored
```
