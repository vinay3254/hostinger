# Deploy Platform M3a Git Providers and Webhooks Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Connect projects to GitHub/GitLab repositories, receive verified push and pull-request events, and translate them into durable source events without starting duplicate deployments.

**Architecture:** Add a provider abstraction for OAuth/repository metadata and a webhook ingestion boundary that verifies signatures before parsing provider-specific payloads. Normalize all providers into one `SourceEvent`; the deployment service consumes normalized events asynchronously through the queue plan and remains independent of GitHub/GitLab SDK details.

**Tech Stack:** Rust/Axum, PostgreSQL, provider OAuth APIs, HMAC-SHA256 signature verification, encrypted provider tokens, `reqwest`, serde, and Redis-compatible event handoff.

**Spec:** `docs/superpowers/plans/2026-09-08-deploy-platform-roadmap.md`, `FRONTEND.md`, and the M2 dashboard/auth plan.

## Global Constraints

- Verify webhook signatures before deserializing or acting on payloads.
- Store provider access/refresh tokens encrypted at rest and never return them to the dashboard.
- Make webhook processing idempotent by provider delivery ID.
- Respond quickly to valid webhooks after durable ingestion; build execution happens in M3b.
- Reject unknown repositories, branches, event types, and malformed payloads without enqueueing work.
- Do not clone or execute source code in the API server.

## File Structure

| Path | Responsibility |
| --- | --- |
| `src/providers/mod.rs` | Provider trait and normalized source types. |
| `src/providers/github.rs` | GitHub OAuth, repository, commit, and webhook adapter. |
| `src/providers/gitlab.rs` | GitLab adapter with the same normalized interface. |
| `src/webhooks.rs` | Signature verification, delivery dedupe, and ingestion routes. |
| `src/source_events.rs` | Durable normalized event repository. |
| `migrations/0002_source_integrations.sql` | Provider installations, repositories, deliveries, source events. |
| `tests/providers_test.rs` | Provider fixture parsing and request contracts. |
| `tests/webhooks_test.rs` | Signature, dedupe, and event normalization tests. |
| `dashboard/app/projects/[projectId]/settings/source/page.tsx` | Source connection UI. |
| `dashboard/app/projects/[projectId]/previews/page.tsx` | PR event visibility. |

### Task 1: Define provider and normalized source contracts

**Files:** `src/providers/mod.rs`, `src/source_events.rs`, `tests/providers_test.rs`, `src/model.rs`

- [ ] **Step 1: Write failing tests** for normalized push, pull-request opened/updated/closed, branch, commit, repository, and delivery ID values.
- [ ] **Step 2: Run `cargo test --test providers_test`** and verify the provider module is missing.
- [ ] **Step 3: Add these types:**

```rust
pub struct RepositoryRef { pub provider: Provider, pub external_id: String, pub clone_url: Url, pub default_branch: String }
pub struct SourceEvent { pub delivery_id: String, pub repository: RepositoryRef, pub kind: SourceEventKind, pub commit_sha: String, pub branch: Option<String>, pub pull_request: Option<PullRequestRef> }
pub enum SourceEventKind { Push, PullRequestOpened, PullRequestUpdated, PullRequestClosed }
pub struct PullRequestRef { pub number: u64, pub head_sha: String, pub base_branch: String, pub action: PullRequestAction }
```

- [ ] **Step 4: Implement serde fixtures** for GitHub and GitLab payloads into the normalized types; reject missing repository or commit identity.
- [ ] **Step 5: Run focused tests and commit `feat: define normalized git source events`.**

### Task 2: Add provider installation persistence and OAuth callbacks

**Files:** `migrations/0002_source_integrations.sql`, `src/providers/github.rs`, `src/providers/gitlab.rs`, `src/api.rs`, `tests/provider_oauth_test.rs`

- [ ] **Step 1: Write failing tests** for OAuth state/CSRF mismatch, successful callback, encrypted token persistence, and repository listing.
- [ ] **Step 2: Add tables** for `provider_connections`, `provider_repositories`, and encrypted token material keyed to the authenticated user.
- [ ] **Step 3: Implement `ProviderClient`** with `begin_authorization`, `complete_authorization`, `list_repositories`, `get_commit`, and `create_webhook` methods.
- [ ] **Step 4: Add OAuth state records** with one-time use and five-minute expiry; verify state before exchanging a code.
- [ ] **Step 5: Add routes** `GET /v1/providers/:provider/connect`, `GET /v1/providers/:provider/callback`, and `GET /v1/providers/:provider/repositories`.
- [ ] **Step 6: Run OAuth tests with HTTP fixtures** and commit `feat: add git provider oauth connections`.

### Task 3: Verify and normalize webhook deliveries

**Files:** `src/webhooks.rs`, `src/source_events.rs`, `tests/webhooks_test.rs`, `migrations/0002_source_integrations.sql`

- [ ] **Step 1: Write failing tests** for valid/invalid HMAC signatures, duplicate delivery IDs, unsupported actions, repository mismatch, and durable event creation.
- [ ] **Step 2: Run `cargo test --test webhooks_test`** and confirm no webhook handler exists.
- [ ] **Step 3: Implement constant-time HMAC verification** using the installation secret and raw request body; reject missing signatures with 401.
- [ ] **Step 4: Store `provider_deliveries` before processing** with a unique `(provider, delivery_id)` constraint and return 202 for duplicates.
- [ ] **Step 5: Normalize accepted events** into `source_events` with project ID, commit SHA, branch/PR data, and a deterministic idempotency key.
- [ ] **Step 6: Add `POST /v1/webhooks/:provider/:project_id`** that never executes a build inline.
- [ ] **Step 7: Run `cargo test --test webhooks_test && cargo test`** and commit `feat: ingest verified git webhooks`.

### Task 4: Add source connection UI

**Files:** `dashboard/app/projects/[projectId]/settings/source/page.tsx`, `dashboard/components/SourceConnectionCard.tsx`, `dashboard/lib/api.ts`, `dashboard/tests/source-settings.spec.ts`

- [ ] **Step 1: Write failing component/E2E tests** for connect, repository selection, branch selection, connection error, and disconnect confirmation.
- [ ] **Step 2: Implement provider selection and OAuth redirect** without exposing provider tokens to the browser.
- [ ] **Step 3: Implement repository/branch selection** with commit preview and loading/empty/error states.
- [ ] **Step 4: Display webhook status** and last delivery time; expose a copyable webhook URL only where an operator needs it.
- [ ] **Step 5: Run dashboard tests/build** and commit `feat: add git source configuration ui`.

## M3a Acceptance Criteria

- GitHub and GitLab repositories can be connected through authenticated OAuth.
- Valid push and pull-request webhooks create exactly one normalized source event.
- Invalid signatures, duplicate deliveries, unknown projects, and unsupported events are rejected safely.
- No source code is cloned or executed by the API server.
- Dashboard users can connect a repository, choose a branch, and see webhook health.
