import { describe, it, expect } from 'vitest';
import React from 'react';
import { render, screen } from '@testing-library/react';
import { StatusBadge } from '../components/StatusBadge';
import { DeploymentCard } from '../components/DeploymentCard';
import { EmptyState } from '../components/EmptyState';
import { LogViewer } from '../components/LogViewer';

describe('StatusBadge', () => {
  it('renders running status badge', () => {
    render(<StatusBadge status="running" />);
    const badge = screen.getByText(/running/i);
    expect(badge).toBeDefined();
    expect(badge.className).toContain('badge-running');
  });

  it('renders failed status badge', () => {
    render(<StatusBadge status="failed" />);
    const badge = screen.getByText(/failed/i);
    expect(badge).toBeDefined();
    expect(badge.className).toContain('badge-failed');
  });
});

describe('DeploymentCard', () => {
  it('renders deployment information', () => {
    const deployment = {
      id: 'd-12345678',
      project_id: 'p-12345678',
      framework: 'static' as const,
      status: 'running' as const,
      port: 43123,
      url: 'http://127.0.0.1:43123',
      created_at: '2026-09-08T12:00:00Z',
    };
    render(<DeploymentCard deployment={deployment} />);
    expect(screen.getByText(/d-12345678/i)).toBeDefined();
    expect(screen.getByText(/http:\/\/127.0.0.1:43123/i)).toBeDefined();
    expect(screen.getByText(/running/i)).toBeDefined();
  });
});

describe('EmptyState', () => {
  it('renders empty state message and action', () => {
    render(
      <EmptyState
        title="No projects found"
        description="Create your first project to get started."
        actionLabel="Create Project"
        actionHref="/projects/new"
      />
    );
    expect(screen.getByText('No projects found')).toBeDefined();
    expect(screen.getByText('Create your first project to get started.')).toBeDefined();
    const link = screen.getByRole('link', { name: 'Create Project' });
    expect(link.getAttribute('href')).toBe('/projects/new');
  });
});

describe('LogViewer', () => {
  it('renders log lines cleanly in a monospace container', () => {
    const logs = 'Line 1: Building container\nLine 2: Server running on :43123';
    render(<LogViewer logs={logs} />);
    expect(screen.getByText(/Line 1: Building container/)).toBeDefined();
    expect(screen.getByText(/Line 2: Server running on :43123/)).toBeDefined();
  });
});
