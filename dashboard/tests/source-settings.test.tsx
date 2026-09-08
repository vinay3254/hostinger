import { describe, it, expect, vi, beforeEach } from 'vitest';
import React from 'react';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { SourceConnectionCard } from '../components/SourceConnectionCard';
import { api } from '../lib/api';

vi.mock('../lib/api', () => ({
  api: {
    getProjectSource: vi.fn(),
    connectProvider: vi.fn(),
    listProviderRepositories: vi.fn(),
    updateProjectSource: vi.fn(),
    disconnectProjectSource: vi.fn(),
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

describe('SourceConnectionCard', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it('renders disconnected state and handles provider selection & connect redirect', async () => {
    const originalLocation = window.location;
    // @ts-ignore
    delete window.location;
    window.location = { ...originalLocation, href: '' } as any;

    vi.mocked(api.connectProvider).mockResolvedValue({
      url: 'https://github.com/login/oauth/authorize?client_id=123',
      state: 'state_123',
    });

    renderWithClient(<SourceConnectionCard projectId="proj-1" initialSourceConfig={null} />);

    expect(screen.getByText(/Connect Git Repository/i)).toBeDefined();
    expect(screen.getByLabelText(/GitHub/i)).toBeDefined();
    expect(screen.getByLabelText(/GitLab/i)).toBeDefined();

    // Select GitLab
    fireEvent.click(screen.getByLabelText(/GitLab/i));

    // Click Connect Provider
    const connectBtn = screen.getByRole('button', { name: /Connect GitLab/i });
    fireEvent.click(connectBtn);

    await waitFor(() => {
      expect(api.connectProvider).toHaveBeenCalledWith('gitlab');
    });

    window.location = originalLocation;
  });

  it('displays repositories, allows branch selection, and links repository', async () => {
    const mockRepos = [
      {
        id: 'repo-1',
        connection_id: 'conn-1',
        external_id: '111',
        full_name: 'vinay/web-app',
        clone_url: 'https://github.com/vinay/web-app.git',
        default_branch: 'main',
        synced_at: new Date().toISOString(),
      },
    ];

    vi.mocked(api.listProviderRepositories).mockResolvedValue(mockRepos);
    vi.mocked(api.updateProjectSource).mockResolvedValue({
      repository: mockRepos[0],
      provider: 'github',
      target_branch: 'develop',
      webhook_url: '/v1/webhooks/github/proj-1',
      has_webhook_secret: true,
      last_delivery_at: null,
    });

    const onUpdate = vi.fn();
    renderWithClient(
      <SourceConnectionCard
        projectId="proj-1"
        initialSourceConfig={null}
        initialRepositories={mockRepos}
        onUpdate={onUpdate}
      />
    );

    // Repository select is visible
    const repoSelect = screen.getByLabelText(/Select Repository/i) as HTMLSelectElement;
    expect(repoSelect).toBeDefined();
    expect(screen.getByText('vinay/web-app')).toBeDefined();

    // Branch input is populated with default_branch
    const branchInput = screen.getByLabelText(/Target Branch/i) as HTMLInputElement;
    expect(branchInput.value).toBe('main');

    // Change branch
    fireEvent.change(branchInput, { target: { value: 'develop' } });
    expect(branchInput.value).toBe('develop');

    // Click Link Repository
    const linkBtn = screen.getByRole('button', { name: /Link Repository/i });
    fireEvent.click(linkBtn);

    await waitFor(() => {
      expect(api.updateProjectSource).toHaveBeenCalledWith('proj-1', {
        repository_id: 'repo-1',
        target_branch: 'develop',
      });
      expect(onUpdate).toHaveBeenCalled();
    });
  });

  it('handles connection and linking errors gracefully', async () => {
    vi.mocked(api.updateProjectSource).mockRejectedValue(new Error('Permission denied'));

    const mockRepos = [
      {
        id: 'repo-1',
        connection_id: 'conn-1',
        external_id: '111',
        full_name: 'vinay/web-app',
        clone_url: 'https://github.com/vinay/web-app.git',
        default_branch: 'main',
        synced_at: new Date().toISOString(),
      },
    ];

    renderWithClient(
      <SourceConnectionCard
        projectId="proj-1"
        initialSourceConfig={null}
        initialRepositories={mockRepos}
      />
    );

    const linkBtn = screen.getByRole('button', { name: /Link Repository/i });
    fireEvent.click(linkBtn);

    await waitFor(() => {
      expect(screen.getByRole('alert')).toBeDefined();
      expect(screen.getByText(/Permission denied/i)).toBeDefined();
    });
  });

  it('renders connected state with webhook status and copyable URL', async () => {
    const connectedConfig = {
      repository: {
        id: 'repo-1',
        connection_id: 'conn-1',
        external_id: '111',
        full_name: 'vinay/cool-site',
        clone_url: 'https://github.com/vinay/cool-site.git',
        default_branch: 'main',
        synced_at: new Date().toISOString(),
      },
      provider: 'github' as const,
      target_branch: 'main',
      webhook_url: '/v1/webhooks/github/proj-1',
      has_webhook_secret: true,
      last_delivery_at: '2026-09-08T12:00:00Z',
    };

    renderWithClient(<SourceConnectionCard projectId="proj-1" initialSourceConfig={connectedConfig} />);

    expect(screen.getByText('vinay/cool-site')).toBeDefined();
    expect(screen.getByText(/Branch:/i)).toBeDefined();
    expect(screen.getByText('main')).toBeDefined();
    expect(screen.getByDisplayValue(/.*\/v1\/webhooks\/github\/proj-1/)).toBeDefined();
    expect(screen.getByText(/Active \(Configured\)/i)).toBeDefined();
    expect(screen.getByText(/Last Delivery/i)).toBeDefined();
  });

  it('confirms before disconnecting repository', async () => {
    const confirmSpy = vi.spyOn(window, 'confirm').mockReturnValue(true);
    vi.mocked(api.disconnectProjectSource).mockResolvedValue(undefined);

    const connectedConfig = {
      repository: {
        id: 'repo-1',
        connection_id: 'conn-1',
        external_id: '111',
        full_name: 'vinay/cool-site',
        clone_url: 'https://github.com/vinay/cool-site.git',
        default_branch: 'main',
        synced_at: new Date().toISOString(),
      },
      provider: 'github' as const,
      target_branch: 'main',
      webhook_url: '/v1/webhooks/github/proj-1',
      has_webhook_secret: true,
      last_delivery_at: null,
    };

    const onUpdate = vi.fn();
    renderWithClient(
      <SourceConnectionCard
        projectId="proj-1"
        initialSourceConfig={connectedConfig}
        onUpdate={onUpdate}
      />
    );

    const disconnectBtn = screen.getByRole('button', { name: /Disconnect Repository/i });
    fireEvent.click(disconnectBtn);

    expect(confirmSpy).toHaveBeenCalledWith(
      expect.stringContaining('Are you sure you want to disconnect')
    );

    await waitFor(() => {
      expect(api.disconnectProjectSource).toHaveBeenCalledWith('proj-1');
      expect(onUpdate).toHaveBeenCalled();
    });

    confirmSpy.mockRestore();
  });
});
