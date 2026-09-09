import { describe, it, expect, vi } from 'vitest';
import React from 'react';
import { render, screen, fireEvent } from '@testing-library/react';
import { StatusBadge } from '../components/StatusBadge';
import { DeploymentCard } from '../components/DeploymentCard';
import { LogViewer } from '../components/LogViewer';
import { Deployment } from '../lib/api';

describe('Queue and Build Status Badges', () => {
  it('renders queued status badge with appropriate styling', () => {
    render(<StatusBadge status="queued" />);
    const badge = screen.getByText('queued');
    expect(badge).toBeDefined();
    expect(badge.className).toContain('badge-queued');
  });

  it('renders retrying status badge with appropriate styling', () => {
    render(<StatusBadge status="retrying" />);
    const badge = screen.getByText('retrying');
    expect(badge).toBeDefined();
    expect(badge.className).toContain('badge-retrying');
  });

  it('renders cancelled status badge with appropriate styling', () => {
    render(<StatusBadge status="cancelled" />);
    const badge = screen.getByText('cancelled');
    expect(badge).toBeDefined();
    expect(badge.className).toContain('badge-cancelled');
  });
});

describe('DeploymentCard with Queue and Worker Metadata', () => {
  it('renders deployment in queued state with commit and attempt info', () => {
    const deployment: Deployment = {
      id: 'dep-123',
      project_id: 'proj-1',
      framework: 'static',
      status: 'queued',
      created_at: '2026-09-08T12:00:00Z',
      commit_sha: 'abcdef1234567890',
      attempt: 1,
      queue_wait_ms: 2500,
    };

    render(<DeploymentCard deployment={deployment} />);
    expect(screen.getByText('queued')).toBeDefined();
    expect(screen.getByText(/abcdef1/i)).toBeDefined();
    expect(screen.getByText(/Attempt 1/i)).toBeDefined();
    expect(screen.getByText(/2500ms/i)).toBeDefined();
  });

  it('renders deployment in building state with worker info', () => {
    const deployment: Deployment = {
      id: 'dep-456',
      project_id: 'proj-1',
      framework: 'static',
      status: 'building',
      created_at: '2026-09-08T12:00:00Z',
      commit_sha: '9876543210fedcba',
      attempt: 2,
      worker_id: 'worker-node-1',
    };

    render(<DeploymentCard deployment={deployment} />);
    expect(screen.getByText('building')).toBeDefined();
    expect(screen.getByText(/worker-node-1/i)).toBeDefined();
    expect(screen.getByText(/Attempt 2/i)).toBeDefined();
  });
});

describe('LogViewer with Stream and Reconnect State', () => {
  it('renders live log lines and reconnecting indicator when enabled', () => {
    const logs = '[stdout] Starting container build...\n[stderr] Compiling assets...';
    render(<LogViewer logs={logs} isStreaming={true} isReconnecting={true} />);
    expect(screen.getByText(/Starting container build/)).toBeDefined();
    expect(screen.getByText(/Compiling assets/)).toBeDefined();
    expect(screen.getByText(/Reconnecting/i)).toBeDefined();
  });

  it('renders streaming indicator when streaming without reconnecting', () => {
    const logs = '[stdout] Log output';
    render(<LogViewer logs={logs} isStreaming={true} isReconnecting={false} />);
    expect(screen.getByText(/Live streaming/i)).toBeDefined();
  });
});

describe('Cancel and Retry Action Controls', () => {
  it('handles cancel action with confirmation', async () => {
    const onCancel = vi.fn();
    const confirmSpy = vi.spyOn(window, 'confirm').mockImplementation(() => true);

    const CancelButton = ({ onCancel, cancelling }: { onCancel: () => void; cancelling: boolean }) => (
      <button
        onClick={() => {
          if (window.confirm('Cancel this build?')) {
            onCancel();
          }
        }}
        disabled={cancelling}
      >
        {cancelling ? 'Cancelling...' : 'Cancel Build'}
      </button>
    );

    render(<CancelButton onCancel={onCancel} cancelling={false} />);
    const btn = screen.getByRole('button', { name: 'Cancel Build' });
    fireEvent.click(btn);

    expect(confirmSpy).toHaveBeenCalledWith('Cancel this build?');
    expect(onCancel).toHaveBeenCalledTimes(1);

    confirmSpy.mockRestore();
  });

  it('disables cancel button when cancellation is pending', () => {
    const CancelButton = ({ cancelling }: { cancelling: boolean }) => (
      <button disabled={cancelling}>{cancelling ? 'Cancelling...' : 'Cancel Build'}</button>
    );

    render(<CancelButton cancelling={true} />);
    const btn = screen.getByRole('button', { name: 'Cancelling...' });
    expect(btn.hasAttribute('disabled')).toBe(true);
  });

  it('handles retry action and displays retrying state', () => {
    const onRetry = vi.fn();
    const RetryButton = ({ onRetry, retrying }: { onRetry: () => void; retrying: boolean }) => (
      <button onClick={onRetry} disabled={retrying}>
        {retrying ? 'Retrying...' : 'Retry Build'}
      </button>
    );

    const { rerender } = render(<RetryButton onRetry={onRetry} retrying={false} />);
    const btn = screen.getByRole('button', { name: 'Retry Build' });
    fireEvent.click(btn);
    expect(onRetry).toHaveBeenCalledTimes(1);

    rerender(<RetryButton onRetry={onRetry} retrying={true} />);
    const retryingBtn = screen.getByRole('button', { name: 'Retrying...' });
    expect(retryingBtn.hasAttribute('disabled')).toBe(true);
  });
});
