import { describe, it, expect, vi, beforeEach } from 'vitest';
import React from 'react';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { RollbackDialog } from '../components/RollbackDialog';
import { ReleaseProgress } from '../components/ReleaseProgress';
import { api, Release, ReleaseTransitionRecord } from '../lib/api';

vi.mock('../lib/api', () => ({
  api: {
    rollbackDeployment: vi.fn(),
    listDeploymentReleases: vi.fn(),
    listReleaseEvents: vi.fn(),
  },
}));

function renderWithClient(ui: React.ReactElement) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  return render(<QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>);
}

describe('RollbackDialog Component', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('does not render when isOpen is false', () => {
    renderWithClient(
      <RollbackDialog
        isOpen={false}
        onClose={vi.fn()}
        projectId="proj-1"
        deploymentId="dep-1"
      />
    );
    expect(screen.queryByText(/Rollback Deployment/i)).toBeNull();
  });

  it('renders modal with commit and drain options when isOpen is true', () => {
    renderWithClient(
      <RollbackDialog
        isOpen={true}
        onClose={vi.fn()}
        projectId="proj-1"
        deploymentId="dep-1"
        currentCommitSha="abcdef1234567890"
        environment="production"
      />
    );

    expect(screen.getByText('Rollback Deployment')).toBeDefined();
    expect(screen.getByText('abcdef1')).toBeDefined();
    expect(screen.getByText('production')).toBeDefined();
    expect(screen.getByText(/Connection Drain Timeout/i)).toBeDefined();
  });

  it('disables submit button until the authorization checkbox is checked', () => {
    renderWithClient(
      <RollbackDialog
        isOpen={true}
        onClose={vi.fn()}
        projectId="proj-1"
        deploymentId="dep-1"
      />
    );

    const submitBtn = screen.getByRole('button', { name: /Confirm Rollback/i });
    expect(submitBtn.hasAttribute('disabled')).toBe(true);

    const checkbox = screen.getByRole('checkbox');
    fireEvent.click(checkbox);

    expect(submitBtn.hasAttribute('disabled')).toBe(false);
  });

  it('submits rollback with configured drain timeout when confirmed', async () => {
    const mockOnClose = vi.fn();
    const mockOnSuccess = vi.fn();

    vi.mocked(api.rollbackDeployment).mockResolvedValueOnce({
      status: 'rolled_back',
      rollback_deployment_id: 'dep-new-rollback',
      target_release_id: 'rel-target-1',
      previous_healthy_deployment_id: 'dep-target-1',
      active_route_url: 'http://127.0.0.1:40001',
    });

    renderWithClient(
      <RollbackDialog
        isOpen={true}
        onClose={mockOnClose}
        projectId="proj-1"
        deploymentId="dep-1"
        onSuccess={mockOnSuccess}
      />
    );

    const select = screen.getByRole('combobox');
    fireEvent.change(select, { target: { value: '15' } });

    const checkbox = screen.getByRole('checkbox');
    fireEvent.click(checkbox);

    const submitBtn = screen.getByRole('button', { name: /Confirm Rollback/i });
    fireEvent.click(submitBtn);

    await waitFor(() => {
      expect(api.rollbackDeployment).toHaveBeenCalledWith('dep-1', {
        drain_timeout_secs: 15,
      });
      expect(mockOnSuccess).toHaveBeenCalledTimes(1);
      expect(mockOnClose).toHaveBeenCalledTimes(1);
    });
  });

  it('displays error banner when rollback API call fails', async () => {
    vi.mocked(api.rollbackDeployment).mockRejectedValueOnce(
      new Error('No healthy predecessor release available for rollback')
    );

    renderWithClient(
      <RollbackDialog
        isOpen={true}
        onClose={vi.fn()}
        projectId="proj-1"
        deploymentId="dep-1"
      />
    );

    const checkbox = screen.getByRole('checkbox');
    fireEvent.click(checkbox);

    const submitBtn = screen.getByRole('button', { name: /Confirm Rollback/i });
    fireEvent.click(submitBtn);

    await waitFor(() => {
      expect(
        screen.getByText(/No healthy predecessor release available for rollback/i)
      ).toBeDefined();
    });
  });
});

describe('ReleaseProgress Component', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders active route badge and pipeline steps for releases', async () => {
    const mockReleases: Release[] = [
      {
        id: 'rel-active-1',
        deployment_id: 'dep-1',
        version: 2,
        container_id: 'c-active-1',
        port: 40001,
        status: 'active',
        is_active_route: true,
        health_checked_at: '2026-09-08T12:01:00Z',
        activated_at: '2026-09-08T12:01:05Z',
        created_at: '2026-09-08T12:00:00Z',
      },
      {
        id: 'rel-old-1',
        deployment_id: 'dep-1',
        version: 1,
        container_id: 'c-old-1',
        port: 40000,
        status: 'stopped',
        is_active_route: false,
        health_checked_at: '2026-09-08T11:01:00Z',
        activated_at: '2026-09-08T11:01:05Z',
        stopped_at: '2026-09-08T12:01:10Z',
        created_at: '2026-09-08T11:00:00Z',
      },
    ];

    vi.mocked(api.listDeploymentReleases).mockResolvedValueOnce(mockReleases);

    renderWithClient(<ReleaseProgress deploymentId="dep-1" />);

    await waitFor(() => {
      expect(screen.getByText('Zero-Downtime Release Pipeline')).toBeDefined();
    });

    expect(screen.getByText('Active Route Protected')).toBeDefined();
    expect(screen.getByText('Live Traffic')).toBeDefined();
    expect(screen.getByText('Release v2')).toBeDefined();
    expect(screen.getByText('Release v1')).toBeDefined();
  });

  it('fetches and displays release events when toggled', async () => {
    const mockReleases: Release[] = [
      {
        id: 'rel-1',
        deployment_id: 'dep-1',
        version: 1,
        container_id: 'c-1',
        port: 40001,
        status: 'active',
        is_active_route: true,
        created_at: '2026-09-08T12:00:00Z',
      },
    ];

    const mockEvents: ReleaseTransitionRecord[] = [
      {
        id: 'evt-1',
        release_id: 'rel-1',
        from_status: 'ready',
        to_status: 'active',
        reason: 'Traffic router switched to target port',
        created_at: '2026-09-08T12:01:05Z',
      },
    ];

    vi.mocked(api.listDeploymentReleases).mockResolvedValueOnce(mockReleases);
    vi.mocked(api.listReleaseEvents).mockResolvedValueOnce(mockEvents);

    renderWithClient(<ReleaseProgress deploymentId="dep-1" />);

    await waitFor(() => {
      expect(screen.getByText('Zero-Downtime Release Pipeline')).toBeDefined();
    });

    const toggleBtn = screen.getByRole('button', { name: /View Events/i });
    fireEvent.click(toggleBtn);

    await waitFor(() => {
      expect(api.listReleaseEvents).toHaveBeenCalledWith('rel-1');
      expect(
        screen.getByText(/Traffic router switched to target port/i)
      ).toBeDefined();
    });
  });
});
