export interface User {
  id: string;
  email: string;
  name: string;
  scopes: string[];
}

export interface Project {
  id: string;
  name: string;
  user_id?: string;
  source_dir: string;
  base_image: string;
  server_command: string[];
  created_at: string;
  active_deployment?: string | null;
}

export type DeploymentStatus =
  | 'pending'
  | 'queued'
  | 'retrying'
  | 'building'
  | 'running'
  | 'failed'
  | 'stopped'
  | 'cancelled';

export interface Deployment {
  id: string;
  project_id: string;
  framework: string;
  status: DeploymentStatus;
  image_path?: string | null;
  container_id?: string | null;
  port?: number | null;
  url?: string | null;
  created_at: string;
  finished_at?: string | null;
  error?: string | null;
  commit_sha?: string | null;
  attempt?: number;
  worker_id?: string | null;
  queue_wait_ms?: number | null;
  cache_status?: string | null;
}

export interface ApiTokenSummary {
  id: string;
  name: string;
  prefix: string;
  scopes: string[];
  revoked: boolean;
  last_used_at?: string | null;
  created_at: string;
}

export interface IssuedToken extends ApiTokenSummary {
  raw_token: string;
}

export interface ProviderRepository {
  id: string;
  connection_id: string;
  external_id: string;
  full_name: string;
  clone_url: string;
  default_branch: string;
  synced_at: string;
}

export interface ConnectProviderResponse {
  url: string;
  state: string;
}

export interface ProjectSourceConfig {
  repository: ProviderRepository | null;
  provider?: 'github' | 'gitlab' | null;
  target_branch?: string | null;
  webhook_url?: string | null;
  has_webhook_secret: boolean;
  last_delivery_at?: string | null;
}

export interface PullRequestSummary {
  number: number;
  head_sha: string;
  base_branch: string;
  action: 'opened' | 'updated' | 'closed';
}

export interface SourceEvent {
  id: string;
  project_id: string;
  provider: 'github' | 'gitlab';
  delivery_id: string;
  kind: 'push' | 'pull_request_opened' | 'pull_request_updated' | 'pull_request_closed';
  commit_sha: string;
  branch?: string | null;
  pull_request?: PullRequestSummary | null;
  idempotency_key: string;
  created_at: string;
}

export type PreviewStatus = 'building' | 'ready' | 'failed' | 'closed';

export interface Preview {
  id: string;
  project_id: string;
  provider: string;
  pr_number: number;
  head_sha: string;
  base_branch: string;
  head_branch: string;
  deployment_id?: string | null;
  hostname: string;
  status: PreviewStatus;
  closed_at?: string | null;
  cleanup_attempt: number;
  created_at: string;
  updated_at: string;
}

export class ApiError extends Error {
  status: number;
  constructor(status: number, message: string) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
  }
}

export function redactParams<T extends Record<string, any>>(params: T): Record<string, any> {
  const sanitized: Record<string, any> = {};
  for (const [k, v] of Object.entries(params)) {
    const lk = k.toLowerCase();
    if (
      lk.includes('secret') ||
      lk.includes('token') ||
      lk.includes('password') ||
      lk.includes('key')
    ) {
      sanitized[k] = '[REDACTED]';
    } else {
      sanitized[k] = v;
    }
  }
  return sanitized;
}

const getApiBase = () => {
  if (typeof window !== 'undefined') {
    return process.env.NEXT_PUBLIC_API_URL || 'http://127.0.0.1:4000';
  }
  return process.env.API_URL || process.env.NEXT_PUBLIC_API_URL || 'http://127.0.0.1:4000';
};

async function request<T>(path: string, options: RequestInit = {}): Promise<T> {
  const url = `${getApiBase()}${path}`;
  const headers = new Headers(options.headers || {});
  if (!headers.has('Content-Type') && options.body && typeof options.body === 'string') {
    headers.set('Content-Type', 'application/json');
  }

  const res = await fetch(url, {
    ...options,
    headers,
    credentials: 'include',
  });

  if (res.status === 204) {
    return undefined as unknown as T;
  }

  let data;
  try {
    data = await res.json();
  } catch {
    data = null;
  }

  if (!res.ok) {
    const errorMsg = data?.error || `Request failed with status ${res.status}`;
    throw new ApiError(res.status, errorMsg);
  }

  return data as T;
}

export const api = {
  login: (credentials: { email: string; password: string }) =>
    request<{ user: User; token: string }>('/v1/auth/session', {
      method: 'POST',
      body: JSON.stringify(credentials),
    }),

  logout: () =>
    request<void>('/v1/auth/session', {
      method: 'DELETE',
    }),

  me: () => request<User>('/v1/me'),

  listProjects: () => request<Project[]>('/v1/projects'),

  getProject: (projectId: string) => request<Project>(`/v1/projects/${projectId}`),

  createProject: (input: { name: string; source_dir: string; base_image: string }) =>
    request<Project>('/v1/projects', {
      method: 'POST',
      body: JSON.stringify(input),
    }),

  listDeployments: (projectId: string) =>
    request<Deployment[]>(`/v1/projects/${projectId}/deployments`),

  createDeployment: (projectId: string) =>
    request<Deployment>(`/v1/projects/${projectId}/deployments`, {
      method: 'POST',
    }),

  getDeployment: (deploymentId: string) =>
    request<Deployment>(`/v1/deployments/${deploymentId}`),

  getDeploymentLogs: (deploymentId: string) =>
    request<{ logs: string }>(`/v1/deployments/${deploymentId}/logs`),

  stopDeployment: (deploymentId: string) =>
    request<Deployment>(`/v1/deployments/${deploymentId}/stop`, {
      method: 'POST',
    }),

  listApiTokens: () => request<ApiTokenSummary[]>('/v1/me/api-tokens'),

  createApiToken: (input: { name: string; scopes?: string[] }) =>
    request<IssuedToken>('/v1/me/api-tokens', {
      method: 'POST',
      body: JSON.stringify(input),
    }),

  revokeApiToken: (tokenId: string) =>
    request<void>(`/v1/me/api-tokens/${tokenId}`, {
      method: 'DELETE',
    }),

  connectProvider: (provider: 'github' | 'gitlab') =>
    request<ConnectProviderResponse>(`/v1/providers/${provider}/connect`),

  listProviderRepositories: (provider: 'github' | 'gitlab') =>
    request<ProviderRepository[]>(`/v1/providers/${provider}/repositories`),

  getProjectSource: (projectId: string) =>
    request<ProjectSourceConfig>(`/v1/projects/${projectId}/source`),

  updateProjectSource: (
    projectId: string,
    input: { repository_id: string; target_branch?: string }
  ) =>
    request<ProjectSourceConfig>(`/v1/projects/${projectId}/source`, {
      method: 'POST',
      body: JSON.stringify(input),
    }),

  disconnectProjectSource: (projectId: string) =>
    request<void>(`/v1/projects/${projectId}/source`, {
      method: 'DELETE',
    }),

  listProjectSourceEvents: (projectId: string) =>
    request<SourceEvent[]>(`/v1/projects/${projectId}/source/events`),

  cancelDeployment: (deploymentId: string) =>
    request<Deployment>(`/v1/deployments/${deploymentId}/cancel`, {
      method: 'POST',
    }),

  retryDeployment: (deploymentId: string) =>
    request<Deployment>(`/v1/deployments/${deploymentId}/retry`, {
      method: 'POST',
    }),

  listProjectPreviews: (projectId: string) =>
    request<Preview[]>(`/v1/projects/${projectId}/previews`),

  getPreview: (previewId: string) =>
    request<Preview>(`/v1/previews/${previewId}`),

  promotePreview: (previewId: string) =>
    request<Deployment>(`/v1/previews/${previewId}/promote`, {
      method: 'POST',
    }),

  stopPreview: (previewId: string) =>
    request<Preview>(`/v1/previews/${previewId}/stop`, {
      method: 'POST',
    }),
};
