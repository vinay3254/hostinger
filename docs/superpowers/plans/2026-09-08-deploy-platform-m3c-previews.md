# Deploy Platform M3c Preview Deployments Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create an isolated preview deployment for every eligible pull request, expose a stable unique hostname, update it on new commits, and tear it down automatically when the pull request closes or merges.

**Architecture:** A normalized pull-request source event creates or updates one `Preview` record keyed by project/provider/PR number. The preview reuses the M3b queue/build/artifact pipeline but targets a preview environment and route. Teardown is idempotent and retains history after the runtime is removed.

**Tech Stack:** Rust/Axum/PostgreSQL, M3a source events, M3b Redis build queue, M5 release/health interfaces, and the dashboard preview routes in `FRONTEND.md`.

**Spec:** `docs/superpowers/plans/2026-09-08-deploy-platform-roadmap.md`, `FRONTEND.md`, M3a, and M3b plans.

## Global Constraints

- Preview hostname is generated from validated project slug and PR number; never interpolate raw branch names into DNS labels.
- One active preview per project/provider/PR number; repeated events update the same logical preview.
- Preview environment variables are isolated from production and never displayed as values.
- Closed/merged pull requests stop runtime traffic and clean up containers/artifacts according to retention policy.
- A preview build failure does not change production deployment state.
- Promote-to-production creates a new production deployment from the exact preview commit; it does not relabel a preview.

## File Structure

| Path | Responsibility |
| --- | --- |
| `migrations/0004_previews.sql` | Preview records, PR identity, hostnames, teardown state. |
| `src/previews.rs` | Preview lifecycle and idempotency. |
| `src/preview_events.rs` | Pull-request event consumer. |
| `src/hostname.rs` | Safe preview slug/hostname generation. |
| `src/api.rs` | Preview list/detail/promote/stop routes. |
| `tests/previews_test.rs` | Lifecycle, duplicate events, and cleanup. |
| `tests/hostname_test.rs` | Hostname normalization and limits. |
| `dashboard/app/projects/[projectId]/previews/page.tsx` | Preview list. |
| `dashboard/app/projects/[projectId]/previews/[previewId]/page.tsx` | Preview detail. |
| `dashboard/tests/previews.spec.ts` | End-to-end preview workflows. |

### Task 1: Define preview records and safe hostnames

- [x] **Step 1: Write failing tests** for PR identity uniqueness, branch names with unsafe characters, maximum label length, unicode normalization, and deterministic hostname output.
- [x] **Step 2: Implement `preview_hostname(project_slug, pr_number) -> Result<String>`** with lowercase DNS labels, bounded length, and a hash suffix when truncation would collide.
- [x] **Step 3: Add preview schema** with project/provider/PR unique key, deployment ID, hostname, status, closed timestamp, and cleanup attempt.
- [x] **Step 4: Run tests and commit `feat: add preview identity and hostnames`**.

### Task 2: Consume pull-request events idempotently

- [x] **Step 1: Write failing tests** for opened, synchronized, reopened, closed, and merged events plus duplicate delivery.
- [x] **Step 2: Implement `PreviewService::apply_event`**: create/update preview, create a preview deployment for new SHA, enqueue rebuild for changed SHA, and request teardown for closed/merged state.
- [x] **Step 3: Add event ordering checks** so an older commit cannot replace a newer preview.
- [x] **Step 4: Run service tests and commit `feat: create previews from pull requests`**.

### Task 3: Integrate preview build/runtime/release lifecycle

- [x] **Step 1: Write failing tests** for preview build success, failed build preserving prior preview, health failure cleanup, and promote-to-production source equality.
- [x] **Step 2: Pass preview target and isolated env scope into `BuildJob`.**
- [x] **Step 3: Allocate preview port/route through the runtime/release interface** and persist URL only after health succeeds.
- [x] **Step 4: Implement idempotent teardown** that stops release, removes route, and marks preview stopped while retaining logs/history.
- [x] **Step 5: Implement promotion** as a new production deployment referencing the exact preview commit and build artifact where compatible.
- [x] **Step 6: Run integration tests with fake queue/runtime and commit `feat: manage preview runtime lifecycle`**.

### Task 4: Add preview API and dashboard

- [x] **Step 1: Write failing API tests** for list/detail, closed preview, promote, stop, and permission errors.
- [x] **Step 2: Add routes** `GET /v1/projects/:id/previews`, `GET /v1/previews/:id`, `POST /v1/previews/:id/promote`, `POST /v1/previews/:id/stop`.
- [x] **Step 3: Implement dashboard preview list/detail** with PR title/number, commit, author, URL, status, timestamps, and actions from `FRONTEND.md`.
- [x] **Step 4: Add live status/log links and explicit closed/expired state.**
- [x] **Step 5: Run dashboard/API E2E tests and commit `feat: add preview deployment dashboard`**.

## M3c Acceptance Criteria

- Every eligible PR has one deterministic preview identity and hostname.
- New PR commits update the preview without affecting production.
- Closed/merged PRs stop preview traffic idempotently.
- Preview failures retain history and logs.
- Promote creates a distinct production deployment from the exact preview commit.
