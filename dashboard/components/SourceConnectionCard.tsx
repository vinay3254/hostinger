'use client';

import React, { useState, useEffect } from 'react';
import { useQuery } from '@tanstack/react-query';
import { api, ProjectSourceConfig, ProviderRepository } from '../lib/api';

interface SourceConnectionCardProps {
  projectId: string;
  initialSourceConfig?: ProjectSourceConfig | null;
  initialRepositories?: ProviderRepository[];
  onUpdate?: () => void;
}

export const SourceConnectionCard: React.FC<SourceConnectionCardProps> = ({
  projectId,
  initialSourceConfig,
  initialRepositories,
  onUpdate,
}) => {
  const [provider, setProvider] = useState<'github' | 'gitlab'>('github');
  const [selectedRepoId, setSelectedRepoId] = useState<string>('');
  const [targetBranch, setTargetBranch] = useState<string>('main');
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState<boolean>(false);
  const [isLinking, setIsLinking] = useState<boolean>(false);
  const [isDisconnecting, setIsDisconnecting] = useState<boolean>(false);

  // Source configuration query
  const { data: sourceConfig, refetch: refetchConfig } = useQuery({
    queryKey: ['project-source', projectId],
    queryFn: () => api.getProjectSource(projectId),
    initialData: initialSourceConfig !== undefined ? initialSourceConfig : undefined,
    enabled: Boolean(projectId) && initialSourceConfig === undefined,
  });

  // Repositories query for chosen provider
  const {
    data: repositories = initialRepositories || [],
    isLoading: isLoadingRepos,
    isError: isReposError,
  } = useQuery({
    queryKey: ['provider-repos', provider],
    queryFn: () => api.listProviderRepositories(provider),
    initialData: initialRepositories,
    enabled: Boolean(provider) && !sourceConfig?.repository && initialRepositories === undefined,
  });

  // Update selected repo when repo list changes
  useEffect(() => {
    if (repositories && repositories.length > 0) {
      if (!selectedRepoId || !repositories.some((r) => r.id === selectedRepoId)) {
        setSelectedRepoId(repositories[0].id);
        setTargetBranch(repositories[0].default_branch || 'main');
      }
    }
  }, [repositories, selectedRepoId]);

  const handleProviderChange = (newProvider: 'github' | 'gitlab') => {
    setProvider(newProvider);
    setError(null);
  };

  const handleRepoChange = (repoId: string) => {
    setSelectedRepoId(repoId);
    const repo = repositories.find((r) => r.id === repoId);
    if (repo?.default_branch) {
      setTargetBranch(repo.default_branch);
    }
  };

  const handleConnectProvider = async () => {
    setError(null);
    try {
      const resp = await api.connectProvider(provider);
      if (resp.url) {
        window.location.href = resp.url;
      }
    } catch (err: any) {
      setError(err.message || 'Failed to start provider authorization');
    }
  };

  const handleLinkRepository = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!selectedRepoId) {
      setError('Please select a repository to link.');
      return;
    }
    setError(null);
    setIsLinking(true);
    try {
      await api.updateProjectSource(projectId, {
        repository_id: selectedRepoId,
        target_branch: targetBranch.trim() || 'main',
      });
      await refetchConfig();
      if (onUpdate) onUpdate();
    } catch (err: any) {
      setError(err.message || 'Failed to link repository');
    } finally {
      setIsLinking(false);
    }
  };

  const handleDisconnect = async () => {
    if (
      !window.confirm(
        'Are you sure you want to disconnect this repository? Automated deployments on push and pull requests will stop.'
      )
    ) {
      return;
    }
    setError(null);
    setIsDisconnecting(true);
    try {
      await api.disconnectProjectSource(projectId);
      await refetchConfig();
      if (onUpdate) onUpdate();
    } catch (err: any) {
      setError(err.message || 'Failed to disconnect repository');
    } finally {
      setIsDisconnecting(false);
    }
  };

  const copyWebhookUrl = (url: string) => {
    navigator.clipboard.writeText(url);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  // ----------------------------------------------------
  // Render: Connected State
  // ----------------------------------------------------
  if (sourceConfig?.repository) {
    const repo = sourceConfig.repository;
    const fullWebhookUrl =
      typeof window !== 'undefined'
        ? `${window.location.origin}${sourceConfig.webhook_url}`
        : sourceConfig.webhook_url || '';

    return (
      <div
        style={{
          backgroundColor: '#ffffff',
          border: '1px solid #e5e7eb',
          borderRadius: '8px',
          padding: '24px',
          boxShadow: '0 1px 3px rgba(0,0,0,0.05)',
        }}
      >
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'flex-start' }}>
          <div>
            <div style={{ display: 'flex', alignItems: 'center', gap: '8px', marginBottom: '4px' }}>
              <span
                style={{
                  textTransform: 'uppercase',
                  fontSize: '11px',
                  fontWeight: 700,
                  letterSpacing: '0.05em',
                  padding: '2px 8px',
                  borderRadius: '4px',
                  backgroundColor: '#eff6ff',
                  color: '#1d4ed8',
                }}
              >
                {sourceConfig.provider || 'git'}
              </span>
              <h2 style={{ margin: 0, fontSize: '18px', fontWeight: 600, color: '#111827' }}>
                {repo.full_name}
              </h2>
            </div>
            <p style={{ margin: '4px 0 0 0', fontSize: '13px', color: '#4b5563' }}>
              Branch: <code style={{ backgroundColor: '#f3f4f6', padding: '2px 6px', borderRadius: '4px' }}>{sourceConfig.target_branch || 'main'}</code>
            </p>
          </div>

          <button
            type="button"
            onClick={handleDisconnect}
            disabled={isDisconnecting}
            style={{
              padding: '6px 12px',
              backgroundColor: '#fee2e2',
              color: '#b91c1c',
              border: '1px solid #fecaca',
              borderRadius: '6px',
              fontSize: '13px',
              fontWeight: 500,
              cursor: isDisconnecting ? 'not-allowed' : 'pointer',
            }}
          >
            {isDisconnecting ? 'Disconnecting...' : 'Disconnect Repository'}
          </button>
        </div>

        {error && (
          <div
            role="alert"
            style={{
              marginTop: '16px',
              padding: '10px 14px',
              backgroundColor: '#fef2f2',
              border: '1px solid #fecaca',
              color: '#991b1b',
              borderRadius: '6px',
              fontSize: '13px',
            }}
          >
            {error}
          </div>
        )}

        <hr style={{ border: 'none', borderTop: '1px solid #e5e7eb', margin: '20px 0' }} />

        {/* Webhook Status & Details */}
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: '16px', marginBottom: '20px' }}>
          <div>
            <span style={{ display: 'block', fontSize: '12px', color: '#6b7280', marginBottom: '4px', fontWeight: 500 }}>
              Webhook Secret Status
            </span>
            <span style={{ fontSize: '13px', fontWeight: 600, color: '#059669' }}>
              {sourceConfig.has_webhook_secret ? 'Active (Configured)' : 'Not Configured'}
            </span>
          </div>

          <div>
            <span style={{ display: 'block', fontSize: '12px', color: '#6b7280', marginBottom: '4px', fontWeight: 500 }}>
              Last Delivery
            </span>
            <span style={{ fontSize: '13px', color: '#374151' }}>
              {sourceConfig.last_delivery_at
                ? new Date(sourceConfig.last_delivery_at).toLocaleString()
                : 'No webhooks received yet'}
            </span>
          </div>
        </div>

        {/* Webhook URL Box */}
        {sourceConfig.webhook_url && (
          <div>
            <label
              htmlFor="webhook-url-display"
              style={{ display: 'block', fontSize: '13px', fontWeight: 500, color: '#374151', marginBottom: '6px' }}
            >
              Webhook Payload URL
            </label>
            <div style={{ display: 'flex', gap: '8px' }}>
              <input
                id="webhook-url-display"
                type="text"
                readOnly
                value={fullWebhookUrl}
                style={{
                  flex: 1,
                  padding: '8px 12px',
                  borderRadius: '6px',
                  border: '1px solid #d1d5db',
                  backgroundColor: '#f9fafb',
                  fontSize: '13px',
                  fontFamily: 'monospace',
                  color: '#374151',
                }}
              />
              <button
                type="button"
                onClick={() => copyWebhookUrl(fullWebhookUrl)}
                style={{
                  padding: '8px 14px',
                  backgroundColor: '#f3f4f6',
                  color: '#374151',
                  border: '1px solid #d1d5db',
                  borderRadius: '6px',
                  fontSize: '13px',
                  fontWeight: 500,
                  cursor: 'pointer',
                }}
              >
                {copied ? 'Copied!' : 'Copy URL'}
              </button>
            </div>
            <p style={{ margin: '6px 0 0 0', fontSize: '12px', color: '#6b7280' }}>
              Configure this endpoint in your repository settings with Content type <code>application/json</code>.
            </p>
          </div>
        )}
      </div>
    );
  }

  // ----------------------------------------------------
  // Render: Disconnected State
  // ----------------------------------------------------
  return (
    <div
      style={{
        backgroundColor: '#ffffff',
        border: '1px solid #e5e7eb',
        borderRadius: '8px',
        padding: '24px',
        boxShadow: '0 1px 3px rgba(0,0,0,0.05)',
      }}
    >
      <h2 style={{ margin: '0 0 8px 0', fontSize: '18px', fontWeight: 600, color: '#111827' }}>
        Connect Git Repository
      </h2>
      <p style={{ margin: '0 0 20px 0', fontSize: '13px', color: '#6b7280' }}>
        Select your git provider, authenticate your account, and choose a repository to enable push and PR webhooks.
      </p>

      {error && (
        <div
          role="alert"
          style={{
            marginBottom: '16px',
            padding: '10px 14px',
            backgroundColor: '#fef2f2',
            border: '1px solid #fecaca',
            color: '#991b1b',
            borderRadius: '6px',
            fontSize: '13px',
          }}
        >
          {error}
        </div>
      )}

      {/* Provider Selector */}
      <div style={{ marginBottom: '20px' }}>
        <span style={{ display: 'block', fontSize: '13px', fontWeight: 500, color: '#374151', marginBottom: '8px' }}>
          Provider
        </span>
        <div style={{ display: 'flex', gap: '16px' }}>
          <label style={{ display: 'flex', alignItems: 'center', gap: '6px', fontSize: '14px', cursor: 'pointer' }}>
            <input
              type="radio"
              name="provider"
              value="github"
              checked={provider === 'github'}
              onChange={() => handleProviderChange('github')}
            />
            <span>GitHub</span>
          </label>
          <label style={{ display: 'flex', alignItems: 'center', gap: '6px', fontSize: '14px', cursor: 'pointer' }}>
            <input
              type="radio"
              name="provider"
              value="gitlab"
              checked={provider === 'gitlab'}
              onChange={() => handleProviderChange('gitlab')}
            />
            <span>GitLab</span>
          </label>
        </div>
      </div>

      {/* If repos are available, show selection form. Otherwise show OAuth connect button */}
      {repositories && repositories.length > 0 ? (
        <form onSubmit={handleLinkRepository} style={{ display: 'flex', flexDirection: 'column', gap: '16px' }}>
          <div>
            <label
              htmlFor="repo-select"
              style={{ display: 'block', fontSize: '13px', fontWeight: 500, color: '#374151', marginBottom: '6px' }}
            >
              Select Repository
            </label>
            <select
              id="repo-select"
              value={selectedRepoId}
              onChange={(e) => handleRepoChange(e.target.value)}
              style={{
                width: '100%',
                padding: '8px 12px',
                borderRadius: '6px',
                border: '1px solid #d1d5db',
                backgroundColor: '#ffffff',
                fontSize: '14px',
              }}
            >
              {repositories.map((r) => (
                <option key={r.id} value={r.id}>
                  {r.full_name}
                </option>
              ))}
            </select>
          </div>

          <div>
            <label
              htmlFor="target-branch"
              style={{ display: 'block', fontSize: '13px', fontWeight: 500, color: '#374151', marginBottom: '6px' }}
            >
              Target Branch
            </label>
            <input
              id="target-branch"
              type="text"
              required
              value={targetBranch}
              onChange={(e) => setTargetBranch(e.target.value)}
              placeholder="e.g. main"
              style={{
                width: '100%',
                padding: '8px 12px',
                borderRadius: '6px',
                border: '1px solid #d1d5db',
                fontSize: '14px',
                boxSizing: 'border-box',
                fontFamily: 'monospace',
              }}
            />
          </div>

          <div style={{ display: 'flex', gap: '12px', marginTop: '8px' }}>
            <button
              type="submit"
              disabled={isLinking}
              style={{
                padding: '8px 16px',
                backgroundColor: '#111827',
                color: '#ffffff',
                border: 'none',
                borderRadius: '6px',
                fontSize: '13px',
                fontWeight: 500,
                cursor: isLinking ? 'not-allowed' : 'pointer',
              }}
            >
              {isLinking ? 'Linking...' : 'Link Repository'}
            </button>
            <button
              type="button"
              onClick={handleConnectProvider}
              style={{
                padding: '8px 16px',
                backgroundColor: '#ffffff',
                color: '#374151',
                border: '1px solid #d1d5db',
                borderRadius: '6px',
                fontSize: '13px',
                fontWeight: 500,
                cursor: 'pointer',
              }}
            >
              Reconnect {provider === 'github' ? 'GitHub' : 'GitLab'}
            </button>
          </div>
        </form>
      ) : (
        <div>
          {isLoadingRepos ? (
            <p style={{ fontSize: '13px', color: '#6b7280' }}>Loading repositories...</p>
          ) : isReposError ? (
            <p style={{ fontSize: '13px', color: '#dc2626' }}>
              Failed to load repositories. Please reconnect your account.
            </p>
          ) : (
            <p style={{ fontSize: '13px', color: '#6b7280', marginBottom: '16px' }}>
              Connect your account to fetch your repositories.
            </p>
          )}

          <button
            type="button"
            onClick={handleConnectProvider}
            style={{
              padding: '8px 16px',
              backgroundColor: '#111827',
              color: '#ffffff',
              border: 'none',
              borderRadius: '6px',
              fontSize: '13px',
              fontWeight: 500,
              cursor: 'pointer',
            }}
          >
            Connect {provider === 'github' ? 'GitHub' : 'GitLab'}
          </button>
        </div>
      )}
    </div>
  );
};
