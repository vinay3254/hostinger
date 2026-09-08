# Deploy Platform M7b SSR and Autoscaling Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend the static-only platform to support long-running server-rendered applications and bounded autoscaling based on observable demand and resource health.

**Architecture:** Build detection produces an explicit runtime contract instead of guessing from a framework name. SSR deployments package a start command, port, health policy, resource limits, and graceful-shutdown behavior. The M6 scheduler manages a desired replica count; autoscaling changes that desired count through the same release/placement APIs and never bypasses health-gated traffic activation.

**Tech Stack:** Rust control plane/worker/scheduler, Next.js/Node and Python runtime templates, minidock, M4 metrics, M5 releases, M6 scheduler, and M7a router.

**Spec:** `docs/superpowers/plans/2026-09-08-deploy-platform-roadmap.md`, `FRONTEND.md`, M3b build workers, M5 releases, M6 scheduler, and M7a router.

## Global Constraints

- Runtime contracts are explicit structured data: command argv, port, health endpoint, shutdown timeout, CPU/memory, and environment target.
- SSR processes run in isolated runtime containers and never receive host filesystem access beyond declared mounts.
- Autoscaling has minimum/maximum replicas, cooldown, stabilization, and rate limits.
- Scaling decisions are explainable and persisted with metric windows and policy versions.
- A failed new replica never removes healthy capacity.
- Cost/usage limits are not silently exceeded; capacity rejection is visible.

## File Structure

| Path | Responsibility |
| --- | --- |
| `src/runtime_contract.rs` | Structured build/runtime contract. |
| `src/framework.rs` | SSR framework detection and build plan extensions. |
| `src/autoscaling.rs` | Policy evaluation, stabilization, desired replica changes. |
| `src/scaling_events.rs` | Decision/audit events. |
| `migrations/0009_ssr_scaling.sql` | Runtime contracts, scaling policies, decisions, replica targets. |
| `tests/runtime_contract_test.rs` | Validation and safe argv/port behavior. |
| `tests/framework_ssr_test.rs` | Next.js/Node/Python detection. |
| `tests/autoscaling_test.rs` | Scaling math, cooldown, bounds, failure. |
| `dashboard/app/projects/[projectId]/settings/runtime/page.tsx` | Runtime settings UI. |
| `dashboard/app/projects/[projectId]/settings/scaling/page.tsx` | Autoscaling UI. |
| `dashboard/tests/runtime-scaling.spec.ts` | SSR/scaling E2E workflows. |

### Task 1: Define runtime contracts and SSR build plans

- [ ] **Step 1: Write failing tests** for static versus server contract, argv validation, port placeholder, health policy, shutdown timeout, and resource bounds.
- [ ] **Step 2: Add runtime contract schema** with framework, command argv, working directory, port, health, resources, and graceful shutdown.
- [ ] **Step 3: Extend framework detection** for Next.js SSR, Node start scripts, and Python WSGI/ASGI entry points; reject ambiguous or unsafe commands.
- [ ] **Step 4: Add build-plan output and artifact manifest** that records runtime contract, toolchain, and expected process behavior.
- [ ] **Step 5: Run tests and commit `feat: define server runtime contracts`**.

### Task 2: Build and release long-running servers

- [ ] **Step 1: Write failing tests** for process start, health gate, graceful SIGTERM, timeout escalation, restart, and artifact/runtime mismatch.
- [ ] **Step 2: Extend build worker** to package SSR artifact and runtime contract without embedding secret values.
- [ ] **Step 3: Extend runtime agent** to start one server process per release, capture logs, enforce resources, and expose health state.
- [ ] **Step 4: Reuse M5 release controller** for health-gated traffic and M6 placement; do not add an SSR-specific traffic shortcut.
- [ ] **Step 5: Run fake-runtime integration tests and ignored privileged smoke tests**; commit `feat: run server rendered deployments`.

### Task 3: Define autoscaling policy and decision engine

- [ ] **Step 1: Write failing tests** for min/max replicas, CPU/request/latency thresholds, cooldown, stabilization window, scale-up/down limits, missing metrics, and capacity rejection.
- [ ] **Step 2: Add policy schema** with metric target, min/max, cooldown, evaluation window, step limits, and enabled state.
- [ ] **Step 3: Implement deterministic `ScalingDecision::evaluate`** using M4 rollups and an explicit policy version.
- [ ] **Step 4: Persist decision reason, input window, old/new target, and result/error.**
- [ ] **Step 5: Add a reconciliation loop** that applies desired replica count through M6 scheduler and waits for healthy capacity.
- [ ] **Step 6: Run autoscaling tests and commit `feat: add bounded autoscaling decisions`**.

### Task 4: Add runtime/scaling API and dashboard

- [ ] **Step 1: Write failing API tests** for runtime settings, scaling policy validation, preview/prod permissions, decision history, and disabled policy behavior.
- [ ] **Step 2: Add routes** `GET/PUT /v1/projects/:id/runtime`, `GET/PUT /v1/projects/:id/scaling`, and `GET /v1/projects/:id/scaling/decisions`.
- [ ] **Step 3: Implement runtime settings UI** for framework, start command fields, port, health path, resources, and graceful shutdown.
- [ ] **Step 4: Implement scaling UI** with min/max, target metric, cooldown, current/desired replicas, and decision reason.
- [ ] **Step 5: Add E2E tests** for enabling SSR, setting policy, observing a decision, and disabling scaling.
- [ ] **Step 6: Run Rust/dashboard verification and commit `feat: add ssr and autoscaling controls`**.

## M7b Acceptance Criteria

- A supported SSR app builds into an explicit runtime contract and starts in an isolated container.
- Health-gated traffic remains available during process restart and scaling changes.
- Autoscaling decisions obey bounds, cooldowns, stabilization, and capacity constraints.
- Users can understand why replicas changed and can disable scaling safely.
- Static deployments continue to use the simpler static runtime path.
