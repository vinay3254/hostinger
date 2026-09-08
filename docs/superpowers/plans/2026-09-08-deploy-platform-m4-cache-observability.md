# Deploy Platform M4 Build Cache and Observability Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make repeat builds fast and make build/runtime behavior queryable through durable logs, metrics, and dashboard visualizations.

**Architecture:** Build workers compute a content-addressed cache key from lockfiles, framework, build configuration, and relevant source metadata. Cache entries are immutable and scoped by runtime/toolchain. Logs are append-only records in object/local storage with indexed metadata; metrics are aggregated from runtime/router events rather than scraped from frontend state.

**Tech Stack:** Rust/Tokio, PostgreSQL metadata, S3-compatible object storage or a filesystem adapter, Redis for event fanout, SHA-256 hashing, OpenTelemetry-compatible metric names, and Next.js charts/tables.

**Spec:** `docs/superpowers/plans/2026-09-08-deploy-platform-roadmap.md`, `FRONTEND.md`, and M3b queue/worker contracts.

## Global Constraints

- Cache keys must include every input that can change build output: lockfile, framework, build config, runtime/toolchain, and declared environment names/versions.
- Never cache secret values or emit them into cache metadata.
- Cache hits must be verified by artifact checksum and schema/toolchain version.
- Log retention and metric retention are explicit configuration, not unbounded disk growth.
- Log queries are scoped by project/deployment and paginated or streamed.
- Metrics include units, aggregation window, source timestamp, and partial-data state.
- Observability failures must not fail an otherwise healthy deployment; they must surface as degraded telemetry.

## File Structure

| Path | Responsibility |
| --- | --- |
| `src/cache.rs` | Cache key, metadata, lookup, publish, invalidation. |
| `src/artifact_store.rs` | Content-addressed artifact/log object storage. |
| `src/logs.rs` | Structured log append/query/stream contracts. |
| `src/metrics.rs` | Metric events, rollups, query service. |
| `migrations/0005_cache_observability.sql` | Cache, log segments, metric samples/rollups. |
| `tests/cache_test.rs` | Key stability, invalidation, checksum, hit/miss. |
| `tests/logs_test.rs` | Redaction, ordering, pagination, retention. |
| `tests/metrics_test.rs` | Aggregation, time windows, partial data. |
| `dashboard/app/projects/[projectId]/metrics/page.tsx` | Metrics page. |
| `dashboard/components/LogViewer.tsx` | Build/runtime stream viewer. |
| `dashboard/tests/metrics.spec.ts` | Dashboard metric/log workflows. |

### Task 1: Define cache keys and artifact storage

- [ ] **Step 1: Write failing tests** for stable key ordering, source/config changes producing different keys, secret exclusion, artifact checksum mismatch, and cache schema versioning.
- [ ] **Step 2: Implement `CacheKey::from_build_inputs`** using canonical JSON plus SHA-256; include framework/toolchain/config/lockfile digest and declared env key/version metadata.
- [ ] **Step 3: Add cache/artifact tables** with key, checksum, size, storage URI, toolchain, created/last-used, and invalidation status.
- [ ] **Step 4: Implement `ArtifactStore`** with atomic upload, checksum verification, scoped paths, and a filesystem test adapter.
- [ ] **Step 5: Run tests and commit `feat: add content addressed build cache`**.

### Task 2: Integrate cache lookup and publish into workers

- [ ] **Step 1: Write failing worker tests** for cache hit skipping install/build, cache miss running the full plan, failed publish preserving build success, and invalidation.
- [ ] **Step 2: Add cache lookup before build execution** and emit `build.cache.checked` with hit/miss reason but no secret values.
- [ ] **Step 3: Publish successful immutable artifacts** only after checksum/manifest validation; update last-used asynchronously.
- [ ] **Step 4: Add force-rebuild and project cache-clear operations** with audit events.
- [ ] **Step 5: Run worker tests and commit `feat: integrate build cache into workers`**.

### Task 3: Persist and stream build/runtime logs

- [ ] **Step 1: Write failing tests** for sequence ordering, reconnect from sequence, redaction, pagination, retention deletion, and terminal stream closure.
- [ ] **Step 2: Implement `LogSink`, `LogQuery`, and `LogStream`** with deployment/project scope and monotonic sequence numbers.
- [ ] **Step 3: Persist log segments in object storage** and searchable metadata in PostgreSQL; rotate by size/time.
- [ ] **Step 4: Add `GET /v1/deployments/:id/logs`, download, and stream endpoints** with auth and bounded query limits.
- [ ] **Step 5: Add redaction middleware** for configured secret fingerprints and common credential patterns.
- [ ] **Step 6: Run log tests and commit `feat: add durable deployment logs`**.

### Task 4: Add metrics events and query API

- [ ] **Step 1: Write failing tests** for counter/rate/latency aggregation, out-of-order samples, time-zone boundaries, and no-data windows.
- [ ] **Step 2: Define metric names**: `build_duration_seconds`, `build_cache_hit_total`, `deployment_health_check_total`, `request_total`, `request_error_total`, `request_latency_ms`, `container_cpu_seconds`, and `container_memory_bytes`.
- [ ] **Step 3: Implement metric ingestion and fixed-window rollups** with source timestamps and partial-data flags.
- [ ] **Step 4: Add `GET /v1/projects/:id/metrics`** with metric, range, resolution, and environment filters.
- [ ] **Step 5: Run metrics tests and commit `feat: add deployment metrics api`**.

### Task 5: Add dashboard cache/log/metrics surfaces

- [ ] **Step 1: Write failing dashboard tests** for cache hit/miss display, log reconnect, chart empty/partial state, table alternative, and time-range changes.
- [ ] **Step 2: Update deployment detail** with cache key summary, duration comparison, and build artifact metadata.
- [ ] **Step 3: Implement virtualized `LogViewer`** with search, pause/follow, download, reconnect, and secret-safe rendering.
- [ ] **Step 4: Implement metrics cards/charts plus accessible data tables** from `FRONTEND.md`.
- [ ] **Step 5: Run dashboard tests/build and commit `feat: add cache and observability dashboard`**.

## M4 Acceptance Criteria

- Repeat builds visibly report cache hits and skip unchanged work.
- Cache entries are immutable, checksummed, scoped, and safe to invalidate.
- Users can stream, search, download, and resume deployment logs.
- Metrics API and dashboard show requests, errors, latency, CPU, memory, build time, and cache rate with units and timestamps.
- Telemetry failure degrades observability only; it does not silently change deployment status.
