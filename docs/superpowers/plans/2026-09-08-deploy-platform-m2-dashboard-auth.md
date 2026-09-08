# Deploy Platform M2 Dashboard and Auth Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the local-only M1 workflow with an authenticated control plane, durable project/deployment persistence, and a Next.js dashboard covering project overview, deployment history, logs, environment variables, and CLI tokens.

**Architecture:** Introduce PostgreSQL-backed repositories behind the existing service interfaces, server-side sessions for dashboard access, hashed API tokens for the CLI, and a typed HTTP API. Add `dashboard/` as a separate Next.js application that consumes only the API and renders the routes defined in `FRONTEND.md`.

**Tech Stack:** Rust/Axum, PostgreSQL, SQL migrations, Argon2id, secure HTTP-only cookies, `uuid`, `time`, Next.js, TypeScript, TanStack Query, Playwright, and React Testing Library.

**Spec:** `docs/superpowers/plans/2026-09-08-deploy-platform-roadmap.md`, `FRONTEND.md`, and `docs/superpowers/specs/2026-09-08-deploy-platform-m1-design.md`.

## Global Constraints

- Preserve the M1 API behavior while adding authenticated variants.
- Store password/token hashes only; never persist or return raw secrets.
- Use PostgreSQL as the control-plane source of truth; JSON state remains readable for migration only.
- Sessions use secure, HTTP-only, same-site cookies and expire server-side.
- API tokens display their secret exactly once and are revocable by token ID.
- The dashboard calls the API server only and never accesses runtime state directly.
- Every mutation has authorization, audit metadata, validation, and an idempotency strategy.
- Default tests use a disposable database fixture or repository fakes; privileged minidock tests remain ignored.

## File Structure

| Path | Responsibility |
| --- | --- |
| `migrations/` | PostgreSQL schema for users, sessions, projects, deployments, env metadata, tokens, audit events. |
| `src/db.rs` | Pool creation, migrations, transaction helpers. |
| `src/auth.rs` | Password/provider identity, sessions, API token hashing, authorization. |
| `src/repository.rs` | Typed CRUD for control-plane entities. |
| `src/api.rs` | Auth middleware and M2 routes. |
| `src/service.rs` | Repository-backed project/deployment operations. |
| `src/main.rs` | API configuration and startup. |
| `src/bin/platform.rs` | CLI login/token/project/deployment commands. |
| `dashboard/package.json` | Next.js dashboard dependencies and scripts. |
| `dashboard/app/` | App Router pages and layouts. |
| `dashboard/components/` | Accessible shared UI components. |
| `dashboard/lib/api.ts` | Typed API client and error normalization. |
| `dashboard/tests/` | Component and Playwright tests. |

### Task 1: Add database schema and repository contract

**Files:**

- Create: `migrations/0001_control_plane.sql`
- Create: `src/db.rs`
- Create: `src/repository.rs`
- Create: `tests/repository_test.rs`
- Modify: `Cargo.toml`, `src/lib.rs`

**Interfaces:**

- `Database::connect(url) -> Result<Database>`
- `Database::migrate() -> Result<()>`
- `ProjectRepository`, `DeploymentRepository`, `UserRepository`, `AuditRepository`
- `Repository::create_project`, `get_project`, `list_projects`, `create_deployment`, `list_deployments`

- [x] **Step 1: Write failing repository tests** for project round trip, deployment status update, unique project names per user, and transaction rollback.
- [x] **Step 2: Run `cargo test --test repository_test`** and confirm missing database/repository symbols fail.
- [x] **Step 3: Add the migration tables** with UUID primary keys, UTC timestamps, unique constraints, status checks, indexes on `(user_id, created_at)` and `(project_id, created_at)`, and an audit event table.
- [x] **Step 4: Implement the pool/migration wrapper** with `sqlx` or an equivalent async PostgreSQL client and a test repository fixture.
- [x] **Step 5: Implement typed repository methods** using parameterized SQL and transactions; never concatenate user input into SQL.
- [x] **Step 6: Run `cargo fmt --check && cargo test --test repository_test`** and verify rollback leaves no partial records.
- [x] **Step 7: Commit** with `git add migrations src/db.rs src/repository.rs tests/repository_test.rs Cargo.toml src/lib.rs && git commit -m "feat: add postgres control plane repositories"`.

### Task 2: Add sessions, authorization, and CLI tokens

**Files:**

- Create: `src/auth.rs`
- Create: `tests/auth_test.rs`
- Modify: `migrations/0001_control_plane.sql`, `src/api.rs`, `Cargo.toml`

**Interfaces:**

- `AuthService::create_session`, `load_session`, `revoke_session`
- `AuthService::issue_api_token`, `revoke_api_token`, `authenticate_api_token`
- `AuthContext { user_id, session_id, scopes }`
- `require_user`, `require_scope` Axum middleware

- [x] **Step 1: Write failing tests** for cookie session expiry, token hashing, one-time raw-token return, revoked token rejection, and scope denial.
- [x] **Step 2: Run `cargo test --test auth_test`** and verify the auth layer is absent.
- [x] **Step 3: Add `users`, `sessions`, and `api_tokens` columns/tables** with hash, expiry, revocation, last-used, and scope fields.
- [x] **Step 4: Implement Argon2id hashing** with a per-token salt; compare hashes in constant time and store only a short token prefix for display.
- [x] **Step 5: Implement secure cookie sessions** with HTTP-only, Secure-in-production, SameSite=Lax attributes and server-side expiry checks.
- [x] **Step 6: Add auth routes**: `POST /v1/auth/session`, `DELETE /v1/auth/session`, `GET /v1/me`, `POST /v1/me/api-tokens`, `DELETE /v1/me/api-tokens/:id`.
- [x] **Step 7: Run focused/full auth tests** and commit `feat: add authenticated sessions and api tokens`.

### Task 3: Move project/deployment routes behind authorization

**Files:**

- Modify: `src/api.rs`, `src/service.rs`, `src/model.rs`
- Create: `tests/authenticated_api_test.rs`

**Interfaces:**

- Existing project/deployment routes accept `AuthContext`.
- `ProjectAccess::can_read`, `can_mutate`, and `can_operate` are explicit checks.
- `AuditService::record(actor, action, target, metadata)` records mutations.

- [x] **Step 1: Write failing API tests** for unauthenticated 401, authenticated ownership, cross-user 403, and audit rows.
- [x] **Step 2: Run the tests and confirm current unauthenticated behavior fails the new contract.**
- [x] **Step 3: Add middleware and access checks** to every project/deployment/environment route.
- [x] **Step 4: Preserve M1 response shapes** while adding `request_id` and authenticated actor metadata.
- [x] **Step 5: Add project/deployment audit records** for create, deploy, stop, and token changes.
- [x] **Step 6: Run `cargo test --test authenticated_api_test && cargo test`** and commit `feat: authorize control plane routes`.

### Task 4: Add the typed dashboard application

**Files:**

- Create: `dashboard/package.json`, `dashboard/tsconfig.json`, `dashboard/next.config.ts`
- Create: `dashboard/app/layout.tsx`, `dashboard/app/login/page.tsx`, `dashboard/app/dashboard/page.tsx`
- Create: `dashboard/app/projects/page.tsx`, `dashboard/app/projects/new/page.tsx`
- Create: `dashboard/app/projects/[projectId]/overview/page.tsx`
- Create: `dashboard/app/projects/[projectId]/deployments/page.tsx`
- Create: `dashboard/app/projects/[projectId]/deployments/[deploymentId]/page.tsx`
- Create: `dashboard/components/*`, `dashboard/lib/api.ts`, `dashboard/lib/query.ts`
- Create: `dashboard/tests/*`

**Interfaces:**

- `ApiClient` methods mirror `/v1/me`, projects, deployments, logs, and auth routes.
- Shared components implement the states in `FRONTEND.md`.

- [x] **Step 1: Write failing component tests** for `StatusBadge`, `DeploymentCard`, `EmptyState`, `LogViewer`, and authenticated route redirect.
- [x] **Step 2: Run `cd dashboard && npm test`** and confirm the app/test harness is absent.
- [x] **Step 3: Scaffold the Next.js app** with strict TypeScript, test scripts, API base URL, and secure session handling.
- [x] **Step 4: Implement `AppShell`, project switcher, status components, loading/error states, and responsive navigation.**
- [x] **Step 5: Implement login, dashboard, project list/create, project overview, deployment list/detail, logs, and stop/redeploy controls.**
- [x] **Step 6: Add Playwright coverage** for login, project creation, successful deployment, failed deployment, and log viewing with a mocked API fixture.
- [x] **Step 7: Run `npm test && npm run build && npx playwright test`** and commit `feat: add authenticated deployment dashboard`.

### Task 5: Add environment/token UI and M2 documentation

**Files:**

- Create: `dashboard/app/projects/[projectId]/environment/page.tsx`
- Create: `dashboard/app/account/api-tokens/page.tsx`
- Modify: `dashboard/lib/api.ts`, `FRONTEND.md`, `README.md`

- [x] **Step 1: Write failing tests** for write-only secret rendering, key validation, token one-time display, and revoke confirmation.
- [x] **Step 2: Implement environment-variable and token pages** with explicit pending/success/failure states.
- [x] **Step 3: Add API client redaction tests** proving secret values never enter query caches, URLs, or analytics payloads.
- [x] **Step 4: Document local PostgreSQL, API, dashboard, and CLI startup.**
- [x] **Step 5: Run all Rust and dashboard verification commands** and commit the M2 docs/UI changes.

## M2 Acceptance Criteria

- A user can log in, create a project, deploy it, view status/logs, and stop it.
- Cross-user project access is rejected with 403.
- Project/deployment mutations create audit events.
- Environment values are write-only when marked secret.
- CLI tokens can be issued once, used, and revoked.
- Dashboard routes match `FRONTEND.md` and work on desktop/mobile.
- Rust tests, dashboard tests, dashboard build, and API end-to-end tests pass.
