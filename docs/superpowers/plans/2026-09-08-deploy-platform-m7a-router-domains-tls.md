# Deploy Platform M7a Router, Domains, and TLS Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Route platform and custom hostnames to healthy releases across nodes, expose request metrics, and provision/renew TLS certificates through an operator-visible domain workflow.

**Architecture:** An edge-router process maintains an in-memory immutable routing snapshot sourced from the control plane. Route updates are versioned and swapped atomically. The router terminates TLS, selects a release endpoint, proxies HTTP/WebSocket traffic with bounded timeouts, and emits request/latency/error metrics. ACME state is stored outside the API process and certificate updates trigger a safe router reload.

**Tech Stack:** Rust/Tokio, Hyper or Pingora-style proxy primitives, rustls, ACME client, PostgreSQL domain state, M6 node/release endpoints, and the M4 metrics contract.

**Spec:** `docs/superpowers/plans/2026-09-08-deploy-platform-roadmap.md`, `FRONTEND.md`, M5 traffic contracts, and M6 node placement contracts.

## Global Constraints

- Never route a hostname to a release that has not passed the M5 health gate.
- Hostnames are validated as DNS names and matched case-insensitively after normalization.
- Routing snapshots are immutable, versioned, and atomically replaced.
- TLS private keys remain on the router/certificate store and never enter dashboard responses.
- ACME challenges are scoped to the requested domain and have explicit expiry/error states.
- Proxy connections have request, header, body, idle, and upstream timeouts.
- Unknown hosts return a safe 404/421 response without revealing internal topology.

## File Structure

| Path | Responsibility |
| --- | --- |
| `router/` | Separate edge-router crate/binary. |
| `router/src/routes.rs` | Immutable route snapshot and matching. |
| `router/src/proxy.rs` | HTTP/WebSocket proxy and timeout policy. |
| `router/src/tls.rs` | Certificate store, SNI resolver, ACME lifecycle. |
| `router/src/metrics.rs` | Request/error/latency event emission. |
| `src/domains.rs` | Domain state, verification, and route ownership. |
| `migrations/0008_domains.sql` | Domains, DNS challenges, certificates, route versions. |
| `tests/routes_test.rs` | Matching/version/health behavior. |
| `tests/domains_test.rs` | Validation/verification/certificate transitions. |
| `tests/proxy_test.rs` | Timeout/header/error behavior. |
| `dashboard/app/projects/[projectId]/domains/page.tsx` | Domain setup UI. |
| `dashboard/tests/domains.spec.ts` | Domain/TLS E2E workflows. |

### Task 1: Define domain ownership and route snapshots

- [ ] **Step 1: Write failing tests** for hostname normalization, project ownership, duplicate domain rejection, preview wildcard collision, and route version ordering.
- [ ] **Step 2: Add domain/certificate/route schema** with verification status, TLS state, active release ID, and route snapshot version.
- [ ] **Step 3: Implement `DomainService`** for add/remove/verify and `RouteSnapshotBuilder` that includes only active healthy releases.
- [ ] **Step 4: Add signed/versioned route publication** for router consumers.
- [ ] **Step 5: Run tests and commit `feat: define domain and route state`**.

### Task 2: Implement edge route matching and proxying

- [ ] **Step 1: Write failing proxy tests** for exact host, wildcard preview, unknown host, inactive release, upstream timeout, and response-header policy.
- [ ] **Step 2: Implement immutable route trie/map** with atomic snapshot swaps and route version acknowledgement.
- [ ] **Step 3: Implement HTTP proxy** with bounded timeouts, request ID forwarding, safe header filtering, body size limits, and upstream error mapping.
- [ ] **Step 4: Add WebSocket upgrade support** only after authentication/route match and idle timeout enforcement.
- [ ] **Step 5: Emit request count/error/latency events** with project/deployment/domain IDs and no secret/header values.
- [ ] **Step 6: Run proxy tests with mock upstreams and commit `feat: add edge router proxy`**.

### Task 3: Add DNS verification and ACME TLS

- [ ] **Step 1: Write failing tests** for DNS challenge generation, verification retry/expiry, certificate pending/active/error, renewal window, and invalid domain.
- [ ] **Step 2: Implement DNS verification records** and a provider-neutral resolver interface.
- [ ] **Step 3: Implement ACME account/certificate lifecycle** with filesystem/object storage for encrypted private keys and certificates.
- [ ] **Step 4: Implement SNI certificate selection** and atomic certificate reload without restarting the router.
- [ ] **Step 5: Run ACME tests with fake DNS/ACME servers and commit `feat: add automated domain tls lifecycle`**.

### Task 4: Add domain API and dashboard

- [ ] **Step 1: Write failing API tests** for add/list/delete, verify, TLS status, authorization, and route activation.
- [ ] **Step 2: Add `GET/POST/DELETE /v1/projects/:id/domains` and `POST /v1/domains/:id/verify`.**
- [ ] **Step 3: Implement the domain wizard** from `FRONTEND.md`: hostname, DNS record, verification polling, certificate provisioning, active/renewal/error states.
- [ ] **Step 4: Add copyable DNS records, explicit external DNS instructions, and no private-key input.**
- [ ] **Step 5: Add E2E tests for a mocked domain from pending DNS through active TLS.**
- [ ] **Step 6: Run Rust/dashboard verification and commit `feat: add domains and tls dashboard`**.

## M7a Acceptance Criteria

- Platform and custom hostnames route only to healthy releases.
- Route snapshots swap atomically and survive router restart.
- HTTP/WebSocket proxying has bounded resource/time limits and request metrics.
- Users can verify a domain, provision TLS, see expiry/renewal state, and recover from errors.
- No internal node address or TLS private key is exposed through the API/dashboard.
