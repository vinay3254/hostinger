# Deploy Platform M5 Zero-Downtime Deploy and Rollback Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Release new containers behind a health gate, switch traffic without dropping the current healthy release, drain the old release, and provide auditable rollback as a new deployment operation.

**Architecture:** Introduce a release controller and traffic-router interface between the deployment service and runtime. A release moves through `starting → health_check → ready → active → draining → stopped`; only the router changes active traffic. Rollback resolves a previous healthy artifact and runs the same release process with a rollback cause.

**Tech Stack:** Rust, PostgreSQL transactions, minidock/runtime trait, router interface, health-check HTTP client, Tokio timeouts, and dashboard deployment actions.

**Spec:** `docs/superpowers/plans/2026-09-08-deploy-platform-roadmap.md`, `FRONTEND.md`, M3b, and M4 observability contracts.

## Global Constraints

- Never remove the current active release before the replacement passes health checks.
- Health checks have bounded timeout, retry count, expected status, and optional body/header assertions.
- Traffic activation and database state must be recoverable after process interruption.
- Drain has a timeout and records forced termination separately from graceful stop.
- Rollback creates a new deployment/release record and preserves the failed/current release history.
- Only authorized production operators can activate or rollback production traffic.

## File Structure

| Path | Responsibility |
| --- | --- |
| `src/releases.rs` | Release state machine and durable transitions. |
| `src/health.rs` | Health-check policy and retry logic. |
| `src/traffic.rs` | `TrafficRouter` trait and route activation contract. |
| `src/rollback.rs` | Previous-release selection and rollback command. |
| `migrations/0006_releases.sql` | Releases, health checks, traffic transitions, rollback operations. |
| `tests/releases_test.rs` | State transitions/recovery. |
| `tests/health_test.rs` | Retry/timeout/validation behavior. |
| `tests/rollback_test.rs` | Eligibility, audit, and exact artifact selection. |
| `dashboard/components/ReleaseProgress.tsx` | Live release state. |
| `dashboard/components/RollbackDialog.tsx` | Production rollback confirmation. |

### Task 1: Define release and traffic contracts

- [x] **Step 1: Write failing tests** for valid/invalid release transitions, duplicate activation, crash recovery from each intermediate state, and stale release version.
- [x] **Step 2: Add release/traffic migration** with unique active route per project/environment and append-only transition records.
- [x] **Step 3: Implement `ReleaseController`** with transactional transition methods and compare-and-set version checks.
- [x] **Step 4: Define:**

```rust
pub trait TrafficRouter {
    fn prepare(&mut self, release: &Release) -> Result<RouteHandle>;
    fn activate(&mut self, route: &RouteHandle) -> Result<()>;
    fn drain(&mut self, route: &RouteHandle, timeout: Duration) -> Result<DrainResult>;
    fn remove(&mut self, route: &RouteHandle) -> Result<()>;
}
```

- [x] **Step 5: Run tests and commit `feat: add release and traffic contracts`**.

### Task 2: Implement health-gated release orchestration

- [x] **Step 1: Write failing tests** for healthy replacement, timeout, non-2xx response, crash during activation, and old-release preservation.
- [x] **Step 2: Implement `HealthPolicy`** with path, port, interval, timeout, attempts, expected status, and startup grace period.
- [x] **Step 3: Implement `wait_until_healthy`** with cancellation and structured attempt records.
- [x] **Step 4: Orchestrate start → health → prepare → activate → drain → stop** with durable transition after each external side effect.
- [x] **Step 5: Add recovery worker** that reconciles `starting`, `ready`, or `draining` releases after restart.
- [x] **Step 6: Run tests with fake runtime/router and commit `feat: add health gated releases`**.

### Task 3: Implement rollback

- [ ] **Step 1: Write failing tests** for no predecessor, failed predecessor, exact artifact reuse, production permission, and rollback audit event.
- [ ] **Step 2: Implement `select_rollback_target(project, environment, deployment_id)`** using only healthy releases with retained artifacts.
- [ ] **Step 3: Create a new deployment with cause `rollback`** and source/artifact references copied immutably from the target.
- [ ] **Step 4: Run it through the normal health-gated release controller** and record previous/new active release IDs.
- [ ] **Step 5: Run rollback tests and commit `feat: add auditable deployment rollback`**.

### Task 4: Add API and dashboard controls

- [ ] **Step 1: Write failing API tests** for release state, health attempts, activate, stop, and rollback authorization.
- [ ] **Step 2: Add `POST /v1/deployments/:id/rollback`, `GET /v1/deployments/:id/releases`, and `GET /v1/releases/:id/events`.**
- [ ] **Step 3: Implement live release progress** on deployment detail with traffic/health/drain states.
- [ ] **Step 4: Implement rollback dialog** with target commit, age, environment, actor confirmation, and failure recovery.
- [ ] **Step 5: Add E2E tests** for successful zero-downtime deploy and rollback after a failed release.
- [ ] **Step 6: Run Rust/dashboard checks and commit `feat: add zero downtime dashboard controls`**.

## M5 Acceptance Criteria

- A new release is health-checked before traffic changes.
- Users never lose the previous healthy release because a new deployment failed.
- Old releases drain with bounded cleanup and visible status.
- Rollback is a new auditable operation using an exact previous artifact.
- Interrupted release operations reconcile safely after service restart.
