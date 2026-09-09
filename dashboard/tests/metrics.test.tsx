import { describe, it, expect, vi, beforeEach } from 'vitest';
import React from 'react';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { LogViewer } from '../components/LogViewer';
import ProjectMetricsPage from '../app/projects/[projectId]/metrics/page';
import DeploymentDetailPage from '../app/projects/[projectId]/deployments/[deploymentId]/page';
import { api } from '../lib/api';

vi.mock('next/navigation', () => ({
  useParams: () => ({ projectId: 'proj-test-123', deploymentId: 'dep-test-456' }),
  useRouter: () => ({ push: vi.fn() }),
  usePathname: () => '/projects/proj-test-123/metrics',
}));

vi.mock('../lib/api', () => ({
  api: {
    me: vi.fn().mockResolvedValue({ id: 'u-1', email: 'user@example.com', name: 'User 1' }),
    listProjects: vi.fn().mockResolvedValue([
      { id: 'proj-test-123', name: 'Test Project' },
    ]),
    getProject: vi.fn().mockResolvedValue({
      id: 'proj-test-123',
      name: 'Test Project',
      source_dir: '/tmp',
      base_image: 'caddy:2-alpine',
    }),
    getDeployment: vi.fn().mockResolvedValue({
      id: 'dep-test-456',
      project_id: 'proj-test-123',
      framework: 'static',
      status: 'running',
      port: 43123,
      url: 'http://127.0.0.1:43123',
      created_at: '2026-09-08T12:00:00Z',
      cache_status: 'HIT',
      cache_key: 'sha256:abcd1234efgh5678ijkl9012mnop',
      build_duration_ms: 1200,
      cached_duration_ms: 8500,
      artifact_size_bytes: 4194304,
    }),
    getDeploymentLogs: vi.fn().mockResolvedValue({
      logs: 'Step 1: Cache hit for sha256:abcd1234\nStep 2: Restored artifact\nServer ready',
    }),
    getProjectMetrics: vi.fn().mockResolvedValue({
      project_id: 'proj-test-123',
      range: '1h',
      resolution: '1m',
      start: '2026-09-08T11:00:00Z',
      end: '2026-09-08T12:00:00Z',
      last_updated: '2026-09-08T12:00:00Z',
      series: [
        {
          metric_name: 'request_total',
          unit: 'count',
          environment: 'production',
          has_data: true,
          is_partial: true,
          summary: {
            total: 150,
            count: 10,
            avg: 15,
            min: 5,
            max: 25,
            p50: null,
            p95: null,
            p99: null,
          },
          points: [
            {
              timestamp: '2026-09-08T11:55:00Z',
              count: 5,
              value: 15,
              sum: 15,
              min: 5,
              max: 25,
              avg: 15,
              p50: null,
              p95: null,
              p99: null,
              is_partial: true,
            },
          ],
        },
        {
          metric_name: 'request_latency_ms',
          unit: 'milliseconds',
          environment: 'production',
          has_data: true,
          is_partial: false,
          summary: {
            total: 45.2,
            count: 50,
            avg: 45.2,
            min: 12.0,
            max: 120.0,
            p50: 42.0,
            p95: 98.0,
            p99: 115.0,
          },
          points: [],
        },
        {
          metric_name: 'build_duration_seconds',
          unit: 'seconds',
          environment: 'production',
          has_data: false,
          is_partial: false,
          summary: {
            total: 0,
            count: 0,
            avg: 0,
            min: 0,
            max: 0,
            p50: null,
            p95: null,
            p99: null,
          },
          points: [],
        },
      ],
    }),
    clearProjectCache: vi.fn().mockResolvedValue({
      success: true,
      message: 'Project build cache cleared successfully',
    }),
  },
}));

function renderWithClient(ui: React.ReactElement) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  });
  return render(<QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>);
}

describe('LogViewer features', () => {
  it('renders search input and filters lines by query', () => {
    const logs = 'Line 1: compile typescript\nLine 2: running test\nLine 3: bundle complete';
    render(<LogViewer logs={logs} />);

    expect(screen.getByText(/Line 1: compile typescript/)).toBeDefined();
    expect(screen.getByText(/Line 2: running test/)).toBeDefined();

    const searchInput = screen.getByLabelText('Search logs');
    fireEvent.change(searchInput, { target: { value: 'bundle' } });

    expect(screen.getByText(/1 matched/)).toBeDefined();
    expect(screen.getByText(/Line 3: bundle complete/)).toBeDefined();
    expect(screen.queryByText(/Line 1: compile typescript/)).toBeNull();
  });

  it('supports pause/follow toggle and download button', () => {
    const logs = 'Line 1: init';
    render(<LogViewer logs={logs} deploymentId="dep-123" isStreaming={true} />);

    const followBtn = screen.getByRole('button', { name: /pause log auto-scroll/i });
    expect(followBtn.textContent).toBe('Following');

    fireEvent.click(followBtn);
    expect(screen.getByText(/Scroll Paused/i)).toBeDefined();
    expect(followBtn.textContent).toBe('Follow');

    const downloadBtn = screen.getByRole('button', { name: /download logs as text file/i });
    expect(downloadBtn).toBeDefined();
  });

  it('masks secret patterns client-side for safety', () => {
    const logs =
      'Using key AKIA1234567890ABCDEF and Bearer secret-auth-token-1234\npassword=MySuperSecretPassword';
    render(<LogViewer logs={logs} />);

    expect(screen.getAllByText(/\[REDACTED\]/).length).toBe(2);
    expect(screen.queryByText(/AKIA1234567890ABCDEF/)).toBeNull();
    expect(screen.queryByText(/secret-auth-token-1234/)).toBeNull();
    expect(screen.queryByText(/MySuperSecretPassword/)).toBeNull();
  });

  it('displays reconnecting status and retry trigger', () => {
    const onReconnect = vi.fn();
    render(<LogViewer logs="Sample log" isReconnecting={true} onReconnect={onReconnect} />);

    expect(screen.getByText(/Reconnecting.../)).toBeDefined();
    const retryBtn = screen.getByRole('button', { name: /retry/i });
    fireEvent.click(retryBtn);
    expect(onReconnect).toHaveBeenCalled();
  });
});

describe('DeploymentDetailPage cache and artifact metadata', () => {
  it('renders cache status badge, cache key, duration comparison, and artifact size', async () => {
    renderWithClient(<DeploymentDetailPage />);

    // Cache status HIT
    const hitBadge = await screen.findByText('HIT');
    expect(hitBadge).toBeDefined();

    // Cache key summary
    expect(screen.getAllByText(/sha256:abcd1234/).length).toBeGreaterThan(0);

    // Duration comparison (1.2s vs cached ~8.5s)
    expect(screen.getByText(/1.2s/)).toBeDefined();
    expect(screen.getByText(/cached ~8.5s/)).toBeDefined();

    // Build artifact metadata (4.00 MB)
    expect(screen.getByText(/4.00 MB/)).toBeDefined();
  });
});

describe('ProjectMetricsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders telemetry header, timezone UTC, and time range controls', async () => {
    renderWithClient(<ProjectMetricsPage />);

    expect(await screen.findByText('Telemetry & Metrics')).toBeDefined();
    expect(screen.getByText('UTC')).toBeDefined();

    // Time ranges
    const btn15m = screen.getByRole('button', { name: '15m' });
    const btn1h = screen.getByRole('button', { name: '1h' });
    const btn24h = screen.getByRole('button', { name: '24h' });
    expect(btn15m).toBeDefined();
    expect(btn1h.getAttribute('aria-pressed')).toBe('true');

    // Click 24h
    fireEvent.click(btn24h);
    expect(btn24h.getAttribute('aria-pressed')).toBe('true');
  });

  it('renders metric cards with explicit units, partial data badges, and empty states', async () => {
    renderWithClient(<ProjectMetricsPage />);

    // HTTP Requests with unit: count
    expect(await screen.findByText('HTTP Requests')).toBeDefined();
    expect(screen.getAllByText(/Unit: count/i).length).toBeGreaterThan(0);

    // Partial data badge for request_total
    expect(screen.getByText(/● Partial data: ongoing window/i)).toBeDefined();

    // Latency card with percentiles
    expect(screen.getByText('Response Latency')).toBeDefined();
    expect(screen.getByText(/P50 Latency/i)).toBeDefined();
    expect(screen.getByText('42.0 ms')).toBeDefined();
    expect(screen.getByText('98.0 ms')).toBeDefined();

    // Empty state explanation for build duration
    expect(
      screen.getByText(/No metric data collected for this project in the selected 1h window/i)
    ).toBeDefined();
  });

  it('toggles accessible tabular alternative for screen readers', async () => {
    renderWithClient(<ProjectMetricsPage />);

    const toggleBtn = await screen.findByRole('button', { name: /Toggle accessible tabular view/i });
    expect(toggleBtn.textContent).toBe('View as Tables');

    fireEvent.click(toggleBtn);

    // Tables are now visible
    const table = screen.getByRole('table', { name: /Detailed tabular data for request_total/i });
    expect(table).toBeDefined();
    expect(screen.getAllByText('Timestamp (UTC)').length).toBeGreaterThan(0);
    expect(screen.getByText('Partial')).toBeDefined();
  });

  it('clears project cache upon button click', async () => {
    vi.spyOn(window, 'confirm').mockImplementation(() => true);
    renderWithClient(<ProjectMetricsPage />);

    const clearBtn = await screen.findByRole('button', { name: /Clear Build Cache/i });
    fireEvent.click(clearBtn);

    await waitFor(() => {
      expect(api.clearProjectCache).toHaveBeenCalledWith('proj-test-123');
    });
    expect(await screen.findByText(/Project build cache cleared successfully/i)).toBeDefined();
  });
});
