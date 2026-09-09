import { describe, it, expect, vi, beforeEach } from 'vitest';
import React from 'react';
import { render, screen, fireEvent, waitFor } from '@testing-library/react';
import { QueryClient, QueryClientProvider } from '@tanstack/react-query';
import { StatusBadge } from '../components/StatusBadge';
import ProjectPreviewsPage from '../app/projects/[projectId]/previews/page';
import PreviewDetailPage from '../app/projects/[projectId]/previews/[previewId]/page';
import { api, Preview } from '../lib/api';

vi.mock('next/navigation', () => ({
  useParams: () => ({ projectId: 'proj-preview-123', previewId: 'prev-456' }),
  useRouter: () => ({ push: vi.fn(), replace: vi.fn() }),
  usePathname: () => '/projects/proj-preview-123/previews',
}));

vi.mock('../lib/api', () => ({
  api: {
    me: vi.fn().mockResolvedValue({ id: 'u1', email: 'test@example.com', name: 'User', scopes: ['*'] }),
    getProject: vi.fn(),
    listProjectPreviews: vi.fn(),
    getPreview: vi.fn(),
    promotePreview: vi.fn(),
    stopPreview: vi.fn(),
    listProjectSourceEvents: vi.fn(),
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

describe('Preview Status Badges', () => {
  it('renders ready status badge with green running styling', () => {
    render(<StatusBadge status="ready" />);
    const badge = screen.getByText('ready');
    expect(badge).toBeDefined();
    expect(badge.className).toContain('badge-ready');
  });

  it('renders closed status badge with gray stopped styling', () => {
    render(<StatusBadge status="closed" />);
    const badge = screen.getByText('closed');
    expect(badge).toBeDefined();
    expect(badge.className).toContain('badge-closed');
  });
});

describe('ProjectPreviewsPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  const mockProject = {
    id: 'proj-preview-123',
    name: 'preview-app',
    source_dir: '/tmp/app',
    base_image: '/tmp/base.tar.gz',
    server_command: ['/bin/busybox'],
    created_at: '2026-09-08T10:00:00Z',
  };

  const mockPreviews: Preview[] = [
    {
      id: 'prev-456',
      project_id: 'proj-preview-123',
      provider: 'github',
      pr_number: 42,
      head_sha: '1234567890abcdef',
      base_branch: 'main',
      head_branch: 'feature/auth',
      deployment_id: 'dep-789',
      hostname: 'preview-app-pr-42.preview.local',
      status: 'ready',
      closed_at: null,
      cleanup_attempt: 0,
      created_at: '2026-09-08T11:00:00Z',
      updated_at: '2026-09-08T11:05:00Z',
    },
    {
      id: 'prev-999',
      project_id: 'proj-preview-123',
      provider: 'github',
      pr_number: 30,
      head_sha: 'fedcba0987654321',
      base_branch: 'main',
      head_branch: 'fix/nav',
      deployment_id: null,
      hostname: 'preview-app-pr-30.preview.local',
      status: 'closed',
      closed_at: '2026-09-08T11:30:00Z',
      cleanup_attempt: 1,
      created_at: '2026-09-08T09:00:00Z',
      updated_at: '2026-09-08T11:30:00Z',
    },
  ];

  it('renders previews list with PR details, status, commit, and hostname', async () => {
    vi.mocked(api.getProject).mockResolvedValue(mockProject);
    vi.mocked(api.listProjectPreviews).mockResolvedValue(mockPreviews);
    vi.mocked(api.listProjectSourceEvents).mockResolvedValue([]);

    renderWithClient(<ProjectPreviewsPage />);

    expect(await screen.findByText(/PR #42/i)).toBeDefined();
    expect(screen.getByText('ready')).toBeDefined();
    expect(screen.getByText('1234567')).toBeDefined();
    expect(screen.getByText('preview-app-pr-42.preview.local')).toBeDefined();

    // Verify PR #30 closed preview
    expect(screen.getByText(/PR #30/i)).toBeDefined();
    expect(screen.getByText('closed')).toBeDefined();
    expect(screen.getByText(/Closed \/ Expired/i)).toBeDefined();
  });
});

describe('PreviewDetailPage', () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  const mockProject = {
    id: 'proj-preview-123',
    name: 'preview-app',
    source_dir: '/tmp/app',
    base_image: '/tmp/base.tar.gz',
    server_command: ['/bin/busybox'],
    created_at: '2026-09-08T10:00:00Z',
  };

  const mockActivePreview: Preview = {
    id: 'prev-456',
    project_id: 'proj-preview-123',
    provider: 'github',
    pr_number: 42,
    head_sha: 'abcdef1234567890',
    base_branch: 'main',
    head_branch: 'feature/auth',
    deployment_id: 'dep-789',
    hostname: 'preview-app-pr-42.preview.local',
    status: 'ready',
    closed_at: null,
    cleanup_attempt: 0,
    created_at: '2026-09-08T11:00:00Z',
    updated_at: '2026-09-08T11:05:00Z',
  };

  it('renders active preview details, open preview link, promote and stop buttons', async () => {
    vi.mocked(api.getProject).mockResolvedValue(mockProject);
    vi.mocked(api.getPreview).mockResolvedValue(mockActivePreview);
    vi.mocked(api.promotePreview).mockResolvedValue({
      id: 'dep-prod-111',
      project_id: 'proj-preview-123',
      framework: 'static',
      status: 'queued',
      created_at: '2026-09-08T12:00:00Z',
      commit_sha: 'abcdef1234567890',
      target: 'production',
    });

    renderWithClient(<PreviewDetailPage />);

    await waitFor(() => {
      expect(screen.getByText(/Preview: PR #42/i)).toBeDefined();
    });

    expect(screen.getByText(/Open Preview/i)).toBeDefined();
    expect(screen.getByRole('button', { name: /Promote to Production/i })).toBeDefined();
    expect(screen.getByRole('button', { name: /Stop Preview/i })).toBeDefined();

    // Trigger promote
    fireEvent.click(screen.getByRole('button', { name: /Promote to Production/i }));
    await waitFor(() => {
      expect(api.promotePreview).toHaveBeenCalledWith('prev-456');
      expect(screen.getByText(/Preview promoted to production!/i)).toBeDefined();
    });
  });

  it('renders prominent closed and teardown state callout when preview is closed', async () => {
    const closedPreview: Preview = {
      ...mockActivePreview,
      status: 'closed',
      closed_at: '2026-09-08T12:30:00Z',
    };
    vi.mocked(api.getProject).mockResolvedValue(mockProject);
    vi.mocked(api.getPreview).mockResolvedValue(closedPreview);

    renderWithClient(<PreviewDetailPage />);

    await waitFor(() => {
      expect(screen.getByText(/Preview Closed and Teardown Complete/i)).toBeDefined();
    });

    // Promote and stop buttons should not be present
    expect(screen.queryByRole('button', { name: /Promote to Production/i })).toBeNull();
    expect(screen.queryByRole('button', { name: /Stop Preview/i })).toBeNull();
  });
});
