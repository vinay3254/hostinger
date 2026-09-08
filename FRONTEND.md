# Deploy Platform Frontend

Product and implementation specification for the complete Deploy Platform web
application: a Vercel/Render-style control plane for building, deploying,
observing, and operating applications on top of minidock.

This document is the frontend source of truth. It describes the full product
surface even when a capability is still planned. Status markers distinguish the
current M1 backend from future frontend work.

## Status Legend

- **M1** — supported by the current API/CLI implementation or required for the
  first dashboard slice.
- **M2** — dashboard and authenticated project management.
- **M3** — preview deployments and pull-request workflows.
- **M4** — build cache and performance visibility.
- **M5** — zero-downtime deploys and rollback.
- **M6** — multi-node scheduling and operations.
- **M7** — custom domains, automatic TLS, autoscaling, and SSR.
- **Future** — intentionally outside the current milestone sequence.

## Product Goals

The frontend should make deployment feel like a clear, observable workflow:

```text
connect source → configure project → deploy → inspect build → visit preview
       → watch health → promote → observe traffic → rollback if needed
```

The experience must answer these questions at a glance:

1. What is deployed right now?
2. What changed in the latest deployment?
3. Is the build or runtime healthy?
4. Where can I open the application?
5. What failed, and what can I do next?
6. Which environment variables, domains, nodes, and previews are connected?

The interface is operational software, not a marketing site. It should favor
clear status, readable logs, useful defaults, reversible actions, and calm
visual hierarchy over decorative dashboards.

## Frontend Technology

The planned implementation uses:

- Next.js with the App Router and TypeScript.
- Server-rendered project and deployment pages where data is stable.
- Client components for live logs, deployment actions, forms, and charts.
- TanStack Query or an equivalent cache for API data, invalidation, and polling.
- Server-sent events or WebSockets for build logs and deployment state changes.
- CSS variables plus a small component system; Tailwind is acceptable if it does
  not hide semantic states or accessibility behavior.
- A chart library only for metrics that cannot be communicated clearly as text.
- Playwright for end-to-end flows and React Testing Library for component states.

The frontend talks to the API server only. It never calls minidock, build
workers, scheduler nodes, databases, or storage directly.

### M2 Implementation (`dashboard/`)

The M2 dashboard application is located in `dashboard/` and features:
- **Framework:** Next.js 14 App Router with TypeScript.
- **Data Fetching:** TanStack Query (`@tanstack/react-query`) with automatic cache invalidation and live polling for active deployments and logs.
- **Client & Redaction:** Strongly-typed `ApiClient` (`dashboard/lib/api.ts`) supporting credentials inclusion, error parsing, and strict redaction of sensitive tokens and keys from telemetry.
- **Testing:** Unit and component tests with Vitest and React Testing Library (`npm test`), plus end-to-end user workflows using Playwright (`npx playwright test`).
- **Commands:**
  - `cd dashboard && npm install`
  - `npm test` - Run Vitest unit & component test suite
  - `npm run build` - Production Next.js build
  - `npx playwright test` - End-to-end integration test suite
  - `npm run dev` - Start local Next.js development server on port 3000

## Information Architecture

### Public routes

| Route | Purpose | Milestone |
| --- | --- | --- |
| `/` | Product landing/redirect to dashboard | M2 |
| `/login` | Session login | M2 |
| `/auth/callback` | Provider callback and session completion | M2 |
| `/invite/:token` | Accept a project/team invitation | Future |

### Authenticated routes

| Route | Purpose | Milestone |
| --- | --- | --- |
| `/dashboard` | Account-wide deployment overview | M2 |
| `/projects` | Project list and creation entry point | M2 |
| `/projects/new` | Connect source and configure a project | M2/M3 |
| `/projects/:projectId/overview` | Project health and latest deployment | M2 |
| `/projects/:projectId/deployments` | Deployment history and filters | M2 |
| `/projects/:projectId/deployments/:deploymentId` | Build, runtime, logs, and actions | M2 |
| `/projects/:projectId/previews` | Branch and pull-request deployments | M3 |
| `/projects/:projectId/environment` | Environment variables and secrets | M2 |
| `/projects/:projectId/domains` | Platform and custom domains | M7 |
| `/projects/:projectId/metrics` | Runtime and edge metrics | M4/M6 |
| `/projects/:projectId/settings` | Source, build, deployment, and danger settings | M2 |
| `/account/api-tokens` | CLI token management | M2 |
| `/account/activity` | Account audit/activity stream | M2 |
| `/admin/nodes` | Node health, capacity, and scheduling | M6 |

Every authenticated page has a project switcher when a project context exists,
preserves the selected environment in the URL, and supports deep links to a
deployment, log position, preview, or domain setup step.

## Application Shell

### Desktop layout

```text
┌──────────────────────────────────────────────────────────────────────┐
│ logo   project switcher             search   help   notifications avatar │
├───────────────┬──────────────────────────────────────────────────────┤
│ Overview      │                                                      │
│ Deployments   │                     page content                      │
│ Previews      │                                                      │
│ Environment   │                                                      │
│ Domains       │                                                      │
│ Metrics       │                                                      │
│ Settings      │                                                      │
│               │                                                      │
│ system status│                                                      │
└───────────────┴──────────────────────────────────────────────────────┘
```

The sidebar is project-aware. Account and admin controls remain in the avatar
menu or separate account routes so project actions stay easy to find.

### Mobile layout

- Replace the persistent sidebar with a menu button and a bottom-priority action
  bar for Deploy, Logs, and Overview.
- Keep deployment status and the primary URL visible above the fold.
- Make log output horizontally scrollable without shrinking text below 12px.
- Convert dense tables into stacked cards with labeled fields.
- Keep destructive actions behind an explicit confirmation sheet.

### Global shell behavior

- Show the current project name and environment in every project page title.
- Use breadcrumbs on deployment detail, domain setup, and settings pages.
- Preserve query parameters for filters, search, time range, and selected log tab.
- Show a global connection indicator when the API or realtime stream is offline.
- Use toast notifications for completed background actions, but keep durable errors
  in the relevant page rather than relying on a toast alone.

## Dashboard

`/dashboard` is the account-level command center.

### Content

- Projects with current deployment status.
- Deployments in progress across all projects.
- Recent failures requiring attention.
- Recent previews awaiting review.
- Aggregate request, CPU, memory, and build-duration summaries when metrics exist.
- A clear empty state for a new account: **Create your first project**.

### Actions

- Create project.
- Open the latest deployment.
- Open the live URL.
- Retry a failed deployment.
- Filter by project, environment, branch, and status.

### States

- Loading skeleton with stable card dimensions.
- No projects state with a single primary action.
- API unavailable state with retry and last-known timestamp.
- Partial metrics state when deployment data is available but metrics are not.

## Project Creation and Onboarding

### `/projects/new`

Use a short wizard with a persistent summary rather than a large single form.

#### Step 1: Source

- Connect GitHub/GitLab account.
- Select organization, repository, and branch.
- Display the latest commit and repository visibility.
- M1 fallback: enter a local/manual source path only in development mode.
- Explain that webhook access is used to trigger future deployments.

#### Step 2: Framework and build

- Show detected framework and detection confidence.
- Show root directory, install command, build command, and output directory.
- Allow advanced overrides with inline explanations.
- Reject unsupported frameworks with a useful alternative, not a generic error.

#### Step 3: Runtime

- Select production or preview behavior.
- Select base image/runtime template.
- Show server command as structured argv fields, never as a shell script.
- Configure health-check path, port, and timeout.

#### Step 4: Environment

- Add environment variables by target: production, preview, or all.
- Mark secret values as write-only after creation.
- Import from a safe local file only with an explicit confirmation.

#### Step 5: Review

- Show source, detected framework, build settings, runtime, and environment target.
- Explain exactly what the first deployment will do.
- Primary action: **Create project and deploy**.

### Onboarding success

After project creation, route to the deployment detail page. Do not send users to
an empty project overview when a deployment is already running.

## Project Overview

`/projects/:projectId/overview` answers whether the project is live.

### Hero section

- Project name and environment selector.
- Current deployment status badge.
- Live URL with copy and open actions.
- Commit/branch and relative deployment time.
- Primary action: **Deploy**.
- Secondary actions: **View logs**, **Rollback**, **Project settings**.

### Summary cards

- Current deployment and previous deployment.
- Build duration and cache hit/miss.
- Runtime health and last health check.
- Request count, latency, CPU, and memory when available.
- Active previews count.

### Timeline

Show source event, build start, build completion, container start, health check,
traffic activation, and any teardown as a single chronological timeline.

### Failure state

When the latest deployment failed:

- Keep the previous healthy deployment prominent.
- Explain the failed stage: source, build, image, runtime, or health check.
- Show the first useful error line with **Open full logs**.
- Offer **Retry deployment** and **Rollback to previous** where valid.

## Deployments

### Deployment list

`/projects/:projectId/deployments` displays newest first.

Each row/card contains:

- Status and stage.
- Deployment ID with copy action.
- Commit SHA, branch, or pull request.
- Deployment type: production or preview.
- Created time and duration.
- URL when available.
- Build cache result.
- Trigger: Git push, pull request, CLI, dashboard, or webhook.

Filters:

- Status: pending, building, running, failed, stopped, cancelled.
- Environment: production, preview.
- Branch/PR.
- Date range.
- Trigger source.

### Deploy action

The Deploy dialog supports:

- branch/commit selection;
- production versus preview target;
- rebuild without cache;
- optional environment target;
- confirmation of the source commit;
- cancellation before build starts.

The submit button changes to **Building** immediately and navigates to the new
deployment detail page.

## Deployment Detail

`/projects/:projectId/deployments/:deploymentId` is the primary operational page.

### Header

- Status badge with text, color, and icon.
- Deployment type and source reference.
- Created time, duration, and deployment ID.
- Open URL, copy URL, redeploy, stop, rollback, and delete actions according to
  status and permissions.

### Stage progress

```text
Queued → Cloning → Detecting → Building → Packaging → Starting → Healthy → Live
```

Each stage has:

- start/end timestamp;
- duration;
- current progress message;
- retry behavior;
- failure reason;
- link to relevant logs.

### Tabs

1. **Overview** — source, status, URL, runtime, and timeline.
2. **Build logs** — live build output with search and download.
3. **Runtime logs** — container stdout/stderr with follow mode.
4. **Metrics** — CPU, memory, requests, latency, and health checks.
5. **Environment** — variable keys and target, never secret values.
6. **Events** — webhook, scheduler, router, and operator events.

### Live log behavior

- Connect only after the user opens the log tab.
- Start at the end for running deployments; allow jump to beginning.
- Pause follow mode while the user selects or searches text.
- Preserve the last visible position when switching tabs.
- Show reconnect state and last received timestamp.
- Never render log lines as HTML.
- Redact known secret values in the frontend as defense in depth.
- Offer download as plain text with deployment ID and timestamp metadata.

## Preview Deployments

`/projects/:projectId/previews` supports the pull-request review workflow.

### Preview list

- Pull-request number and title.
- Branch and commit SHA.
- Author/avatar.
- Preview URL and status.
- Created/updated time.
- Last comment or deployment event.
- Automatic teardown state after merge/close.

### Preview detail

- Show the PR context next to deployment state.
- Display the exact commit under review.
- Provide **Open preview**, **Redeploy**, **Promote to production**, and
  **Stop preview** where allowed.
- Show environment differences between preview and production without exposing
  secret values.
- Show a prominent closed/expired state rather than a broken URL.

### Preview lifecycle

```text
PR opened → preview building → preview live → PR updated → old preview drained
                                  ↓
                         PR merged/closed → preview stopped
```

Preview URLs use the generated hostname from the API and should never be built by
concatenating unescaped user input in the browser.

## Environment Variables and Secrets

`/projects/:projectId/environment` manages runtime configuration.

### Variable table

- Key.
- Target: production, preview, all.
- Scope/status.
- Last updated by and time.
- Whether the value is secret.
- Actions: edit, duplicate target, delete.

Values are write-only for secrets. The UI must never display a secret returned by
the API, cache it in URL state, log it, or place it in analytics events.

### Add/edit flow

- Validate key syntax before submission.
- Show target selector.
- Offer value visibility only while typing, with a clear warning.
- Confirm replacements and deletions.
- Explain when a new deployment is required for the change to take effect.
- Show a deployment link after saving if the API triggers a redeploy.

### Import/export

- Import keys, not raw secret values, into a review list before saving.
- Never offer a plaintext export of stored secrets.
- Allow a redacted `.env.example` export.

## Domains and TLS

`/projects/:projectId/domains` manages platform and custom hostnames.

### Platform domain

- Show the generated platform URL.
- Copy/open actions.
- Current routing target and deployment.
- Preview hostname pattern.

### Custom domain wizard

1. Enter hostname.
2. Validate hostname syntax and ownership requirement.
3. Show required DNS record with copy buttons.
4. Poll verification status.
5. Request/provision certificate.
6. Confirm active routing and TLS status.

Statuses:

- Waiting for DNS.
- DNS detected.
- Ownership verified.
- Certificate provisioning.
- Active.
- Renewal warning.
- Error with actionable remediation.

Never ask users to paste private keys. The frontend displays certificate status,
expiry, issuer, and renewal state only.

## Metrics and Observability

`/projects/:projectId/metrics` provides operational context without pretending
to be a full APM product.

### Core charts

- Requests per minute.
- Error rate.
- P50/P95/P99 latency.
- CPU usage.
- Memory usage.
- Container restarts.
- Build duration.
- Cache hit rate.

Every chart includes:

- explicit units;
- selected time range;
- timezone;
- last-updated time;
- empty/partial-data explanation;
- accessible tabular alternative.

### Time ranges

- 15 minutes.
- 1 hour.
- 6 hours.
- 24 hours.
- 7 days when retention supports it.

## Nodes and Scheduler Operations

`/admin/nodes` is restricted to operator/admin users and is M6 scope.

### Node list

- Node name and hostname.
- Online/degraded/offline status.
- Last heartbeat.
- CPU/memory capacity and utilization.
- Running container count.
- Active deployments.
- Scheduler placement reason.

### Node detail

- Capacity timeline.
- Containers currently running.
- Recent health/heartbeat events.
- Drain/disable action with confirmation.
- Rescheduling history.

The frontend must never suggest that a deployment is healthy solely because a node
is online; it must show application health separately.

## Rollback and Zero-Downtime Operations

Rollback is a first-class action, not a hidden dropdown item.

### Rollback flow

1. User selects a previous healthy deployment.
2. Confirmation shows source commit, build age, URL, and environment target.
3. User confirms.
4. UI shows a new rollback deployment/event, not an edited history row.
5. New container health is shown before traffic changes.
6. The previous failed/current deployment remains inspectable.

### Safety rules

- Disable rollback when no healthy predecessor exists.
- Require explicit confirmation for production.
- Show who initiated the action in activity history.
- Preserve a link to the deployment that was active before the rollback.
- Surface drain/traffic transition progress during zero-downtime swaps.

## Account, Access, and CLI Tokens

### Account settings

- Session/profile information.
- Connected Git providers.
- Notification preferences.
- Timezone and theme.
- API token management.

### API tokens

- Token name and creation date.
- Last-used time.
- Scope list.
- Revoke action.
- Show the token secret exactly once after creation.
- Explain CLI login and never display the token afterward.

### Team access

Future team support should add:

- members and roles;
- project-level access;
- invitations;
- audit trail;
- protected production actions.

## Activity and Notifications

The activity feed is durable; toast notifications are transient.

Events include:

- project created/updated;
- deployment requested/started/succeeded/failed/stopped;
- rollback initiated/completed;
- environment variable changed;
- domain verified/certificate changed;
- preview created/updated/removed;
- node drained/rescheduled;
- access/token changes.

Notifications should link directly to the relevant project, deployment, domain, or
settings page. Users can filter unread, project, event type, and severity.

## API Contract Expectations

The current M1 API is:

```text
GET  /healthz
POST /v1/projects
GET  /v1/projects/:project_id
GET  /v1/projects/:project_id/deployments
POST /v1/projects/:project_id/deployments
GET  /v1/deployments/:deployment_id
GET  /v1/deployments/:deployment_id/logs
POST /v1/deployments/:deployment_id/stop
```

The frontend should build an API client around typed responses rather than using
untyped JSON throughout the component tree.

### Planned API additions

```text
POST /v1/auth/session
GET  /v1/me
GET  /v1/providers/:provider/install
POST /v1/projects/:project_id/source
POST /v1/projects/:project_id/deployments/:deployment_id/cancel
POST /v1/deployments/:deployment_id/redeploy
POST /v1/deployments/:deployment_id/rollback
GET  /v1/deployments/:deployment_id/events
GET  /v1/deployments/:deployment_id/logs/stream
GET  /v1/projects/:project_id/previews
DELETE /v1/previews/:preview_id
GET  /v1/projects/:project_id/env
PUT  /v1/projects/:project_id/env/:key
DELETE /v1/projects/:project_id/env/:key
GET  /v1/projects/:project_id/domains
POST /v1/projects/:project_id/domains
POST /v1/domains/:domain_id/verify
GET  /v1/projects/:project_id/metrics
GET  /v1/nodes
```

### API client rules

- Attach session credentials through the configured secure mechanism.
- Normalize error responses to `{ error, code?, details? }`.
- Treat `401` as a session refresh/redirect event.
- Treat `403` as a permission state with an explanation, not a silent hide.
- Treat `404` as a stale-link state with navigation back to the project.
- Treat `409` as a concurrency/conflict state with refresh and retry.
- Treat `422` as inline validation or deployment failure.
- Treat `5xx` as an operational error with request ID and retry guidance.

## Realtime Data Model

Deployment pages need live state without full-page polling.

### Event envelope

```json
{
  "event_id": "uuid",
  "type": "deployment.stage.updated",
  "project_id": "uuid",
  "deployment_id": "uuid",
  "occurred_at": "timestamp",
  "payload": {}
}
```

The frontend should handle:

- duplicate event IDs idempotently;
- out-of-order events using timestamps/sequence numbers;
- reconnect with a last-event ID;
- terminal deployment states that close the stream;
- API fallback polling when streaming is unavailable.

## Component Inventory

### Navigation and shell

- `AppShell`
- `ProjectSwitcher`
- `EnvironmentSwitcher`
- `Sidebar`
- `MobileNavigation`
- `Breadcrumbs`
- `GlobalSearch`
- `NotificationsMenu`
- `UserMenu`

### Status and feedback

- `StatusBadge`
- `StageStepper`
- `HealthIndicator`
- `ProgressBar`
- `InlineError`
- `ErrorBoundary`
- `ToastRegion`
- `ConnectionBanner`
- `EmptyState`
- `LoadingSkeleton`

### Deployment operations

- `DeploymentCard`
- `DeploymentTable`
- `DeploymentFilters`
- `DeployDialog`
- `DeploymentHeader`
- `DeploymentTimeline`
- `LogViewer`
- `LogToolbar`
- `LogSearch`
- `RollbackDialog`
- `RedeployDialog`

### Configuration

- `ProjectSetupWizard`
- `FrameworkDetectionCard`
- `BuildSettingsForm`
- `EnvironmentVariableTable`
- `SecretValueInput`
- `DomainWizard`
- `DnsRecordCard`
- `ApiTokenTable`

### Operations

- `MetricCard`
- `MetricChart`
- `MetricTable`
- `NodeTable`
- `NodeCapacityBar`
- `ActivityFeed`

## Shared UI States

Every page and component must define these states before implementation:

1. Initial loading.
2. Loaded with data.
3. Loaded with no data.
4. Validation error.
5. Permission denied.
6. Not found/stale link.
7. API unavailable.
8. Realtime disconnected.
9. Action pending.
10. Action succeeded.
11. Action failed with retry.

Buttons performing mutations must show a pending state, prevent duplicate submits,
and preserve the user's input when the request fails.

## Design System

### Visual principles

- Use neutral surfaces and a single accent color for primary actions.
- Status colors must have text/icon support, not color alone:
  - success: healthy/live;
  - warning: degraded/pending/expiring;
  - danger: failed/stopped/critical;
  - neutral: inactive/unknown.
- Use monospace typography for commit SHAs, IDs, ports, paths, and logs.
- Use compact density for tables but generous spacing around primary actions.
- Use rounded containers sparingly; hierarchy should come from layout and type.
- Prefer explicit labels such as “Running” over ambiguous icons.

### Typography

- Page title: 28–32px desktop, 24px mobile.
- Section title: 18–20px.
- Body: 14–16px.
- Supporting metadata: 12–13px.
- Logs: 12–14px monospace with selectable text.

### Interaction

- Keyboard focus is always visible.
- Destructive operations require confirmation and name the target.
- Copy actions provide a text confirmation and remain usable without a tooltip.
- External links identify that they open a new tab.
- Long IDs have copy buttons and accessible full-value labels.

## Accessibility Requirements

- Meet WCAG 2.2 AA for contrast, focus, keyboard access, and form labeling.
- Use semantic landmarks: header, navigation, main, complementary, footer.
- Every status has a text label and an accessible announcement for transitions.
- Live log updates use a polite live region without announcing every line by default.
- Charts provide a data-table alternative and meaningful text summaries.
- Dialogs trap focus, close on Escape, and return focus to their trigger.
- Tables expose headers and responsive cards preserve label/value relationships.
- Reduced-motion preferences disable nonessential transitions and auto-scrolling.
- Do not rely on hover to expose critical actions.

## Responsive Requirements

### Desktop

- Persistent project navigation.
- Two-column deployment detail: main content plus sticky summary/actions.
- Tables for deployment history, environment keys, domains, and nodes.

### Tablet

- Collapsible navigation.
- Deployment summary remains sticky only when it does not cover logs.
- Tables may hide secondary columns behind an expand action.

### Mobile

- Single-column cards.
- Primary action remains accessible without scrolling to the page footer.
- Horizontal log scrolling is intentional and announced.
- Filters open in a bottom sheet.
- URLs and IDs wrap or use copy controls instead of overflowing the viewport.

## Security and Privacy

- Never render secret values after the creation response.
- Redact secrets from client-side error messages and analytics.
- Do not put tokens, environment values, or raw webhook payloads in URLs.
- Use same-origin protections and secure session storage patterns.
- Confirm production-impacting actions with target, source, and actor context.
- Display audit history for environment, domain, rollback, and token mutations.
- Treat logs as untrusted text; escape markup and control characters.
- Do not expose node credentials, internal addresses, or scheduler internals to
  ordinary project users.

## Performance

- Server-render stable project/deployment summaries.
- Lazy-load metrics charts and heavy log tooling.
- Virtualize large log streams.
- Paginate deployment history and activity feeds.
- Cache project metadata and invalidate after mutations.
- Debounce log search and metric range changes.
- Keep first project page useful before realtime connections finish.
- Show stale data timestamps instead of blocking the whole page on metrics.

## Testing Requirements

### Unit/component tests

- Status badge renders text and accessible state for every status.
- Deployment stage transitions render loading, success, and failure.
- Environment form rejects invalid keys and never displays stored secret values.
- Log viewer escapes HTML, searches, pauses follow mode, and reconnects.
- Domain wizard renders every DNS/TLS state.
- Rollback dialog requires explicit confirmation for production.
- API client maps 401/403/404/409/422/500 to the documented UI states.

### End-to-end tests

1. New user opens dashboard and creates a project.
2. Project deploys successfully and live URL opens.
3. Failed build exposes the failed stage and logs.
4. User follows live logs while a deployment runs.
5. User adds an environment variable and sees the redeploy guidance.
6. User creates a preview and opens its unique URL.
7. User rolls back to a prior healthy deployment.
8. User verifies a domain and sees TLS active.
9. User loses API connectivity and recovers without losing page state.
10. User on mobile completes deploy, logs, and stop flows.

## Rollout Plan

### M1 — API-connected internal dashboard

- Health page.
- Project list and project creation from the current API.
- Deployment list/detail.
- Status, URL, logs, and stop actions.
- Local/manual source workflow for development.

### M2 — Authenticated dashboard MVP

- Login/session handling.
- Project overview.
- Deployment history and filters.
- Live build/runtime logs.
- Environment variables.
- API token management.
- Activity feed.

### M3 — Preview workflow

- Git provider connection.
- Pull-request previews.
- Preview list/detail.
- Auto-teardown and promote action.

### M4 — Build performance

- Cache hit/miss visibility.
- Build duration charts.
- Cache invalidation/rebuild controls.

### M5 — Production safety

- Zero-downtime stage progress.
- Rollback dialog and history.
- Health-check and traffic transition events.
- Production action audit trail.

### M6 — Distributed operations

- Node/admin views.
- Capacity and placement information.
- Rescheduling events.
- Container/runtime health overview.

### M7 — Edge and application runtime

- Custom domains.
- DNS verification.
- TLS provisioning/renewal.
- Request metrics and latency.
- SSR runtime settings and autoscaling visibility.

## Definition of Frontend Done

The frontend is ready for a milestone when:

- Every route has loading, empty, error, permission, and success states.
- Every mutation is idempotent or protected from duplicate submission.
- API errors produce actionable UI, not raw JSON.
- Live data reconnects or explains its stale state.
- Keyboard, screen-reader, mobile, and reduced-motion behavior are tested.
- Secrets and tokens never appear in rendered output, URLs, logs, or analytics.
- Critical deploy, stop, rollback, domain, and environment actions have E2E tests.
- The README documents local startup and the dashboard's required API configuration.
- A user can move from source selection to a verified live deployment without
  needing to understand minidock internals.
