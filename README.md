# Deploy Platform (M2: Authenticated Dashboard & Control Plane)

`deploy-platform` is a standalone single-node static-site deployment platform built in Rust with an authenticated Next.js dashboard and PostgreSQL control plane. It validates local static sources, packages them with a base rootfs image, and deploys them using the `minidock` container runtime.

## Prerequisites & System Requirements

- **Rust toolchain:** Rust 1.70 or newer (`cargo`, `rustc`).
- **PostgreSQL:** PostgreSQL 14+ for control plane persistence (or via Docker).
- **Node.js & npm:** Node 18+ and npm for the dashboard frontend (`dashboard/`).
- **Operating System:** Linux is required for executing containers via `minidock`.
- **Privileges & Cgroups:** Minidock container execution requires `root` privileges and Linux **cgroups v1** (`/sys/fs/cgroup`).
- **Unprivileged Development:** Building the Rust backend, running tests, and developing the dashboard require no root privileges.

## PostgreSQL Database Setup

The control plane uses PostgreSQL as its source of truth for users, sessions, API tokens, project ownership, and audit events.

Start PostgreSQL via Docker:
```bash
docker run -d --name deploy-platform-postgres \
  -e POSTGRES_PASSWORD=postgres \
  -e POSTGRES_DB=deploy_platform \
  -p 5432:5432 \
  postgres:16
```

Set the database connection string:
```bash
export DATABASE_URL="postgres://postgres:postgres@localhost:5432/deploy_platform"
```

## Building and Testing

### Backend (Rust)
```bash
# Run all unit, repository, auth, and authenticated API tests
cargo test

# Run clippy checks with warnings denied
cargo clippy --all-targets -- -D warnings

# Check code formatting
cargo fmt --check
```

### Frontend Dashboard (Next.js)
```bash
cd dashboard

# Install dependencies
npm install

# Run component and unit tests
npm test

# Build production bundle
npm run build

# Run end-to-end browser tests
npx playwright test
```

## Running the Platform

### 1. Start the API Server
```bash
cargo run -- serve --listen 127.0.0.1:4000
```

### 2. Start the Dashboard
```bash
cd dashboard
NEXT_PUBLIC_API_URL="http://127.0.0.1:4000" npm run dev
```
Open `http://localhost:3000` to access the dashboard.

## Authentication & Authorization (M2)

- **Password Hashing:** Passwords are hashed with Argon2id using cryptographically secure random salts.
- **Session Cookies:** `POST /v1/auth/session` sets an `HttpOnly`, `SameSite=Lax` session cookie (`dp_session`).
- **CLI API Tokens:** Authenticated users can generate scoped API tokens (`dp_<hex>`). Raw tokens are returned exactly once upon creation and verified via Argon2id.
- **User Isolation:** All project and deployment operations enforce ownership. Cross-user access returns `403 Forbidden`.
- **Audit Logging:** Every mutation (project creation, deployment start/stop, token issue/revoke) creates an immutable audit event in `audit_events`.

### API Endpoints

- `POST /v1/auth/register` - Create a new user account.
- `POST /v1/auth/session` - Log in and obtain a session cookie.
- `DELETE /v1/auth/session` - Log out and invalidate the session.
- `GET /v1/me` - Get the current user profile and permissions.
- `GET /v1/me/api-tokens` - List active personal API tokens.
- `POST /v1/me/api-tokens` - Issue a new personal API token (returns raw token once).
- `DELETE /v1/me/api-tokens/:id` - Revoke an API token.
- `GET /v1/projects` - List projects belonging to the authenticated user.
- `POST /v1/projects` - Create a project.
- `GET /v1/projects/:project_id` - Get project details.
- `GET /v1/projects/:project_id/deployments` - List deployments for a project.
- `POST /v1/projects/:project_id/deployments` - Trigger a deployment.
- `GET /v1/deployments/:deployment_id` - Get deployment status and URL.
- `GET /v1/deployments/:deployment_id/logs` - Fetch deployment logs.
- `POST /v1/deployments/:deployment_id/stop` - Stop running container deployment.

## Privileged Smoke Test

An opt-in integration test runs against a real minidock runtime. It is marked `#[ignore]` by default:
```bash
sudo PLATFORM_BASE_IMAGE=/tmp/base.tar.gz cargo test --test privileged_runtime_test -- --ignored
```

