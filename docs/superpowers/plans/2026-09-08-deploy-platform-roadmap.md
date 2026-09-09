# Deploy Platform Remaining Roadmap

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement each milestone task-by-task. Steps use checkbox (`- [ ]`) syntax when a milestone plan is executed.

**Goal:** Define the executable order for the remaining Deploy Platform work after M1, with one independently testable plan per subsystem and explicit contracts between them.

**Architecture:** Keep the existing M1 service/API/runtime boundary, then split production responsibilities into an API/control-plane service, a durable database, Redis-backed build queue, build workers, scheduler, node agents, edge router, and Next.js dashboard. Every subsystem communicates through versioned typed events or HTTP/gRPC contracts; no frontend or worker talks directly to a runtime node.

**Tech Stack:** Rust 1.70+ for control-plane, queue, worker, scheduler, router, and agent services; PostgreSQL for durable control-plane state; Redis Streams for build jobs; object storage-compatible artifact/log storage; Next.js/TypeScript for the dashboard; tonic or HTTP/2 for node-agent control; Hyper/Tokio and rustls for the edge router.

**Spec:** `docs/superpowers/specs/2026-09-08-deploy-platform-m1-design.md`, `FRONTEND.md`, and the original deploy-platform architecture note.

## Current Baseline

- M1 static local deploy flow is implemented in `/home/vinay/deploy-platform`.
- Current persisted state is atomic JSON and current runtime access is through the minidock Rust path dependency.
- Current API supports project creation, deployment, status, logs, and stop.
- Current tests are unprivileged; the real minidock smoke test remains opt-in.
- The GitHub repository is `vinay3254/hostinger`, with one-file commits for the current tree.

## Shared Contracts

These names and transitions are shared by the remaining plans:

```text
Project       { id: UUID, name, source, build_config, runtime_config }
Deployment    { id: UUID, project_id, type, commit, status, artifact, release }
BuildJob      { id: UUID, deployment_id, priority, attempt, source_ref }
Preview       { id: UUID, project_id, pull_request, deployment_id, hostname }
Node          { id: UUID, name, endpoint, capacity, status, heartbeat_at }
Release       { id: UUID, deployment_id, node_id, runtime_handle, status }
Domain        { id: UUID, project_id, hostname, verification, tls, route }
Environment   { project_id, key, target, encrypted_value, version }
```

Deployment status is one of `pending`, `queued`, `building`, `packaging`,
`starting`, `health_check`, `live`, `failed`, `stopped`, `cancelled`, or
`superseded`. A deployment records immutable source/build facts and mutable
release/traffic state separately so rollback creates a new operation without
rewriting history.

Every asynchronous service emits:

```json
{
  "event_id": "uuid",
  "type": "deployment.stage.updated",
  "project_id": "uuid",
  "deployment_id": "uuid",
  "occurred_at": "rfc3339",
  "sequence": 42,
  "payload": {}
}
```

Event consumers must be idempotent by `event_id` and preserve ordering by
`deployment_id` plus `sequence`. Secrets never appear in event payloads or logs.

## Execution Order

```text
M1 complete
  |
  +--> M2 auth/dashboard/control-plane persistence
  |       |
  |       +--> M3a Git providers/webhooks
  |       |       |
  |       |       +--> M3b queue/build workers
  |       |               |
  |       |               +--> M3c previews
  |       |
  |       +--> M4 cache + logs/metrics
  |       |
  |       +--> M5 zero-downtime + rollback
  |                       |
  |                       +--> M6 multi-node scheduler/agents
  |                               |
  |                               +--> M7a router/domains/TLS
  |                               +--> M7b SSR/autoscaling
```

M2 is the first plan to execute. M3a can begin after M2's project/source schema
exists, but previews should wait for M3b's durable build job and artifact
contracts. M4 and M5 can proceed in parallel after the worker/runtime event
interfaces are stable. M6 must not start until M5 has a reliable release/health
contract. M7a needs the route/release contract from M6 for multi-node traffic;
M7b reuses the worker and runtime interfaces from M3b/M5.

## Plan Inventory

| Plan | File | Deliverable | Status | Depends on |
| --- | --- | --- | --- | --- |
| M1 | `2026-09-08-deploy-platform-m1.md` | Single-node static deployment core | Complete | None |
| M2 | `2026-09-08-deploy-platform-m2-dashboard-auth.md` | Authenticated dashboard and control-plane persistence | Complete | M1 |
| M3a | `2026-09-08-deploy-platform-m3a-git-webhooks.md` | Git provider connections and verified webhook ingestion | Complete | M2 |
| M3b | `2026-09-08-deploy-platform-m3b-queue-workers.md` | Durable build queue and isolated worker | Complete | M2, M3a source contracts |
| M3c | `2026-09-08-deploy-platform-m3c-previews.md` | Pull-request preview lifecycle and unique URLs | Complete | M3a, M3b |
| M4 | `2026-09-08-deploy-platform-m4-cache-observability.md` | Build cache, durable logs, and metrics API | Complete | M3b |
| M5 | `2026-09-08-deploy-platform-m5-zero-downtime-rollback.md` | Health-gated releases, traffic swaps, rollback | Complete | M3b, M4 runtime events |
| M6 | `2026-09-08-deploy-platform-m6-multinode.md` | Node agent, scheduler, capacity-aware placement | In Progress | M5 |
| M7a | `2026-09-08-deploy-platform-m7a-router-domains-tls.md` | Edge routing, custom domains, ACME TLS | Planned | M6 |
| M7b | `2026-09-08-deploy-platform-m7b-ssr-autoscaling.md` | SSR process contract and autoscaling | Planned | M3b, M5, M6 |

## Cross-Cutting Definition of Done

Every plan must:

- start with failing tests for its new behavior;
- keep unit tests unprivileged and isolate privileged/runtime tests;
- add migrations or compatibility handling before changing persisted schemas;
- emit structured errors with a request or operation ID;
- define retry, timeout, cancellation, and cleanup behavior;
- update `FRONTEND.md` when a user-visible state or route changes;
- add API contract examples and an end-to-end happy path;
- run `cargo fmt --check`, `cargo test`, `cargo clippy --all-targets -- -D warnings`,
  and the relevant frontend test/build commands before completion;
- commit each independently meaningful file or change according to the repository
  contribution convention.

## Deferred Boundaries

Billing, team invitations, usage limits, arbitrary plugin runtimes, Kubernetes
integration, global multi-region routing, and a public marketplace are outside
these plans. They may consume the contracts here later but must not expand a
milestone without a new design review.
