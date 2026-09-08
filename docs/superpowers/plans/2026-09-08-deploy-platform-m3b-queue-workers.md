# Deploy Platform M3b Queue and Build Workers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Move builds out of the API process into a durable, retryable queue and an isolated worker that clones a commit, detects the framework, builds in a container, packages an artifact, and emits live logs and terminal results.

**Architecture:** The API writes a `BuildJob` to PostgreSQL and publishes the job ID to a Redis Stream. Workers claim jobs with consumer groups, renew a lease, update attempts, and publish structured events. The worker never trusts shell strings; it uses argv commands and minidock/container isolation for build execution.

**Tech Stack:** Rust/Tokio, PostgreSQL, Redis Streams, `git` subprocess with argv, minidock runtime, artifact storage, structured tracing, and the existing static builder.

**Spec:** `docs/superpowers/plans/2026-09-08-deploy-platform-roadmap.md`, `FRONTEND.md`, and M3a source events.

## Global Constraints

- API requests enqueue work and return a deployment ID; they do not wait for builds.
- Queue jobs are at-least-once; handlers must be idempotent by `job_id` and deployment ID.
- Production jobs have higher priority than previews, but priority must not starve previews indefinitely.
- Build commands run inside an isolated runtime with CPU, memory, timeout, workspace, and network policy.
- Worker logs are streamed as structured lines and persisted with retention metadata.
- Every job has a lease, attempt number, backoff, maximum retry count, and terminal error.
- A cancelled deployment stops its build container and acknowledges/removes the job.

## File Structure

| Path | Responsibility |
| --- | --- |
| `src/queue.rs` | Build job model, enqueue, claim, lease, ack, retry. |
| `src/worker.rs` | Worker loop, cancellation, job execution, event publication. |
| `src/source_checkout.rs` | Safe clone/fetch/checkout at an exact commit. |
| `src/framework.rs` | Framework detection and build plan. |
| `src/build_executor.rs` | Isolated command/container execution and log capture. |
| `src/artifacts.rs` | Artifact metadata and object/local storage. |
| `src/bin/build-worker.rs` | Worker process entry point. |
| `migrations/0003_build_jobs.sql` | Jobs, attempts, logs, artifacts. |
| `tests/queue_test.rs` | Lease/retry/priority behavior. |
| `tests/worker_test.rs` | Fake executor worker lifecycle. |
| `tests/checkout_test.rs` | Exact commit and path safety. |
| `tests/framework_test.rs` | Detection/build-plan behavior. |

### Task 1: Create durable jobs and Redis Stream queue

- [x] **Step 1: Write failing tests** for priority order, duplicate enqueue idempotency, lease expiry, retry backoff, cancellation, and dead-letter transition.
- [x] **Step 2: Run `cargo test --test queue_test`** and confirm queue code is missing.
- [x] **Step 3: Add `build_jobs`, `build_attempts`, and `build_events` tables** with unique deployment IDs, attempt state, lease expiration, and terminal error fields.
- [x] **Step 4: Implement `BuildQueue::enqueue`, `claim`, `renew`, `ack`, `fail`, `cancel`, and `requeue_expired`** using Redis Stream `XADD`, consumer groups, and PostgreSQL state as source of truth.
- [x] **Step 5: Implement weighted priority selection** by publishing to priority streams and giving production a bounded lead over preview jobs.
- [x] **Step 6: Run queue tests with a Redis fixture or fake transport** and commit `feat: add durable build job queue`.

### Task 2: Implement safe source checkout and framework plans

**Interfaces:**

```rust
pub struct Checkout { pub directory: PathBuf, pub commit_sha: String }
pub fn checkout_commit(source: &RepositoryRef, commit: &str, workspace: &Path) -> Result<Checkout>;
pub struct BuildPlan { pub framework: Framework, pub install: Vec<String>, pub build: Vec<String>, pub output: PathBuf, pub server: Option<Vec<String>> }
pub fn detect_build_plan(root: &Path) -> Result<BuildPlan>;
```

- [x] **Step 1: Write failing checkout tests** for exact SHA, branch/SHA mismatch, traversal in workspace, and nonzero git command.
- [x] **Step 2: Implement argv-only `git init/fetch/checkout`** with a no-shell command runner and a workspace under the job UUID.
- [x] **Step 3: Add framework tests** for static, Node/Next.js, Python, unsupported, and invalid config cases.
- [x] **Step 4: Implement build-plan detection** from package/lock files, `requirements.txt`, `index.html`, and `Dockerfile`, rejecting ambiguous plans with an explicit message.
- [x] **Step 5: Run checkout/framework tests** and commit `feat: add exact source checkout and build plans`.

### Task 3: Execute builds in isolated containers

**Interfaces:**

```rust
pub trait BuildExecutor {
    fn execute(&self, plan: &BuildPlan, workspace: &Path, log: &mut dyn LogSink) -> Result<BuildResult>;
}
pub struct BuildResult { pub artifact_path: PathBuf, pub cache_key: String, pub duration: Duration }
```

- [ ] **Step 1: Write failing executor tests** for command ordering, timeout, nonzero exit, log redaction, and artifact containment.
- [ ] **Step 2: Implement `MinidockBuildExecutor`** using a build rootfs, explicit argv, resource limits, timeout, and a separate workspace mount/copy.
- [ ] **Step 3: Stream stdout/stderr as `BuildLogLine` events** with sequence, timestamp, stream, and redacted text.
- [ ] **Step 4: Reject output paths outside the job artifact directory** and package only validated output files.
- [ ] **Step 5: Run executor tests with a fake process/runtime and ignored privileged smoke coverage**; commit `feat: isolate build execution`.

### Task 4: Implement the worker loop and API integration

- [ ] **Step 1: Write failing worker tests** for success, retryable failure, terminal failure, lease renewal, cancellation, and shutdown.
- [ ] **Step 2: Implement `BuildWorker::run_once`**: claim, renew, checkout, detect, execute, persist logs/artifact, publish events, and ack.
- [ ] **Step 3: Classify errors** into retryable infrastructure failures, user build failures, cancellation, and invalid configuration.
- [ ] **Step 4: Add `build-worker` binary** with Redis/Postgres/config validation and graceful SIGTERM shutdown.
- [ ] **Step 5: Change `POST /deployments` and webhook consumers** to create a deployment, enqueue a job, return 202, and let the worker update status.
- [ ] **Step 6: Run Rust tests and a local API/worker fixture**; commit `feat: run deployments through build workers`.

### Task 5: Add dashboard queue/build states

- [ ] **Step 1: Write failing dashboard tests** for queued, retrying, building, cancelled, and failed deployment states.
- [ ] **Step 2: Add live build log stream consumption** with reconnect and last-sequence handling.
- [ ] **Step 3: Show build attempt number, queue wait, worker, cache status placeholder, and actionable errors.**
- [ ] **Step 4: Add cancel/retry controls** with mutation confirmation and disabled states.
- [ ] **Step 5: Run dashboard tests/build and update `FRONTEND.md` if labels/routes changed.**

## M3b Acceptance Criteria

- API deployment requests return quickly with a durable deployment/job ID.
- A worker can build an exact commit outside the API process and publish logs.
- Jobs survive worker restart, retry bounded infrastructure failures, and do not duplicate terminal deployments.
- User build errors are visible with stage and log context.
- Build artifacts are scoped to the deployment and cannot escape the workspace.
