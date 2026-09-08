# Deploy Platform M6 Multi-Node Scheduler and Agent Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Run deployments across multiple minidock nodes with authenticated agents, capacity-aware placement, heartbeats, health-based rescheduling, and operator visibility.

**Architecture:** A scheduler owns placement decisions and desired release state. Each node runs a small agent exposing authenticated control RPCs for create/start/stop/logs/stats and sending heartbeats. The scheduler uses leases and idempotency keys so a node or scheduler restart cannot create uncontrolled duplicate releases.

**Tech Stack:** Rust/Tokio, PostgreSQL, tonic or authenticated HTTP/2, mTLS or signed node tokens, minidock runtime, weighted bin-packing, and the M5 release/traffic contracts.

**Spec:** `docs/superpowers/plans/2026-09-08-deploy-platform-roadmap.md`, `FRONTEND.md`, and M5 release contracts.

## Global Constraints

- Node agents never receive user credentials or arbitrary host commands.
- Every agent RPC has an operation ID, timeout, idempotency key, and bounded payload.
- Scheduler state is durable; in-memory capacity is advisory and refreshed by heartbeat.
- A node is not eligible for placement until its identity and heartbeat are verified.
- Lost-heartbeat handling must avoid immediate duplicate placement during transient network failure.
- Runtime logs/stats are scoped to deployment and node permissions.
- Node drain is explicit and prevents new placement while allowing controlled migration.

## File Structure

| Path | Responsibility |
| --- | --- |
| `src/agent_protocol.rs` | Versioned RPC request/response types. |
| `src/scheduler.rs` | Placement, desired state, leases, rescheduling. |
| `src/node_registry.rs` | Node identity, heartbeat, capacity, drain state. |
| `src/agent_client.rs` | Authenticated scheduler-to-agent client. |
| `src/bin/platform-agent.rs` | Node agent process and runtime adapter. |
| `migrations/0007_nodes_scheduler.sql` | Nodes, leases, placements, operation history. |
| `tests/scheduler_test.rs` | Packing, draining, failure/reschedule. |
| `tests/agent_protocol_test.rs` | Auth/version/idempotency. |
| `tests/node_registry_test.rs` | Heartbeat/expiry/capacity. |
| `dashboard/app/admin/nodes/page.tsx` | Node operations page. |
| `dashboard/tests/nodes.spec.ts` | Admin node workflows. |

### Task 1: Define agent protocol and node identity

- [ ] **Step 1: Write failing protocol tests** for version mismatch, operation ID dedupe, invalid signature, forbidden command, and bounded request size.
- [ ] **Step 2: Define RPCs:** `RegisterNode`, `Heartbeat`, `CreateRelease`, `StartRelease`, `StopRelease`, `DrainRelease`, `ReleaseLogs`, `ReleaseStats`, and `Health`.
- [ ] **Step 3: Add node identity/certificate/token storage** with rotation and revocation state.
- [ ] **Step 4: Implement protocol serialization and auth interceptors**; reject unknown versions and replayed operation IDs.
- [ ] **Step 5: Run tests and commit `feat: define authenticated node agent protocol`**.

### Task 2: Implement node registry and heartbeats

- [ ] **Step 1: Write failing tests** for register, heartbeat update, stale heartbeat, drain, disable, capacity validation, and identity rotation.
- [ ] **Step 2: Add node/heartbeat schema** with status, capacity, observed utilization, last heartbeat, drain flag, and version.
- [ ] **Step 3: Implement `NodeRegistry`** with heartbeat compare-and-set and expiry evaluation.
- [ ] **Step 4: Add scheduler events** for online/degraded/offline/draining transitions.
- [ ] **Step 5: Run tests and commit `feat: track node health and capacity`**.

### Task 3: Implement agent process

- [ ] **Step 1: Write failing agent tests** using a fake runtime for create/start/stop/logs/stats and cancellation.
- [ ] **Step 2: Implement agent server** that maps only protocol commands to the runtime trait and applies per-release resource limits.
- [ ] **Step 3: Add graceful shutdown** that stops accepting new releases and reports draining state.
- [ ] **Step 4: Add installation/configuration docs** for node identity, endpoint, credentials, and health checks.
- [ ] **Step 5: Run agent tests and commit `feat: add minidock node agent`**.

### Task 4: Implement scheduler placement and rescheduling

- [ ] **Step 1: Write failing tests** for CPU/memory bin-packing, capacity rejection, anti-affinity, draining nodes, lease expiry, and failed-start reschedule.
- [ ] **Step 2: Implement `PlacementRequest`, `PlacementDecision`, and a deterministic weighted bin-packing scorer.**
- [ ] **Step 3: Persist desired release/node placement and operation lease** before calling the agent.
- [ ] **Step 4: Reconcile desired state after scheduler restart** and retry only idempotent incomplete operations.
- [ ] **Step 5: Add node failure handling** with grace period, replacement decision, and M5 traffic release reconciliation.
- [ ] **Step 6: Run scheduler tests and commit `feat: add multi node scheduler`**.

### Task 5: Add admin API/dashboard

- [ ] **Step 1: Write failing API tests** for node list/detail, drain, disable, release placement, and admin authorization.
- [ ] **Step 2: Add `/v1/nodes`, `/v1/nodes/:id`, `/v1/nodes/:id/drain`, and `/v1/nodes/:id/enable`.**
- [ ] **Step 3: Implement dashboard node table/detail** with heartbeat, capacity, utilization, running releases, and placement history.
- [ ] **Step 4: Add confirmation flows for drain/disable** and explain application versus node health separately.
- [ ] **Step 5: Run dashboard/API E2E tests and commit `feat: add scheduler operations dashboard`**.

## M6 Acceptance Criteria

- Two or more authenticated nodes can register and receive releases.
- Scheduler placement respects capacity and draining state.
- Agent operations are authenticated, idempotent, bounded, and auditable.
- Lost nodes produce visible degraded state and controlled rescheduling.
- Operators can inspect and drain nodes without creating new placement races.
